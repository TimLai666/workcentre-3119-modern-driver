//! IWiaMiniDrv::drvGetDeviceErrorStr for this driver's own device-error values.
//!
//! Every minidriver entry reports its failing HRESULT through `plDevErrVal`
//! (see `report`), so the device-error vocabulary the service can hand back to
//! this method is exactly that HRESULT set. Zero means no error.

use super::{E_INVALIDARG, E_NOTIMPL, E_POINTER, E_UNEXPECTED, WIA_ERROR_BUSY};
use std::{ffi::c_void, ptr};

const S_OK: i32 = 0;
const E_FAIL: i32 = 0x8000_4005u32 as i32;
const E_OUTOFMEMORY: i32 = 0x8007_000eu32 as i32;
const E_ACCESSDENIED: i32 = 0x8007_0005u32 as i32;
// SDK 10.0.26100.0 wiadef.h: MAKE_HRESULT(SEVERITY_ERROR, FACILITY_WIA, n).
const WIA_ERROR_GENERAL_ERROR: i32 = 0x8021_0001u32 as i32;
const WIA_ERROR_PAPER_JAM: i32 = 0x8021_0002u32 as i32;
const WIA_ERROR_PAPER_EMPTY: i32 = 0x8021_0003u32 as i32;
const WIA_ERROR_PAPER_PROBLEM: i32 = 0x8021_0004u32 as i32;
const WIA_ERROR_OFFLINE: i32 = 0x8021_0005u32 as i32;
const WIA_ERROR_WARMING_UP: i32 = 0x8021_0007u32 as i32;
const WIA_ERROR_USER_INTERVENTION: i32 = 0x8021_0008u32 as i32;
const WIA_ERROR_ITEM_DELETED: i32 = 0x8021_0009u32 as i32;
const WIA_ERROR_DEVICE_COMMUNICATION: i32 = 0x8021_000au32 as i32;
const WIA_ERROR_INVALID_COMMAND: i32 = 0x8021_000bu32 as i32;
const WIA_ERROR_INCORRECT_HARDWARE_SETTING: i32 = 0x8021_000cu32 as i32;
const WIA_ERROR_DEVICE_LOCKED: i32 = 0x8021_000du32 as i32;
const WIA_ERROR_EXCEPTION_IN_DRIVER: i32 = 0x8021_000eu32 as i32;
const WIA_ERROR_INVALID_DRIVER_RESPONSE: i32 = 0x8021_000fu32 as i32;
const WIA_ERROR_COVER_OPEN: i32 = 0x8021_0010u32 as i32;
// SDK winerror.h HRESULT_FROM_WIN32 layout: 0x8007xxxx carries a Win32 code.
const FACILITY_WIN32_PREFIX: u32 = 0x8007_0000;

const FORMAT_MESSAGE_IGNORE_INSERTS: u32 = 0x200;
const FORMAT_MESSAGE_FROM_SYSTEM: u32 = 0x1000;
const MAX_SYSTEM_MESSAGE_UNITS: usize = 512;

/// Map a device-error value to end-user text. `None` means the value is not
/// one this driver reports, which the SDK contract maps to E_INVALIDARG.
pub(super) fn describe(code: i32) -> Option<String> {
    let fixed = match code {
        0 => "No error.",
        WIA_ERROR_BUSY => {
            "The scanner is busy with another operation. Wait for it to finish and try again."
        }
        WIA_ERROR_OFFLINE => {
            "The scanner is not responding. Check that it is switched on and connected by USB."
        }
        WIA_ERROR_GENERAL_ERROR => "The scanner reported a general error.",
        WIA_ERROR_PAPER_JAM => "The scanner reported a paper jam.",
        WIA_ERROR_PAPER_EMPTY => "The scanner reported that no paper is loaded.",
        WIA_ERROR_PAPER_PROBLEM => "The scanner reported a paper handling problem.",
        WIA_ERROR_WARMING_UP => "The scanner is warming up. Try again shortly.",
        WIA_ERROR_USER_INTERVENTION => "The scanner needs attention before it can continue.",
        WIA_ERROR_ITEM_DELETED => "The scan item no longer exists.",
        WIA_ERROR_DEVICE_COMMUNICATION => "Communication with the scanner failed.",
        WIA_ERROR_INVALID_COMMAND => "The scanner rejected the command.",
        WIA_ERROR_INCORRECT_HARDWARE_SETTING => {
            "The requested scan settings are not supported by the scanner."
        }
        WIA_ERROR_DEVICE_LOCKED => "The scanner is locked by another application.",
        WIA_ERROR_EXCEPTION_IN_DRIVER => "The scanner driver stopped unexpectedly.",
        WIA_ERROR_INVALID_DRIVER_RESPONSE => "The scanner returned an unexpected response.",
        WIA_ERROR_COVER_OPEN => "The scanner cover is open.",
        E_INVALIDARG => "The scan request contained an invalid setting or argument.",
        E_POINTER => "The scan request was missing required data.",
        E_NOTIMPL => "The requested operation is not supported by this scanner driver.",
        E_UNEXPECTED => "The scanner driver encountered an unexpected internal state.",
        E_FAIL => "The scan failed.",
        E_OUTOFMEMORY => "There is not enough memory to complete the scan.",
        E_ACCESSDENIED => "Access to the scanner was denied.",
        _ => "",
    };
    if !fixed.is_empty() {
        return Some(fixed.to_owned());
    }
    let raw = code as u32;
    if raw & 0xffff_0000 != FACILITY_WIN32_PREFIX {
        return None;
    }
    // Win32-derived HRESULTs come from USB, PnP and file access. Windows owns
    // their localized text; fall back to the code when it has none.
    let win32 = raw & 0xffff;
    Some(system_message(win32).unwrap_or_else(|| format!("Windows error {win32}.")))
}

fn system_message(win32: u32) -> Option<String> {
    let mut buffer = [0u16; MAX_SYSTEM_MESSAGE_UNITS];
    // SAFETY: the buffer is writable for exactly its declared length, no
    // insert arguments are used, and the source pointer is unused for
    // FORMAT_MESSAGE_FROM_SYSTEM.
    let written = unsafe {
        FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
            ptr::null(),
            win32,
            0,
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            ptr::null(),
        )
    } as usize;
    if written == 0 || written > buffer.len() {
        return None;
    }
    let text = String::from_utf16_lossy(&buffer[..written]);
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_owned())
    }
}

/// Allocate `text` as a NUL-terminated OLE string owned by the caller.
///
/// # Safety
/// `output` must point to writable pointer storage for this synchronous call.
unsafe fn allocate_ole_string(text: &str, output: *mut *mut u16) -> Result<(), i32> {
    let units: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = units.len().checked_mul(2).ok_or(E_OUTOFMEMORY)?;
    // SAFETY: CoTaskMemAlloc returns a block of at least `bytes` bytes or null;
    // the copy writes exactly `units.len()` code units into it.
    unsafe {
        let block = CoTaskMemAlloc(bytes).cast::<u16>();
        if block.is_null() {
            return Err(E_OUTOFMEMORY);
        }
        ptr::copy_nonoverlapping(units.as_ptr(), block, units.len());
        *output = block;
    }
    Ok(())
}

/// # Safety
/// `text`, when non-null, points to writable pointer storage and `error`, when
/// non-null, to writable LONG storage for this synchronous COM call.
pub(super) unsafe extern "system" fn entry(
    this: *mut c_void,
    flags: i32,
    code: i32,
    text: *mut *mut u16,
    error: *mut i32,
) -> i32 {
    if !text.is_null() {
        // SAFETY: a non-null optional output is writable for this COM call.
        unsafe { *text = ptr::null_mut() };
    }
    if error.is_null() {
        return E_POINTER;
    }
    let result = super::super::catch_hresult(|| {
        if this.is_null() || flags != 0 {
            return E_INVALIDARG;
        }
        let Some(description) = describe(code) else {
            return E_INVALIDARG;
        };
        if text.is_null() {
            return S_OK;
        }
        // SAFETY: `text` was checked non-null and is the caller's output slot.
        match unsafe { allocate_ole_string(&description, text) } {
            Ok(()) => S_OK,
            Err(hr) => hr,
        }
    });
    // SAFETY: `error` was checked non-null above and remains writable.
    unsafe { super::report(error, result) }
}

#[link(name = "Kernel32")]
unsafe extern "system" {
    fn FormatMessageW(
        flags: u32,
        source: *const c_void,
        message_id: u32,
        language_id: u32,
        buffer: *mut u16,
        size: u32,
        arguments: *const c_void,
    ) -> u32;
}
#[link(name = "Ole32")]
unsafe extern "system" {
    fn CoTaskMemAlloc(bytes: usize) -> *mut c_void;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[link(name = "Ole32")]
    unsafe extern "system" {
        fn CoTaskMemFree(block: *mut c_void);
    }

    unsafe fn read_and_free(raw: *mut u16) -> String {
        // SAFETY: the driver allocated a NUL-terminated OLE string; ownership
        // transfers to this reader, which frees it exactly once.
        unsafe {
            let mut length = 0;
            while *raw.add(length) != 0 {
                length += 1;
            }
            let text = String::from_utf16(std::slice::from_raw_parts(raw, length)).unwrap();
            CoTaskMemFree(raw.cast());
            text
        }
    }

    #[test]
    fn every_reported_driver_hresult_has_end_user_text() {
        assert_eq!(describe(0).as_deref(), Some("No error."));
        for code in [
            WIA_ERROR_BUSY,
            WIA_ERROR_OFFLINE,
            WIA_ERROR_COVER_OPEN,
            E_INVALIDARG,
            E_POINTER,
            E_UNEXPECTED,
            E_FAIL,
            E_OUTOFMEMORY,
            E_ACCESSDENIED,
        ] {
            let text = describe(code).unwrap_or_else(|| panic!("{code:#x} has no text"));
            assert!(text.ends_with('.'), "{code:#x}: {text}");
        }
        assert!(describe(WIA_ERROR_BUSY).unwrap().contains("busy"));
    }

    #[test]
    fn win32_hresults_use_system_text_and_other_values_are_unrecognized() {
        // ERROR_GEN_FAILURE (31) is what a failed USB transfer surfaces as.
        let usb = describe(0x8007_001fu32 as i32).unwrap();
        assert!(!usb.trim().is_empty() && !usb.ends_with('\n'));
        // A Win32 code Windows has no text for still yields the numeric form.
        let unknown_win32 = describe(0x8007_fff0u32 as i32).unwrap();
        assert!(!unknown_win32.is_empty());
        assert_eq!(describe(0x7fff_1234), None);
        assert_eq!(describe(1), None, "S_FALSE is not a device error");
        assert_eq!(describe(0x8004_1234u32 as i32), None);
    }

    #[test]
    fn entry_allocates_caller_owned_text_and_rejects_unknown_codes() {
        let this = 8usize as *mut c_void; // never dereferenced by this method
        let mut text = ptr::dangling_mut();
        let mut error = 123;
        // SAFETY: outputs are live locals; `this` is only null-checked.
        unsafe {
            assert_eq!(
                entry(this, 0, WIA_ERROR_OFFLINE, &mut text, &mut error),
                S_OK
            );
            assert_eq!(error, 0);
            assert!(read_and_free(text).contains("not responding"));

            text = ptr::dangling_mut();
            assert_eq!(
                entry(this, 0, 0x7fff_1234, &mut text, &mut error),
                E_INVALIDARG
            );
            assert!(text.is_null());
            assert_eq!(error, E_INVALIDARG);

            assert_eq!(entry(this, 0, 0, ptr::null_mut(), &mut error), S_OK);
            assert_eq!(error, 0);
            assert_eq!(entry(this, 1, 0, &mut text, &mut error), E_INVALIDARG);
            assert_eq!(
                entry(ptr::null_mut(), 0, 0, &mut text, &mut error),
                E_INVALIDARG
            );
            text = ptr::dangling_mut();
            assert_eq!(entry(this, 0, 0, &mut text, ptr::null_mut()), E_POINTER);
            assert!(
                text.is_null(),
                "optional output is cleared even on E_POINTER"
            );
        }
    }
}
