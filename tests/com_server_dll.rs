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
#[link(name = "Kernel32")]
unsafe extern "system" {
    fn LoadLibraryExW(path: *const u16, file: *mut c_void, flags: u32) -> *mut c_void;
    fn GetProcAddress(module: *mut c_void, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(module: *mut c_void) -> i32;
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
        drop(object);
        assert_eq!(can_unload(), 1);
        drop(alias);
        assert_eq!(can_unload(), 0);
    }
    module.unload();
}
