use super::{E_INVALIDARG, E_POINTER};
use crate::com_server::Guid;
use std::{ffi::c_void, mem::MaybeUninit, ptr};

const S_OK: i32 = 0;
const E_UNEXPECTED: i32 = 0x8000_ffffu32 as i32;

// SDK 10.0.26100.0 wiadef.h item type constants.
const WIA_ITEM_TYPE_IMAGE: i32 = 0x0000_0001;
const WIA_ITEM_TYPE_ROOT: i32 = 0x0000_0008;
const WIA_ITEM_TYPE_FOLDER: i32 = 0x0000_0004;
const WIA_ITEM_TYPE_TRANSFER: i32 = 0x0000_2000;

// SDK 10.0.26100.0 wiadef.h image format and TYMED constants.
const WIA_IMG_FMT_BMP: Guid = Guid {
    data1: 0xb96b_3cab,
    data2: 0x0728,
    data3: 0x11d3,
    data4: [0x9d, 0x7b, 0, 0, 0xf8, 0x1e, 0xf3, 0x2e],
};
const TYMED_FILE: i32 = 2;

// WIA owns no reference to this static array. Keeping it immutable for the
// process lifetime avoids an allocation that would need to be threaded through
// every driver-item context and freed by drvFreeDrvItemContext.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WiaFormatInfo {
    guid_format_id: Guid,
    tymed: i32,
}

static FORMAT_INFOS: [WiaFormatInfo; 1] = [WiaFormatInfo {
    guid_format_id: WIA_IMG_FMT_BMP,
    tymed: TYMED_FILE,
}];

fn select_formats(item_type: i32) -> Result<&'static [WiaFormatInfo], i32> {
    // A root or folder has no directly transferable image in this flatbed
    // driver. Require the same image/transfer flags used by the item tree.
    if item_type & (WIA_ITEM_TYPE_ROOT | WIA_ITEM_TYPE_FOLDER) != 0
        || item_type & WIA_ITEM_TYPE_IMAGE == 0
        || item_type & WIA_ITEM_TYPE_TRANSFER == 0
    {
        return Err(E_INVALIDARG);
    }
    Ok(&FORMAT_INFOS)
}

/// Publish the driver-owned format table for a validated item type.
///
/// The native entry calls this only after obtaining the item type from the WIA
/// service. `formats` is optional per the SDK; `count` is required.
///
/// # Safety
/// When non-null, `count` points to writable `LONG` storage and `formats`
/// points to writable storage for one pointer. The caller must keep both live
/// for this synchronous call.
unsafe fn publish_formats(item_type: i32, count: *mut i32, formats: *mut *mut c_void) -> i32 {
    if !count.is_null() {
        // SAFETY: guaranteed by this function's contract.
        unsafe { *count = 0 };
    }
    if !formats.is_null() {
        // SAFETY: guaranteed by this function's contract.
        unsafe { *formats = ptr::null_mut() };
    }
    if count.is_null() {
        return E_INVALIDARG;
    }
    let infos = match select_formats(item_type) {
        Ok(infos) => infos,
        Err(hr) => return hr,
    };
    // SAFETY: `count` was checked non-null and `formats`, when present, was
    // supplied as writable output by the native caller.
    unsafe {
        *count = infos.len() as i32;
        if !formats.is_null() {
            *formats = infos.as_ptr().cast_mut().cast();
        }
    }
    S_OK
}

fn helper_failure(hr: i32) -> i32 {
    if hr < 0 { hr } else { E_UNEXPECTED }
}

/// Return the one BMP/file pair supported by this flatbed minidriver.
///
/// The WIA service context is opaque and is passed only to
/// `wiasGetItemType`. The lifecycle borrow keeps the initialized connection
/// and its retained helper alive while that native call runs.
///
/// # Safety
/// `this` must be this module's live `IWiaMiniDrv` interface, `context` must be
/// a live WIA service item context for the synchronous call, and non-null
/// outputs must point to writable storage of the SDK-declared types. The WIA
/// service may omit `formats`, as permitted by the SDK.
pub(super) unsafe extern "system" fn entry(
    this: *mut c_void,
    context: *mut u8,
    flags: i32,
    count: *mut i32,
    formats: *mut *mut c_void,
    error: *mut i32,
) -> i32 {
    // Clear optional outputs before validating the rest, matching the other
    // minidriver list entry points and preventing stale success data.
    if !count.is_null() {
        // SAFETY: a non-null output is writable for this COM call.
        unsafe { *count = 0 };
    }
    if !formats.is_null() {
        // SAFETY: a non-null output is writable for this COM call.
        unsafe { *formats = ptr::null_mut() };
    }
    if error.is_null() {
        return E_POINTER;
    }
    let result = super::super::catch_hresult(|| {
        if this.is_null() || context.is_null() || count.is_null() || flags != 0 {
            return E_INVALIDARG;
        }
        // SAFETY: the COM receiver is one of this driver's embedded interfaces.
        // Borrow::take moves the connected resources out of the lifecycle state
        // and blocks reentrant disconnect/lock/acquire while WIA is queried.
        let _connection =
            match super::locking::Borrow::take(unsafe { &*this.cast::<super::Interface>() }) {
                Ok(connection) => connection,
                Err(hr) => return hr,
            };
        let mut item_type = MaybeUninit::<i32>::uninit();
        // SAFETY: WIA owns and validates the opaque context for this callback;
        // the local LONG is writable storage for the helper's output.
        let hr = unsafe { wiasGetItemType(context, item_type.as_mut_ptr()) };
        if hr != S_OK {
            return helper_failure(hr);
        }
        // SAFETY: WiasGetItemType writes the LONG before returning S_OK.
        let item_type = unsafe { item_type.assume_init() };
        // SAFETY: outputs were checked by the caller contract and the helper
        // clears and publishes them synchronously.
        unsafe { publish_formats(item_type, count, formats) }
    });
    // SAFETY: `error` was checked non-null and remains valid for this call.
    unsafe { super::report(error, result) }
}

#[link(name = "Wiaservc")]
unsafe extern "system" {
    // SDK 10.0.26100.0 wiamdef.h declaration of wiasGetItemType.
    fn wiasGetItemType(context: *mut u8, item_type: *mut i32) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_server::Guid;
    use std::{
        ffi::c_void,
        mem::{offset_of, size_of},
        ptr,
    };

    const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
    const E_POINTER: i32 = 0x8000_4003u32 as i32;
    const S_OK: i32 = 0;
    const ROOT_TYPE: i32 = 0x0000_004c;
    const FLATBED_TYPE: i32 = 0x0008_2003;
    const IMAGE_WITHOUT_TRANSFER: i32 = 0x0000_0003;
    const TYMED_FILE: i32 = 2;

    fn bmp_guid() -> Guid {
        Guid {
            data1: 0xb96b_3cab,
            data2: 0x0728,
            data3: 0x11d3,
            data4: [0x9d, 0x7b, 0, 0, 0xf8, 0x1e, 0xf3, 0x2e],
        }
    }

    #[test]
    fn synthetic_item_type_selector_accepts_flatbed_only() {
        assert_eq!(select_formats(FLATBED_TYPE).unwrap().len(), 1);
        for item_type in [ROOT_TYPE, IMAGE_WITHOUT_TRANSFER, 0x0000_2005] {
            assert_eq!(select_formats(item_type), Err(E_INVALIDARG));
        }
    }

    #[test]
    fn synthetic_publisher_returns_one_bmp_file_pair() {
        assert_eq!(size_of::<WiaFormatInfo>(), 20);
        assert_eq!(offset_of!(WiaFormatInfo, tymed), 16);
        let mut count = -1;
        let mut formats = ptr::null_mut();
        // SAFETY: both outputs are live writable local storage.
        let result = unsafe { publish_formats(FLATBED_TYPE, &mut count, &mut formats) };
        assert_eq!(result, S_OK);
        assert_eq!(count, 1);
        assert!(!formats.is_null());
        // SAFETY: publish_formats returned the immutable driver-owned entry.
        let info = unsafe { &*formats.cast::<WiaFormatInfo>() };
        assert_eq!(info.guid_format_id, bmp_guid());
        assert_eq!(info.tymed, TYMED_FILE);
    }

    #[test]
    fn formats_output_is_optional_but_count_and_error_are_required() {
        let mut count = -1;
        // SAFETY: count is writable; the optional table output is omitted.
        let result = unsafe { publish_formats(FLATBED_TYPE, &mut count, ptr::null_mut()) };
        assert_eq!(result, S_OK);
        assert_eq!(count, 1);

        // SAFETY: the helper rejects absent outputs without dereferencing them.
        let result = unsafe { publish_formats(FLATBED_TYPE, ptr::null_mut(), ptr::null_mut()) };
        assert_eq!(result, E_INVALIDARG);
    }

    #[test]
    fn native_entry_rejects_invalid_inputs_without_fabricating_context() {
        let mut count = -1;
        let mut formats = ptr::dangling_mut::<c_void>();
        let mut error = 7;
        // SAFETY: null context is intentionally rejected before any WIA helper.
        let result = unsafe {
            entry(
                ptr::dangling_mut::<c_void>(),
                ptr::null_mut(),
                0,
                &mut count,
                &mut formats,
                &mut error,
            )
        };
        assert_eq!(result, E_INVALIDARG);
        assert_eq!(error, E_INVALIDARG);
        assert_eq!(count, 0);
        assert!(formats.is_null());

        // SAFETY: missing device-error output is rejected before native work.
        assert_eq!(
            // SAFETY: all inputs are null and rejected before dereferencing.
            unsafe {
                entry(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    0,
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                )
            },
            E_POINTER
        );
    }
}
