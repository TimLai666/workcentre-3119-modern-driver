#![cfg(windows)]

//! Owned, thread-bound access to WIA's stream-transfer callback.
//!
//! The WIA service supplies an `IWiaMiniDrvCallBack` borrowed pointer.  This
//! module queries the `IWiaMiniDrvTransferCallback` interface, owns that one
//! query reference, and translates only the callback-specific status values.

use std::{
    ffi::c_void,
    fmt, io,
    marker::PhantomData,
    ptr::{self, NonNull},
    rc::Rc,
};

use crate::com_stream::ComOutputStream;

type HResult = i32;
type BStr = *mut u16;

const S_OK: HResult = 0;
const S_FALSE: HResult = 1;
const E_POINTER: HResult = 0x8000_4003u32 as HResult;
const E_NOINTERFACE: HResult = 0x8000_4002u32 as HResult;
const E_INVALIDARG: HResult = 0x8007_0057u32 as HResult;
const E_OUTOFMEMORY: HResult = 0x8007_000Eu32 as HResult;

const WIA_STATUS_SKIP_ITEM: HResult = 0x0021_0009;
const WIA_TRANSFER_MSG_STATUS: HResult = 0x0000_0001;

const MAX_BSTR_CODE_UNITS: usize = 16 * 1024;

#[repr(C)]
#[derive(Clone, Copy)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const IID_IWIA_MINI_DRV_TRANSFER_CALLBACK: Guid = Guid {
    data1: 0xa9d2_ee89,
    data2: 0x2ce5,
    data3: 0x4ff0,
    data4: [0x8a, 0xdb, 0xc9, 0x61, 0xd1, 0xd7, 0x74, 0xca],
};

#[repr(C)]
struct UnknownVtable {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}

#[repr(C)]
struct TransferCallbackVtable {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_next_stream:
        unsafe extern "system" fn(*mut c_void, HResult, BStr, BStr, *mut *mut c_void) -> HResult,
    send_message:
        unsafe extern "system" fn(*mut c_void, HResult, *const WiaTransferParams) -> HResult,
}

#[repr(C)]
struct WiaTransferParams {
    message: HResult,
    percent_complete: HResult,
    transferred_bytes: u64,
    error_status: HResult,
}

/// Original HRESULT returned by an WIA transfer callback operation.
#[derive(Debug)]
pub struct CallbackError {
    operation: &'static str,
    hresult: HResult,
}

impl CallbackError {
    /// Returns the unchanged HRESULT, including unexpected positive statuses.
    pub fn hresult(&self) -> HResult {
        self.hresult
    }
}

impl fmt::Display for CallbackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "WIA transfer callback {} returned HRESULT 0x{:08X}",
            self.operation, self.hresult as u32
        )
    }
}

impl std::error::Error for CallbackError {}

/// Result of requesting the next WIA destination stream.
pub enum NextStream {
    /// One owned COM stream reference, released when this value is dropped.
    Stream(ComOutputStream),
    /// WIA cancelled the current transfer sequence.
    Cancelled,
    /// WIA requested that the current image be skipped.
    Skipped,
}

impl fmt::Debug for NextStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stream(_) => f.write_str("Stream(..)"),
            Self::Cancelled => f.write_str("Cancelled"),
            Self::Skipped => f.write_str("Skipped"),
        }
    }
}

/// Result of sending a WIA transfer progress message.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CallbackStatus {
    Continue,
    Cancelled,
}

/// Owns one `IWiaMiniDrvTransferCallback` query reference and is bound to its
/// originating COM apartment and thread.
pub struct TransferCallback {
    raw: NonNull<c_void>,
    apartment: PhantomData<Rc<()>>,
}

impl fmt::Debug for TransferCallback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TransferCallback")
            .field("raw", &self.raw)
            .finish_non_exhaustive()
    }
}

impl TransferCallback {
    /// Queries `IWiaMiniDrvTransferCallback` from a borrowed COM callback.
    ///
    /// The returned wrapper owns exactly the reference produced by
    /// `QueryInterface`; it releases that reference once on drop. The caller
    /// must keep the borrowed callback and its COM apartment alive until this
    /// value is dropped, and must call this method on the originating thread.
    ///
    /// # Safety
    /// If non-null, `raw` must point to a live COM interface whose first three
    /// vtable entries are the standard `IUnknown` methods. A null pointer is
    /// rejected with `E_POINTER`. The object must remain callable on this thread
    /// until the returned wrapper drops.
    pub unsafe fn query_from_borrowed(raw: *mut c_void) -> io::Result<Self> {
        let raw = NonNull::new(raw).ok_or_else(|| callback_error("QueryInterface", E_POINTER))?;
        // SAFETY: the caller's safety contract guarantees a live IUnknown vtable.
        let vtable = unsafe { &**raw.as_ptr().cast::<*const UnknownVtable>() };
        let mut queried = ptr::null_mut();
        // SAFETY: the caller's safety contract guarantees the live IUnknown vtable and output.
        let hresult = unsafe {
            (vtable.query_interface)(
                raw.as_ptr(),
                &IID_IWIA_MINI_DRV_TRANSFER_CALLBACK,
                &mut queried,
            )
        };
        if hresult != S_OK {
            return Err(callback_error("QueryInterface", hresult));
        }
        let queried =
            NonNull::new(queried).ok_or_else(|| callback_error("QueryInterface", E_POINTER))?;
        Ok(Self {
            raw: queried,
            apartment: PhantomData,
        })
    }

    /// Obtains the next destination stream for one WIA item.
    pub fn next_stream(&mut self, item: &str, full: &str) -> io::Result<NextStream> {
        let item = BString::new(item)?;
        let full = BString::new(full)?;
        let mut stream = ptr::null_mut();
        // SAFETY: BSTR guards keep both strings alive for the synchronous COM call;
        // the wrapper owns a live callback reference on this thread.
        let hresult = unsafe {
            (self.vtable().get_next_stream)(
                self.raw.as_ptr(),
                0,
                item.as_ptr(),
                full.as_ptr(),
                &mut stream,
            )
        };
        match hresult {
            S_OK => {
                let stream = NonNull::new(stream)
                    .ok_or_else(|| callback_error("GetNextStream", E_POINTER))?;
                // SAFETY: S_OK transfers one owned IStream reference to the caller.
                let stream = unsafe { ComOutputStream::from_raw_owned(stream.as_ptr()) }?;
                Ok(NextStream::Stream(stream))
            }
            S_FALSE => {
                release_optional_interface(stream);
                Ok(NextStream::Cancelled)
            }
            WIA_STATUS_SKIP_ITEM => {
                release_optional_interface(stream);
                Ok(NextStream::Skipped)
            }
            hresult => {
                release_optional_interface(stream);
                Err(callback_error("GetNextStream", hresult))
            }
        }
    }

    /// Sends a progress status message for the current transfer.
    pub fn status(&mut self, percent: u32, bytes: u64) -> io::Result<CallbackStatus> {
        self.send_message(WIA_TRANSFER_MSG_STATUS, percent, bytes)
    }

    fn send_message(
        &mut self,
        message: HResult,
        percent: u32,
        bytes: u64,
    ) -> io::Result<CallbackStatus> {
        if percent > 100 {
            return Err(callback_error("SendMessage", E_INVALIDARG));
        }
        let params = WiaTransferParams {
            message,
            percent_complete: percent as HResult,
            transferred_bytes: bytes,
            error_status: S_OK,
        };
        // SAFETY: params is a valid SDK-layout input block for the synchronous COM call.
        let hresult = unsafe { (self.vtable().send_message)(self.raw.as_ptr(), 0, &params) };
        match hresult {
            S_OK => Ok(CallbackStatus::Continue),
            S_FALSE => Ok(CallbackStatus::Cancelled),
            hresult => Err(callback_error("SendMessage", hresult)),
        }
    }

    fn vtable(&self) -> &TransferCallbackVtable {
        // SAFETY: QueryInterface returned an interface with the SDK vtable layout;
        // the owned reference remains live until Drop on this thread.
        unsafe { &**self.raw.as_ptr().cast::<*const TransferCallbackVtable>() }
    }
}

impl Drop for TransferCallback {
    fn drop(&mut self) {
        // SAFETY: the wrapper owns exactly one QI reference and releases it once,
        // before its originating COM apartment may be torn down.
        unsafe {
            (self.vtable().release)(self.raw.as_ptr());
        }
    }
}

fn release_optional_interface(raw: *mut c_void) {
    if raw.is_null() {
        return;
    }
    // SAFETY: a non-null callback output is an owned COM interface reference under
    // the out-parameter contract; release it once when the status does not consume it.
    unsafe {
        let vtable = &**raw.cast::<*const UnknownVtable>();
        (vtable.release)(raw);
    }
}

fn callback_error(operation: &'static str, hresult: HResult) -> io::Error {
    let kind = match hresult {
        E_POINTER | E_INVALIDARG => io::ErrorKind::InvalidInput,
        E_OUTOFMEMORY => io::ErrorKind::OutOfMemory,
        E_NOINTERFACE => io::ErrorKind::Unsupported,
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, CallbackError { operation, hresult })
}

struct BString {
    raw: NonNull<u16>,
}

impl BString {
    fn new(value: &str) -> io::Result<Self> {
        let utf16: Vec<u16> = value.encode_utf16().take(MAX_BSTR_CODE_UNITS + 1).collect();
        if utf16.len() > MAX_BSTR_CODE_UNITS {
            return Err(callback_error("BSTR allocation", E_INVALIDARG));
        }
        let units = utf16.len();
        let empty = [0u16; 1];
        let source = if utf16.is_empty() {
            empty.as_ptr()
        } else {
            utf16.as_ptr()
        };
        // SAFETY: source points to at least `units` UTF-16 code units and the count
        // is bounded to UINT; OleAut32 returns an allocator-owned BSTR.
        let raw = unsafe { SysAllocStringLen(source, units as u32) };
        let raw =
            NonNull::new(raw).ok_or_else(|| callback_error("BSTR allocation", E_OUTOFMEMORY))?;
        Ok(Self { raw })
    }

    fn as_ptr(&self) -> BStr {
        self.raw.as_ptr()
    }
}

impl Drop for BString {
    fn drop(&mut self) {
        // SAFETY: raw was returned by SysAllocStringLen and is released exactly once.
        unsafe { SysFreeString(self.raw.as_ptr()) }
    }
}

#[link(name = "OleAut32")]
unsafe extern "system" {
    fn SysAllocStringLen(value: *const u16, length: u32) -> *mut u16;
    fn SysFreeString(value: *mut u16);
}
