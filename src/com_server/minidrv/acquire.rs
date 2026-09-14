//! WIA 2 stream-download entry, reusing the locked STI scan and BMP pipeline.

use super::{
    E_INVALIDARG, E_POINTER, E_UNEXPECTED, Guid, Instance, Interface, locking, properties,
};
use crate::wia_transfer::TransferOutcome;
use std::{ffi::c_void, io, mem::size_of, sync::atomic::AtomicBool};

// SDK wiamindr_lh.h, MINIDRV_TRANSFER_CONTEXT. These are service-owned fields,
// not a driver context. Stream-mode output is delivered through the callback;
// legacy file handles and transfer buffers must never be accessed directly.
#[repr(C)]
struct TransferContext {
    size: i32,
    width: i32,
    lines: i32,
    depth: i32,
    x_resolution: i32,
    y_resolution: i32,
    compression: i32,
    format: Guid,
    tymed: i32,
    file: isize,
    offset: i32,
    buffer_size: i32,
    active_buffer: i32,
    num_buffers: i32,
    base_buffer: *mut u8,
    transfer_buffer: *mut u8,
    transfer_data_callback: i32,
    class_allocated_buffer: i32,
    client_address: isize,
    callback: *mut c_void,
    image_size: i32,
    header_size: i32,
    item_size: i32,
    width_bytes: i32,
    page: i32,
    current_ifd_offset: i32,
    previous_ifd_offset: i32,
}

fn failed_status(hr: i32) -> i32 {
    if hr < 0 { hr } else { E_UNEXPECTED }
}

fn error_status(error: &io::Error) -> i32 {
    if let Some(inner) = error.get_ref() {
        if let Some(transfer) = inner.downcast_ref::<crate::wia_transfer::TransferError>() {
            return error_status(transfer.original());
        }
        if let Some(stream) = inner.downcast_ref::<crate::com_stream::StreamError>() {
            return failed_status(stream.hresult());
        }
        if let Some(callback) = inner.downcast_ref::<crate::wia_callback::CallbackError>() {
            return failed_status(callback.hresult());
        }
    }
    if let Some(code) = error.raw_os_error() {
        return if code < 0 {
            code
        } else if code > 0 {
            (0x80070000u32 | (code as u32 & 0xffff)) as i32
        } else {
            E_UNEXPECTED
        };
    }
    match error.kind() {
        io::ErrorKind::InvalidInput => E_INVALIDARG,
        io::ErrorKind::WouldBlock => super::WIA_ERROR_BUSY,
        io::ErrorKind::PermissionDenied => 0x80070005u32 as i32,
        io::ErrorKind::NotConnected => 0x80210005u32 as i32, // WIA_ERROR_OFFLINE
        io::ErrorKind::OutOfMemory => 0x8007000eu32 as i32,
        _ => 0x80004005u32 as i32, // E_FAIL, including interrupted output I/O
    }
}

fn outcome(result: io::Result<TransferOutcome>) -> i32 {
    match result {
        Ok(TransferOutcome::Completed(_) | TransferOutcome::Skipped) => 0,
        Ok(TransferOutcome::Cancelled) => 1, // S_FALSE only for explicit cancellation
        Err(error) => error_status(&error),
    }
}

pub(super) fn dispatch(
    interface: &Interface,
    read: impl FnOnce() -> Result<properties::Snapshot, i32>,
    transfer: impl FnOnce(properties::Snapshot) -> io::Result<TransferOutcome>,
) -> i32 {
    let _connection = match locking::Borrow::take(interface) {
        Ok(connection) => connection,
        Err(hr) => return hr,
    };
    match read() {
        Ok(snapshot) => outcome(transfer(snapshot)),
        Err(hr) => failed_status(hr),
    }
}

pub(super) unsafe extern "system" fn entry(
    this: *mut c_void,
    context: *mut u8,
    flags: i32,
    transfer: *mut c_void,
    error: *mut i32,
) -> i32 {
    if error.is_null() {
        return E_POINTER;
    }
    let result = super::super::catch_hresult(|| {
        if this.is_null() || context.is_null() || transfer.is_null() || flags != 2 {
            return E_INVALIDARG;
        }
        // SAFETY: WIA supplies live native pointers throughout this synchronous
        // call. Inspect lSize first, before accessing later SDK structure fields.
        unsafe {
            if *transfer.cast::<i32>() != size_of::<TransferContext>() as i32 {
                return E_INVALIDARG;
            }
            let callback = (*transfer.cast::<TransferContext>()).callback;
            if callback.is_null() {
                return E_POINTER;
            }
            let interface = &*this.cast::<Interface>();
            // The service already owns the WIA lock. Borrow the same STI USB
            // session; callback reentry cannot disconnect this item tree. The
            // callback handles cancellation; WIA_EVENT_CANCEL_IO is still pending.
            let cancel = AtomicBool::new(false);
            dispatch(
                interface,
                || properties::read(context),
                |snapshot| {
                    (*super::owner(this).cast::<Instance>()).state.transfer_bmp(
                        snapshot.settings,
                        &cancel,
                        callback,
                        &snapshot.item,
                        &snapshot.full_item,
                    )
                },
            )
        }
    });
    // SAFETY: the caller supplies writable error storage retained across callbacks.
    unsafe { super::report(error, result) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        mem::{offset_of, size_of},
        ptr,
    };

    #[test]
    fn transfer_context_matches_sdk_and_rejects_absent_inputs() {
        assert_eq!(size_of::<TransferContext>(), 144);
        assert_eq!(offset_of!(TransferContext, callback), 104);
        assert_eq!(offset_of!(TransferContext, image_size), 112);
        let mut error = 7;
        // SAFETY: null inputs and valid output; no service context is fabricated.
        unsafe {
            assert_eq!(
                entry(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    2,
                    ptr::null_mut(),
                    &mut error
                ),
                E_INVALIDARG
            );
            assert_eq!(error, E_INVALIDARG);
            assert_eq!(
                entry(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    2,
                    ptr::null_mut(),
                    ptr::null_mut()
                ),
                E_POINTER
            );
        }
    }

    #[test]
    fn cancellation_and_skipping_are_distinct_from_io_failure() {
        assert_eq!(failed_status(1), E_UNEXPECTED);
        assert_eq!(failed_status(0), E_UNEXPECTED);
        assert_eq!(outcome(Ok(TransferOutcome::Cancelled)), 1);
        assert_eq!(outcome(Ok(TransferOutcome::Skipped)), 0);
        assert_eq!(
            outcome(Ok(TransferOutcome::Completed(crate::scan::ScanSummary {
                width: 4,
                height: 3,
                bands: 1,
                bytes: 12,
            }))),
            0
        );
        for kind in [
            io::ErrorKind::Interrupted,
            io::ErrorKind::Other,
            io::ErrorKind::InvalidData,
        ] {
            assert!(outcome(Err(io::Error::new(kind, "synthetic output failure"))) < 0);
        }
        assert_eq!(
            outcome(Err(io::Error::from_raw_os_error(5))),
            0x80070005u32 as i32
        );
        assert_eq!(
            outcome(Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "synthetic busy"
            ))),
            super::super::WIA_ERROR_BUSY
        );
    }

    #[test]
    fn structured_callback_hresult_is_preserved_as_failure() {
        // SAFETY: null callback is rejected without touching a native pointer.
        let error =
            unsafe { crate::wia_callback::TransferCallback::query_from_borrowed(ptr::null_mut()) }
                .unwrap_err();
        assert_eq!(error_status(&error), E_POINTER);
    }
}
