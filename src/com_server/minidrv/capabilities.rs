//! IWiaMiniDrv::drvGetCapabilities and drvDeviceCommand for the static flatbed tree.
//!
//! The service reads these tables to learn which commands it may issue through
//! drvDeviceCommand and which events it may deliver through drvNotifyPnpEvent.
//! Only the synchronize command and the two connection events that Windows
//! itself raises are declared; the driver signals no events of its own.

use super::{E_INVALIDARG, E_NOTIMPL, E_POINTER, Guid};
use std::{ffi::c_void, ptr};

const S_OK: i32 = 0;
// SDK 10.0.26100.0 wiadef.h.
const WIA_DEVICE_COMMANDS: i32 = 1;
const WIA_DEVICE_EVENTS: i32 = 2;
const WIA_NOTIFICATION_EVENT: u32 = 0x1;
pub(super) const WIA_CMD_SYNCHRONIZE: Guid = Guid {
    data1: 0x9b26_b7b2,
    data2: 0xacad,
    data3: 0x11d2,
    data4: [0xa0, 0x93, 0x00, 0xc0, 0x4f, 0x72, 0xdc, 0x3c],
};
pub(super) const WIA_EVENT_DEVICE_CONNECTED: Guid = Guid {
    data1: 0xa28b_bade,
    data2: 0x64b6,
    data3: 0x11d2,
    data4: [0xa2, 0x31, 0x00, 0xc0, 0x4f, 0xa3, 0x18, 0x09],
};
pub(super) const WIA_EVENT_DEVICE_DISCONNECTED: Guid = Guid {
    data1: 0x143e_4e83,
    data2: 0x6497,
    data3: 0x11d2,
    data4: [0xa2, 0x31, 0x00, 0xc0, 0x4f, 0xa3, 0x18, 0x09],
};

// SDK 10.0.26100.0 wiamindr_lh.h, WIA_DEV_CAP_DRV. The service borrows this
// table and never frees it, so every pointer targets process-lifetime statics.
#[repr(C)]
struct DevCap {
    guid: *const Guid,
    flags: u32,
    name: *const u16,
    description: *const u16,
    icon: *const u16,
}
// SAFETY: every pointer refers to immutable statics; the table is never
// mutated after construction and carries no thread-affine state.
unsafe impl Sync for DevCap {}

macro_rules! wide {
    ($text:literal) => {{
        const UNITS: &[u8] = $text.as_bytes();
        const LEN: usize = UNITS.len() + 1;
        const WIDE: [u16; LEN] = {
            let mut out = [0u16; LEN];
            let mut index = 0;
            while index < UNITS.len() {
                out[index] = UNITS[index] as u16;
                index += 1;
            }
            out
        };
        &WIDE
    }};
}

// Icons follow SDK wiadef.h WIA_ICON_SYNCHRONIZE / WIA_ICON_DEVICE_CONNECTED.
static SYNCHRONIZE: DevCap = DevCap {
    guid: &WIA_CMD_SYNCHRONIZE,
    flags: 0,
    name: wide!("Synchronize").as_ptr(),
    description: wide!("Synchronize the flatbed item tree with the scanner").as_ptr(),
    icon: wide!("sti.dll,-2000").as_ptr(),
};
static CONNECTED: DevCap = DevCap {
    guid: &WIA_EVENT_DEVICE_CONNECTED,
    flags: WIA_NOTIFICATION_EVENT,
    name: wide!("Device Connected").as_ptr(),
    description: wide!("The scanner was connected").as_ptr(),
    icon: wide!("sti.dll,-1001").as_ptr(),
};
static DISCONNECTED: DevCap = DevCap {
    guid: &WIA_EVENT_DEVICE_DISCONNECTED,
    flags: WIA_NOTIFICATION_EVENT,
    name: wide!("Device Disconnected").as_ptr(),
    description: wide!("The scanner was disconnected").as_ptr(),
    icon: wide!("sti.dll,-1001").as_ptr(),
};

// Commands precede events when both are requested, per the SDK contract.
static COMMANDS: [&DevCap; 1] = [&SYNCHRONIZE];
static EVENTS: [&DevCap; 2] = [&CONNECTED, &DISCONNECTED];
static BOTH: [&DevCap; 3] = [&SYNCHRONIZE, &CONNECTED, &DISCONNECTED];

/// The service expects a contiguous array of structures, not of pointers.
/// These copies are built once from the same statics and stay immutable.
struct Table<const N: usize>([DevCap; N]);
// SAFETY: see `DevCap`; the array is a compile-time copy of immutable statics.
unsafe impl<const N: usize> Sync for Table<N> {}

const fn copy(cap: &DevCap) -> DevCap {
    DevCap {
        guid: cap.guid,
        flags: cap.flags,
        name: cap.name,
        description: cap.description,
        icon: cap.icon,
    }
}
static COMMAND_TABLE: Table<1> = Table([copy(COMMANDS[0])]);
static EVENT_TABLE: Table<2> = Table([copy(EVENTS[0]), copy(EVENTS[1])]);
static BOTH_TABLE: Table<3> = Table([copy(BOTH[0]), copy(BOTH[1]), copy(BOTH[2])]);

fn select(flags: i32) -> Result<&'static [DevCap], i32> {
    match flags {
        WIA_DEVICE_COMMANDS => Ok(&COMMAND_TABLE.0),
        WIA_DEVICE_EVENTS => Ok(&EVENT_TABLE.0),
        f if f == WIA_DEVICE_COMMANDS | WIA_DEVICE_EVENTS => Ok(&BOTH_TABLE.0),
        _ => Err(E_INVALIDARG),
    }
}

pub(super) fn declares_event(event: &Guid) -> bool {
    EVENTS.iter().any(|cap| {
        // SAFETY: each table entry points to a process-lifetime static GUID.
        unsafe { *cap.guid == *event }
    })
}

/// # Safety
/// Non-null outputs point to writable storage of the SDK-declared types for
/// this synchronous COM call. The service may pass a null item context when it
/// enumerates capabilities before the item tree exists.
pub(super) unsafe extern "system" fn entry(
    this: *mut c_void,
    _context: *mut u8,
    flags: i32,
    count: *mut i32,
    list: *mut *mut c_void,
    error: *mut i32,
) -> i32 {
    if !count.is_null() {
        // SAFETY: a non-null output is writable for this COM call.
        unsafe { *count = 0 };
    }
    if !list.is_null() {
        // SAFETY: a non-null output is writable for this COM call.
        unsafe { *list = ptr::null_mut() };
    }
    if error.is_null() {
        return E_POINTER;
    }
    let result = super::super::catch_hresult(|| {
        if this.is_null() || count.is_null() {
            return E_INVALIDARG;
        }
        let table = match select(flags) {
            Ok(table) => table,
            Err(hr) => return hr,
        };
        // SAFETY: `count` was checked non-null; `list` is optional per the SDK.
        unsafe {
            *count = table.len() as i32;
            if !list.is_null() {
                *list = table.as_ptr().cast_mut().cast();
            }
        }
        S_OK
    });
    // SAFETY: `error` was checked non-null above and remains writable.
    unsafe { super::report(error, result) }
}

/// # Safety
/// `command`, when non-null, points to a live GUID; `item` and `error`, when
/// non-null, point to writable storage for this synchronous COM call.
pub(super) unsafe extern "system" fn command(
    this: *mut c_void,
    context: *mut u8,
    flags: i32,
    command: *const Guid,
    item: *mut *mut c_void,
    error: *mut i32,
) -> i32 {
    if !item.is_null() {
        // SAFETY: a non-null optional output is writable for this COM call.
        unsafe { *item = ptr::null_mut() };
    }
    if error.is_null() {
        return E_POINTER;
    }
    let result = super::super::catch_hresult(|| {
        if command.is_null() {
            return E_POINTER;
        }
        if this.is_null() || context.is_null() || flags != 0 {
            return E_INVALIDARG;
        }
        // SAFETY: the caller supplies a live GUID for this synchronous call.
        if unsafe { *command } == WIA_CMD_SYNCHRONIZE {
            // The flatbed tree is fixed at initialization and never diverges
            // from the device, so there is nothing to rebuild or signal.
            S_OK
        } else {
            E_NOTIMPL
        }
    });
    // SAFETY: `error` was checked non-null above and remains writable.
    unsafe { super::report(error, result) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::mem::{offset_of, size_of};

    unsafe fn read_wide(raw: *const u16) -> String {
        // SAFETY: all table strings are NUL-terminated statics.
        unsafe {
            let mut length = 0;
            while *raw.add(length) != 0 {
                length += 1;
            }
            String::from_utf16(std::slice::from_raw_parts(raw, length)).unwrap()
        }
    }

    #[test]
    fn dev_cap_matches_sdk_layout() {
        assert_eq!(size_of::<DevCap>(), 40);
        assert_eq!(offset_of!(DevCap, flags), 8);
        assert_eq!(offset_of!(DevCap, name), 16);
        assert_eq!(offset_of!(DevCap, description), 24);
        assert_eq!(offset_of!(DevCap, icon), 32);
    }

    #[test]
    fn tables_list_commands_before_events_with_terminated_names() {
        let both = select(3).unwrap();
        assert_eq!(both.len(), 3);
        // SAFETY: table entries reference process-lifetime statics.
        unsafe {
            assert_eq!(*both[0].guid, WIA_CMD_SYNCHRONIZE);
            assert_eq!(both[0].flags, 0);
            assert_eq!(read_wide(both[0].name), "Synchronize");
            assert_eq!(read_wide(both[0].icon), "sti.dll,-2000");
            assert_eq!(*both[1].guid, WIA_EVENT_DEVICE_CONNECTED);
            assert_eq!(both[1].flags, WIA_NOTIFICATION_EVENT);
            assert_eq!(read_wide(both[1].name), "Device Connected");
            assert_eq!(*both[2].guid, WIA_EVENT_DEVICE_DISCONNECTED);
            assert_eq!(
                read_wide(both[2].description),
                "The scanner was disconnected"
            );
        }
        assert_eq!(select(1).unwrap().len(), 1);
        assert_eq!(select(2).unwrap().len(), 2);
        for flags in [0, 4, -1] {
            assert_eq!(select(flags).map(|t| t.len()), Err(E_INVALIDARG));
        }
        assert!(declares_event(&WIA_EVENT_DEVICE_CONNECTED));
        assert!(declares_event(&WIA_EVENT_DEVICE_DISCONNECTED));
        assert!(!declares_event(&WIA_CMD_SYNCHRONIZE));
    }

    #[test]
    fn entry_allows_null_context_and_optional_list_but_requires_count() {
        let this = 8usize as *mut c_void; // never dereferenced by this method
        let mut count = 99;
        let mut list = ptr::dangling_mut();
        let mut error = 123;
        // SAFETY: outputs are live locals; `this` is only null-checked.
        unsafe {
            assert_eq!(
                entry(this, ptr::null_mut(), 3, &mut count, &mut list, &mut error),
                S_OK
            );
            assert_eq!((count, error), (3, 0));
            assert_eq!(list.cast::<DevCap>().cast_const(), BOTH_TABLE.0.as_ptr());
            assert_eq!(
                entry(
                    this,
                    ptr::null_mut(),
                    2,
                    &mut count,
                    ptr::null_mut(),
                    &mut error
                ),
                S_OK
            );
            assert_eq!(count, 2);
            assert_eq!(
                entry(
                    this,
                    ptr::null_mut(),
                    1,
                    ptr::null_mut(),
                    &mut list,
                    &mut error
                ),
                E_INVALIDARG
            );
            assert!(list.is_null());
            assert_eq!(
                entry(this, ptr::null_mut(), 7, &mut count, &mut list, &mut error),
                E_INVALIDARG
            );
            assert_eq!((count, error), (0, E_INVALIDARG));
            assert_eq!(
                entry(
                    ptr::null_mut(),
                    ptr::null_mut(),
                    3,
                    &mut count,
                    &mut list,
                    &mut error
                ),
                E_INVALIDARG
            );
            assert_eq!(
                entry(
                    this,
                    ptr::null_mut(),
                    3,
                    &mut count,
                    &mut list,
                    ptr::null_mut()
                ),
                E_POINTER
            );
        }
    }

    #[test]
    fn synchronize_is_a_no_op_and_other_commands_stay_unsupported() {
        let this = 8usize as *mut c_void;
        let context = 8usize as *mut u8;
        let mut item = ptr::dangling_mut();
        let mut error = 123;
        // SAFETY: outputs are live locals; receiver and context are only
        // null-checked and the GUIDs are live constants.
        unsafe {
            assert_eq!(
                command(
                    this,
                    context,
                    0,
                    &WIA_CMD_SYNCHRONIZE,
                    &mut item,
                    &mut error
                ),
                S_OK
            );
            assert!(item.is_null());
            assert_eq!(error, 0);
            assert_eq!(
                command(
                    this,
                    context,
                    0,
                    &WIA_EVENT_DEVICE_CONNECTED,
                    &mut item,
                    &mut error
                ),
                E_NOTIMPL
            );
            assert_eq!(error, E_NOTIMPL);
            assert_eq!(
                command(
                    this,
                    context,
                    1,
                    &WIA_CMD_SYNCHRONIZE,
                    &mut item,
                    &mut error
                ),
                E_INVALIDARG
            );
            assert_eq!(
                command(
                    this,
                    ptr::null_mut(),
                    0,
                    &WIA_CMD_SYNCHRONIZE,
                    &mut item,
                    &mut error
                ),
                E_INVALIDARG
            );
            assert_eq!(
                command(this, context, 0, ptr::null(), &mut item, &mut error),
                E_POINTER
            );
            assert_eq!(
                command(
                    this,
                    context,
                    0,
                    &WIA_CMD_SYNCHRONIZE,
                    ptr::null_mut(),
                    &mut error
                ),
                S_OK,
                "the item output is optional"
            );
            assert_eq!(
                command(
                    this,
                    context,
                    0,
                    &WIA_CMD_SYNCHRONIZE,
                    &mut item,
                    ptr::null_mut()
                ),
                E_POINTER
            );
        }
    }
}
