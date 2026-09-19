//! IWiaMiniDrv::drvReadItemProperties: refresh the root document-handling
//! status from the live device before the service returns it to applications.
//!
//! Every other advertised property is stored by the service and updated by
//! initialization or validation, so reading it needs no device access.

use super::validation_entry::{MAX_PROPERTY_SPECS, RequestedProperty, read_requested};
use super::{E_INVALIDARG, E_POINTER, S_OK, WIA_ITEM_TYPE_ROOT, helper_failure, native};
use std::{ffi::c_void, mem::MaybeUninit};

// SDK 10.0.26100.0 wiadef.h. The name is the one this driver registered in
// `catalog::PropertyCatalog::root`.
const WIA_DPS_DOCUMENT_HANDLING_STATUS: u32 = 3087;
const DOCUMENT_HANDLING_STATUS_NAME: &str = "Document Handling Status";
const FLAT_READY: i32 = 0x02;

fn requests_status(requested: &[RequestedProperty]) -> bool {
    requested.iter().any(|property| match property {
        RequestedProperty::Id(id) => *id == WIA_DPS_DOCUMENT_HANDLING_STATUS,
        RequestedProperty::Name(name) => name == DOCUMENT_HANDLING_STATUS_NAME,
    })
}

unsafe fn item_type(context: *mut u8) -> Result<i32, i32> {
    let mut value = MaybeUninit::<i32>::uninit();
    // SAFETY: WIA supplied the live opaque context; the local LONG is writable
    // storage matching the SDK declaration.
    let hr = unsafe { super::wiasGetItemType(context, value.as_mut_ptr()) };
    if hr != S_OK {
        return Err(helper_failure(hr));
    }
    // SAFETY: S_OK from wiasGetItemType initializes this output.
    Ok(unsafe { value.assume_init() })
}

/// # Safety
/// The receiver and context are live objects supplied by WIA. `specs` contains
/// `count` initialized SDK PROPSPECs, with valid terminated strings for names;
/// `error` is writable LONG storage throughout this synchronous callback.
pub(in crate::com_server::minidrv) unsafe extern "system" fn entry(
    this: *mut c_void,
    context: *mut u8,
    flags: i32,
    count: u32,
    specs: *const c_void,
    error: *mut i32,
) -> i32 {
    if error.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller supplies writable output storage.
    unsafe { *error = 0 };
    let result = super::super::super::catch_hresult(|| {
        if this.is_null() || context.is_null() || flags != 0 || count > MAX_PROPERTY_SPECS {
            return E_INVALIDARG;
        }
        if count != 0 && specs.is_null() {
            return E_POINTER;
        }
        // SAFETY: the WIA callback contract supplies the array and strings;
        // count is bounded above, and zero count never dereferences the pointer.
        let requested = match unsafe { read_requested(specs.cast(), count) } {
            Ok(requested) => requested,
            Err(error) => return error,
        };
        // SAFETY: the caller retains our live embedded COM interface.
        let interface = unsafe { &*this.cast::<super::super::Interface>() };
        if !requests_status(&requested) {
            // Nothing requested needs the device. Only confirm this object is
            // still connected; do not query the item type: the 2026-09-19 live
            // service read the WIA_DIP_* set through a generated compatibility
            // item whose driver-item flags are not available yet, and
            // wiasGetItemType on it fails. Every other advertised property is
            // stored by the service and updated by init/validation.
            let _connection = match super::super::locking::Borrow::take(interface) {
                Ok(connection) => connection,
                Err(hr) => return hr,
            };
            return S_OK;
        }
        // SAFETY: the borrow keeps the COM object live and Busy through the
        // service lock, the INQUIRY on the same STI session, and the write.
        unsafe {
            super::super::locking::with_live_capabilities_guarded(interface, |_caps, quarantine| {
                if item_type(context)? & WIA_ITEM_TYPE_ROOT == 0 {
                    // Flatbed items own no device-refreshed property.
                    return Ok(());
                }
                // A successful INQUIRY on the exclusive session is the only
                // readiness signal the protocol exposes; failures above return
                // their HRESULT and leave the stored status untouched.
                *quarantine = true;
                native::write_long(context, WIA_DPS_DOCUMENT_HANDLING_STATUS, FLAT_READY)
            })
        }
        .map_or_else(|error| error, |_| S_OK)
    });
    // SAFETY: output storage remains valid until the callback returns.
    unsafe { super::super::report(error, result) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ptr;

    #[test]
    fn only_document_handling_status_triggers_a_device_query() {
        assert!(!requests_status(&[]));
        assert!(!requests_status(&[
            RequestedProperty::Id(4098),
            RequestedProperty::Name("Item Category".to_owned()),
        ]));
        assert!(requests_status(&[
            RequestedProperty::Id(4098),
            RequestedProperty::Id(WIA_DPS_DOCUMENT_HANDLING_STATUS),
        ]));
        assert!(requests_status(&[RequestedProperty::Name(
            DOCUMENT_HANDLING_STATUS_NAME.to_owned()
        )]));
    }

    #[test]
    fn entry_rejects_absent_inputs_before_any_service_call() {
        let mut error = 7;
        // SAFETY: null inputs and a valid output; no service context is used.
        unsafe {
            assert_eq!(
                entry(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    0,
                    0,
                    ptr::null(),
                    &mut error
                ),
                E_INVALIDARG
            );
            assert_eq!(error, E_INVALIDARG);
            assert_eq!(
                entry(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    0,
                    0,
                    ptr::null(),
                    ptr::null_mut()
                ),
                E_POINTER
            );
            let this = 8usize as *mut c_void;
            let context = 8usize as *mut u8;
            assert_eq!(
                entry(this, context, 1, 0, ptr::null(), &mut error),
                E_INVALIDARG,
                "reserved flags are rejected before the receiver is used"
            );
            assert_eq!(
                entry(
                    this,
                    context,
                    0,
                    MAX_PROPERTY_SPECS + 1,
                    ptr::null(),
                    &mut error
                ),
                E_INVALIDARG
            );
            assert_eq!(
                entry(this, context, 0, 1, ptr::null(), &mut error),
                E_POINTER,
                "a non-zero count needs the PROPSPEC array"
            );
        }
    }
}
