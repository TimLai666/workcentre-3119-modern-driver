//! Explicit release DLL smoke test, without COM registration or USB access.
#![cfg(windows)]

use std::{
    ffi::{CString, c_char, c_void},
    os::windows::ffi::OsStrExt,
    path::PathBuf,
    ptr,
};

#[repr(C)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}
// Deliberately independent of Rust exports: this test checks the actual DLL ABI.
const CLASS: Guid = Guid {
    data1: 0xf71a8435,
    data2: 0xaa10,
    data3: 0x40a6,
    data4: [0x83, 0x34, 0x49, 0xee, 0xc8, 0xfe, 0x9c, 0x63],
};
const UNKNOWN: Guid = Guid {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const FACTORY: Guid = Guid {
    data1: 1,
    ..UNKNOWN
};
const STI: Guid = Guid {
    data1: 0x0c9bb460,
    data2: 0x51ac,
    data3: 0x11d0,
    data4: [0x90, 0xea, 0, 0xaa, 0, 0x60, 0xf8, 0x6c],
};
const MINI: Guid = Guid {
    data1: 0xd8cdee14,
    data2: 0x3c6c,
    data3: 0x11d2,
    data4: [0x9a, 0x35, 0, 0xc0, 0x4f, 0xa3, 0x61, 0x45],
};
const E_NOTIMPL: i32 = 0x80004001u32 as i32;
const WIA_EVENT_CANCEL_IO: Guid = Guid {
    data1: 0xc860_f7b8,
    data2: 0x9ccd,
    data3: 0x41ea,
    data4: [0xbb, 0xbf, 0x4d, 0xd0, 0x9c, 0x5b, 0x17, 0x95],
};
#[repr(C)]
struct UnknownTable {
    query: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    add: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}
#[repr(C)]
struct FactoryTable {
    unknown: UnknownTable,
    create:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, *mut *mut c_void) -> i32,
    lock: unsafe extern "system" fn(*mut c_void, i32) -> i32,
}
type ItemMethod = unsafe extern "system" fn(*mut c_void, *mut u8, i32, *mut i32) -> i32;
type PropertiesMethod =
    unsafe extern "system" fn(*mut c_void, *mut u8, i32, u32, *const c_void, *mut i32) -> i32;
type TransferMethod =
    unsafe extern "system" fn(*mut c_void, *mut u8, i32, *mut c_void, *mut i32) -> i32;
type ListMethod = unsafe extern "system" fn(
    *mut c_void,
    *mut u8,
    i32,
    *mut i32,
    *mut *mut c_void,
    *mut i32,
) -> i32;
// SDK 10.0.26100.0 wiamindr_lh.h: drvNotifyPnpEvent is IWiaMiniDrvVtbl
// slot 18, followed by drvUnInitialize at slot 19.
#[repr(C)]
struct MiniTable {
    unknown: UnknownTable,
    initialize: unsafe extern "system" fn(
        *mut c_void,
        *mut u8,
        i32,
        *mut u16,
        *mut u16,
        *mut c_void,
        *mut c_void,
        *mut *mut c_void,
        *mut *mut c_void,
        *mut i32,
    ) -> i32,
    acquire: TransferMethod,
    init_properties: ItemMethod,
    validate_properties: PropertiesMethod,
    write_properties: TransferMethod,
    read_properties: PropertiesMethod,
    lock_item: ItemMethod,
    unlock_item: ItemMethod,
    analyze: ItemMethod,
    error_string: unsafe extern "system" fn(*mut c_void, i32, i32, *mut *mut u16, *mut i32) -> i32,
    command: unsafe extern "system" fn(
        *mut c_void,
        *mut u8,
        i32,
        *const Guid,
        *mut *mut c_void,
        *mut i32,
    ) -> i32,
    capabilities: ListMethod,
    delete_item: ItemMethod,
    free_context: unsafe extern "system" fn(*mut c_void, i32, *mut u8, *mut i32) -> i32,
    formats: ListMethod,
    notify: unsafe extern "system" fn(*mut c_void, *const Guid, *mut u16, u32) -> i32,
    uninitialize: unsafe extern "system" fn(*mut c_void, *mut u8) -> i32,
}
#[link(name = "Kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(path: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
}
#[link(name = "OleAut32")]
unsafe extern "system" {
    fn SysAllocStringLen(value: *const u16, length: u32) -> *mut u16;
    fn SysFreeString(value: *mut u16);
}
#[link(name = "Ole32")]
unsafe extern "system" {
    fn CoTaskMemFree(block: *mut c_void);
}
struct Module(*mut c_void);
impl Drop for Module {
    fn drop(&mut self) {
        // SAFETY: one successful LoadLibraryExW reference, after all borrowed COM objects drop.
        unsafe {
            FreeLibrary(self.0);
        }
    }
}
impl Module {
    fn unload(self) {
        let module = std::mem::ManuallyDrop::new(self);
        // SAFETY: takes the sole loader reference after all borrowing COM guards are dropped.
        let result = unsafe { FreeLibrary(module.0) };
        assert_ne!(
            result,
            0,
            "FreeLibrary failed: {}",
            std::io::Error::last_os_error()
        );
    }

    fn symbol(&self, name: &str) -> *mut c_void {
        let name = CString::new(name).unwrap();
        // SAFETY: live module and terminated ASCII symbol name.
        let address = unsafe { GetProcAddress(self.0, name.as_ptr()) };
        assert!(!address.is_null(), "missing DLL export {name:?}");
        address
    }
}
struct Owned<'a> {
    raw: *mut c_void,
    _module: &'a Module,
}
impl Drop for Owned<'_> {
    fn drop(&mut self) {
        // SAFETY: holds one COM reference with IUnknown prefix; module remains loaded.
        unsafe {
            ((**self.raw.cast::<*const UnknownTable>()).release)(self.raw);
        }
    }
}
struct Bstr(*mut u16);
impl Drop for Bstr {
    fn drop(&mut self) {
        // SAFETY: this is either null or the sole BSTR returned by OleAut32.
        unsafe {
            SysFreeString(self.0);
        }
    }
}

#[test]
#[ignore = "Build release cdylib and set WC3119_TEST_DLL to its absolute path; see ENG.md"]
fn release_dll_exports_real_factory_and_keeps_objects_alive() {
    let path = PathBuf::from(std::env::var_os("WC3119_TEST_DLL").expect("set WC3119_TEST_DLL"));
    assert!(path.is_absolute() && path.is_file());
    let mut wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    assert!(!wide.contains(&0));
    wide.push(0);
    // SAFETY: absolute terminated path; null reserved handle. Search only DLL dir/default dirs.
    let handle = unsafe { LoadLibraryExW(wide.as_ptr(), ptr::null_mut(), 0x100 | 0x1000) };
    assert!(
        !handle.is_null(),
        "LoadLibraryExW failed: {}",
        std::io::Error::last_os_error()
    );
    let module = Module(handle);
    type GetClass = unsafe extern "system" fn(*const Guid, *const Guid, *mut *mut c_void) -> i32;
    type CanUnload = unsafe extern "system" fn() -> i32;
    // SAFETY: named DLL exports are specified by COM with these ABI signatures.
    let get_class: GetClass = unsafe { std::mem::transmute(module.symbol("DllGetClassObject")) };
    // SAFETY: same export contract for DllCanUnloadNow, no parameters.
    let can_unload: CanUnload = unsafe { std::mem::transmute(module.symbol("DllCanUnloadNow")) };
    // SAFETY: module stays loaded and every COM pointer below is owned until its guard drops.
    unsafe {
        assert_eq!(can_unload(), 0);
        let mut raw = ptr::null_mut();
        assert_eq!(get_class(&CLASS, &FACTORY, &mut raw), 0);
        assert!(!raw.is_null());
        let factory = Owned {
            raw,
            _module: &module,
        };
        assert_eq!(can_unload(), 1);
        let methods = &**factory.raw.cast::<*const FactoryTable>();
        let mut object = ptr::null_mut();
        assert_eq!(
            (methods.create)(factory.raw, ptr::null_mut(), &UNKNOWN, &mut object),
            0
        );
        assert!(!object.is_null());
        let object = Owned {
            raw: object,
            _module: &module,
        };
        drop(factory);
        assert_eq!(can_unload(), 1);
        let mut alias = ptr::null_mut();
        assert_eq!(
            ((**object.raw.cast::<*const UnknownTable>()).query)(object.raw, &UNKNOWN, &mut alias),
            0
        );
        assert_eq!(alias, object.raw);
        let alias = Owned {
            raw: alias,
            _module: &module,
        };
        let mut sti = ptr::null_mut();
        assert_eq!(
            ((**object.raw.cast::<*const UnknownTable>()).query)(object.raw, &STI, &mut sti),
            0
        );
        // IStiUSD has its own method table; only IID_IUnknown must be identical.
        assert_ne!(sti, object.raw);
        let mut sti_identity = ptr::null_mut();
        assert_eq!(
            ((**sti.cast::<*const UnknownTable>()).query)(sti, &UNKNOWN, &mut sti_identity),
            0
        );
        assert_eq!(sti_identity, object.raw);
        drop(Owned {
            raw: sti_identity,
            _module: &module,
        });
        drop(Owned {
            raw: sti,
            _module: &module,
        });
        let mut mini = ptr::null_mut();
        assert_eq!(
            ((**object.raw.cast::<*const UnknownTable>()).query)(object.raw, &MINI, &mut mini),
            0
        );
        assert_ne!(mini, object.raw);
        let mini = Owned {
            raw: mini,
            _module: &module,
        };
        let methods = &**mini.raw.cast::<*const MiniTable>();
        assert_eq!(std::mem::size_of::<MiniTable>(), 160);
        assert_eq!(std::mem::offset_of!(MiniTable, notify), 144);
        assert_eq!(std::mem::offset_of!(MiniTable, init_properties), 40);
        assert_eq!(std::mem::offset_of!(MiniTable, validate_properties), 48);
        assert_eq!(std::mem::offset_of!(MiniTable, read_properties), 64);
        assert_eq!(std::mem::offset_of!(MiniTable, error_string), 96);
        assert_eq!(std::mem::offset_of!(MiniTable, capabilities), 112);
        let mut property_error = 123;
        // A real COM object with absent service context must fail before USB
        // or WIA property helpers. Never fabricate a service-owned context.
        assert_eq!(
            (methods.init_properties)(mini.raw, ptr::null_mut(), 0, &mut property_error),
            0x80070057u32 as i32
        );
        assert_eq!(property_error, 0x80070057u32 as i32);
        assert_eq!(
            (methods.init_properties)(mini.raw, ptr::null_mut(), 0, ptr::null_mut()),
            0x80004003u32 as i32
        );
        property_error = 123;
        assert_eq!(
            (methods.validate_properties)(
                mini.raw,
                ptr::null_mut(),
                0,
                0,
                ptr::null(),
                &mut property_error
            ),
            0x80070057u32 as i32
        );
        assert_eq!(property_error, 0x80070057u32 as i32);
        assert_eq!(
            (methods.validate_properties)(
                mini.raw,
                ptr::null_mut(),
                0,
                0,
                ptr::null(),
                ptr::null_mut()
            ),
            0x80004003u32 as i32
        );
        property_error = 123;
        assert_eq!(
            (methods.read_properties)(
                mini.raw,
                ptr::null_mut(),
                0,
                0,
                ptr::null(),
                &mut property_error
            ),
            0x80070057u32 as i32
        );
        assert_eq!(property_error, 0x80070057u32 as i32);
        let mut text = ptr::dangling_mut();
        assert_eq!(
            (methods.error_string)(
                mini.raw,
                0,
                0x80210005u32 as i32,
                &mut text,
                &mut property_error
            ),
            0
        );
        assert_eq!(property_error, 0);
        assert!(!text.is_null());
        CoTaskMemFree(text.cast());
        let mut count = 99;
        let mut list = ptr::dangling_mut();
        assert_eq!(
            (methods.capabilities)(
                mini.raw,
                ptr::null_mut(),
                2,
                &mut count,
                &mut list,
                &mut property_error
            ),
            0
        );
        assert_eq!((count, property_error), (2, 0));
        assert!(!list.is_null());
        let device_name: Vec<u16> = "synthetic-device".encode_utf16().collect();
        let device = Bstr(SysAllocStringLen(
            device_name.as_ptr(),
            device_name.len() as u32,
        ));
        assert!(!device.0.is_null());
        assert_eq!(
            (methods.notify)(mini.raw, &WIA_EVENT_CANCEL_IO, device.0, 0),
            0
        );
        assert_eq!((methods.notify)(mini.raw, &CLASS, device.0, 0), E_NOTIMPL);
        let mut common_identity = ptr::null_mut();
        assert_eq!(
            ((**mini.raw.cast::<*const UnknownTable>()).query)(
                mini.raw,
                &UNKNOWN,
                &mut common_identity
            ),
            0
        );
        assert_eq!(common_identity, object.raw);
        drop(Owned {
            raw: common_identity,
            _module: &module,
        });
        drop(object);
        assert_eq!(can_unload(), 1);
        drop(alias);
        assert_eq!(can_unload(), 1);
        drop(mini);
        assert_eq!(can_unload(), 0);
    }
    module.unload();
}
