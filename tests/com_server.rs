//! COM loader contracts. No registration, USB access, or hardware initialization.
#![cfg(windows)]

use std::{ffi::c_void, ptr, sync::Mutex};
use workcentre_3119::com_server::{DRIVER_CLASS_ID, DllCanUnloadNow, DllGetClassObject, Guid};

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
const UNSUPPORTED: Guid = Guid {
    data1: 0x12345678,
    ..UNKNOWN
};
const MINIDRIVER: Guid = Guid {
    data1: 0xd8cd_ee14,
    data2: 0x3c6c,
    data3: 0x11d2,
    data4: [0x9a, 0x35, 0, 0xc0, 0x4f, 0xa3, 0x61, 0x45],
};
const STI: Guid = Guid {
    data1: 0x0c9b_b460,
    data2: 0x51ac,
    data3: 0x11d0,
    data4: [0x90, 0xea, 0, 0xaa, 0, 0x60, 0xf8, 0x6c],
};
const E_POINTER: i32 = 0x80004003u32 as i32;
const E_NOINTERFACE: i32 = 0x80004002u32 as i32;
const CLASS_E_CLASSNOTAVAILABLE: i32 = 0x80040111u32 as i32;
const CLASS_E_NOAGGREGATION: i32 = 0x80040110u32 as i32;
static SERIAL: Mutex<()> = Mutex::new(());

// Independent declarations follow SDK unknwnbase.h rather than implementation structs.
#[repr(C)]
struct UnknownTable {
    query: unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
}
#[repr(C)]
struct FactoryTable {
    unknown: UnknownTable,
    create:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, *mut *mut c_void) -> i32,
    lock: unsafe extern "system" fn(*mut c_void, i32) -> i32,
}

#[repr(C)]
struct MiniPrefix {
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
}

fn factory() -> *mut c_void {
    let mut result = ptr::null_mut();
    assert_eq!(
        // SAFETY: GUIDs and output live across this synchronous call.
        unsafe { DllGetClassObject(&DRIVER_CLASS_ID, &FACTORY, &mut result) },
        0
    );
    assert!(!result.is_null());
    result
}

unsafe fn table<'a>(object: *mut c_void) -> &'a UnknownTable {
    // SAFETY: tests hold a reference to the COM object with this SDK vtable prefix.
    unsafe { &**object.cast::<*const UnknownTable>() }
}
unsafe fn factory_table<'a>(object: *mut c_void) -> &'a FactoryTable {
    // SAFETY: only used for a live pointer obtained for IID_IClassFactory.
    unsafe { &**object.cast::<*const FactoryTable>() }
}

#[test]
fn class_lookup_rejects_invalid_requests_without_retaining_objects() {
    let _serial = SERIAL.lock().unwrap();
    assert_eq!(DllCanUnloadNow(), 0);
    let mut output = ptr::dangling_mut::<c_void>();
    // SAFETY: all non-null inputs are live GUIDs/output; nulls test the entry contract.
    unsafe {
        assert_eq!(
            DllGetClassObject(&UNSUPPORTED, &FACTORY, &mut output),
            CLASS_E_CLASSNOTAVAILABLE
        );
        assert!(output.is_null());
        output = ptr::dangling_mut();
        assert_eq!(
            DllGetClassObject(&DRIVER_CLASS_ID, &UNSUPPORTED, &mut output),
            E_NOINTERFACE
        );
        assert!(output.is_null());
        assert_eq!(
            DllGetClassObject(ptr::null(), &FACTORY, &mut output),
            E_POINTER
        );
        assert_eq!(
            DllGetClassObject(&DRIVER_CLASS_ID, ptr::null(), &mut output),
            E_POINTER
        );
        assert_eq!(
            DllGetClassObject(&DRIVER_CLASS_ID, &FACTORY, ptr::null_mut()),
            E_POINTER
        );
    }
    assert_eq!(DllCanUnloadNow(), 0);
}

#[test]
fn factory_and_created_identity_hold_the_module_until_final_release() {
    let _serial = SERIAL.lock().unwrap();
    let class = factory();
    assert_eq!(DllCanUnloadNow(), 1);
    // SAFETY: every call uses a live owned reference and valid SDK ABI arguments.
    unsafe {
        let methods = factory_table(class);
        let mut alias = ptr::null_mut();
        assert_eq!((methods.unknown.query)(class, &UNKNOWN, &mut alias), 0);
        assert_eq!(alias, class);
        assert_eq!((table(alias).release)(alias), 1);
        let mut instance = ptr::null_mut();
        assert_eq!(
            (methods.create)(class, ptr::null_mut(), &UNKNOWN, &mut instance),
            0
        );
        assert!(!instance.is_null());
        let mut identity = ptr::null_mut();
        assert_eq!(
            (table(instance).query)(instance, &UNKNOWN, &mut identity),
            0
        );
        assert_eq!(identity, instance);
        assert_eq!((table(identity).release)(identity), 1);
        let mut rejected = ptr::dangling_mut();
        assert_eq!(
            (table(instance).query)(instance, &FACTORY, &mut rejected),
            E_NOINTERFACE
        );
        assert!(rejected.is_null());
        assert_eq!((methods.unknown.release)(class), 0);
        assert_eq!(DllCanUnloadNow(), 1);
        assert_eq!((table(instance).release)(instance), 0);
    }
    assert_eq!(DllCanUnloadNow(), 0);
}

#[test]
fn server_locks_and_failed_creation_keep_counts_balanced() {
    let _serial = SERIAL.lock().unwrap();
    let class = factory();
    // SAFETY: live class factory and output storage. Non-null outer is never dereferenced.
    unsafe {
        let methods = factory_table(class);
        let mut output = ptr::dangling_mut();
        assert_eq!(
            (methods.create)(class, ptr::null_mut(), &UNSUPPORTED, &mut output),
            E_NOINTERFACE
        );
        assert!(output.is_null());
        output = ptr::dangling_mut();
        assert_eq!(
            (methods.create)(class, ptr::dangling_mut(), &UNKNOWN, &mut output),
            CLASS_E_NOAGGREGATION
        );
        assert!(output.is_null());
        assert_eq!(
            (methods.create)(class, ptr::null_mut(), ptr::null(), &mut output),
            E_POINTER
        );
        assert_eq!(
            (methods.create)(class, ptr::null_mut(), &UNKNOWN, ptr::null_mut()),
            E_POINTER
        );
        assert!(
            (methods.lock)(class, 0) < 0,
            "unmatched unlock must not underflow"
        );
        assert_eq!((methods.lock)(class, 1), 0);
        assert_eq!((methods.unknown.release)(class), 0);
        assert_eq!(
            DllCanUnloadNow(),
            1,
            "server lock keeps DLL loaded without factory references"
        );
        let next_class = factory();
        assert_eq!((factory_table(next_class).lock)(next_class, 0), 0);
        assert_eq!((table(next_class).release)(next_class), 0);
    }
    assert_eq!(DllCanUnloadNow(), 0);
}

#[test]
fn reference_count_survives_concurrent_balanced_calls() {
    let _serial = SERIAL.lock().unwrap();
    let class = factory();
    let address = class as usize;
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(move || {
                let class = address as *mut c_void;
                for _ in 0..10000 {
                    // SAFETY: original reference remains alive until all threads join;
                    // each temporary reference is released once, only atomic IUnknown calls occur.
                    unsafe {
                        (table(class).add_ref)(class);
                        (table(class).release)(class);
                    }
                }
            });
        }
    });
    assert_eq!(DllCanUnloadNow(), 1);
    // SAFETY: all worker threads finished; release the sole remaining original reference.
    assert_eq!(unsafe { (table(class).release)(class) }, 0);
    assert_eq!(DllCanUnloadNow(), 0);
}

#[test]
fn minidriver_and_sti_share_one_identity_and_lifetime() {
    let _serial = SERIAL.lock().unwrap();
    let class = factory();
    // SAFETY: independent SDK interface identifiers and live owned COM references.
    unsafe {
        let mut mini = ptr::null_mut();
        let hr = (factory_table(class).create)(class, ptr::null_mut(), &MINIDRIVER, &mut mini);
        (table(class).release)(class);
        assert_eq!(hr, 0);
        let mut sti = ptr::null_mut();
        let mut identity = ptr::null_mut();
        let mut identity_from_sti = ptr::null_mut();
        let mut mini_from_sti = ptr::null_mut();
        assert_eq!((table(mini).query)(mini, &STI, &mut sti), 0);
        assert_eq!((table(mini).query)(mini, &UNKNOWN, &mut identity), 0);
        assert_eq!((table(sti).query)(sti, &UNKNOWN, &mut identity_from_sti), 0);
        assert_eq!(identity, identity_from_sti);
        assert_eq!((table(sti).query)(sti, &MINIDRIVER, &mut mini_from_sti), 0);
        assert_eq!(mini, mini_from_sti);
        assert_ne!(mini, sti, "the interfaces have different method tables");
        assert_eq!((table(mini_from_sti).release)(mini_from_sti), 4);
        assert_eq!((table(identity_from_sti).release)(identity_from_sti), 3);
        assert_eq!((table(identity).release)(identity), 2);
        assert_eq!((table(sti).release)(sti), 1);
        assert_eq!(DllCanUnloadNow(), 1);
        assert_eq!((table(mini).release)(mini), 0);
    }
    assert_eq!(DllCanUnloadNow(), 0);
}

#[test]
fn minidriver_rejects_absent_service_context_and_clears_outputs() {
    let _serial = SERIAL.lock().unwrap();
    let class = factory();
    // SAFETY: held interface and writable outputs. No fake WIA service context
    // is supplied; a null context must reject before any BSTR or helper access.
    unsafe {
        let mut mini = ptr::null_mut();
        assert_eq!(
            (factory_table(class).create)(class, ptr::null_mut(), &MINIDRIVER, &mut mini),
            0
        );
        (table(class).release)(class);
        let methods = &**mini.cast::<*const MiniPrefix>();
        let mut root = ptr::dangling_mut();
        let mut inner = ptr::dangling_mut();
        let mut error = 123;
        let invalid = 0x80070057u32 as i32;
        assert_eq!(
            (methods.initialize)(
                mini,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut root,
                &mut inner,
                &mut error
            ),
            invalid
        );
        assert!(root.is_null() && inner.is_null());
        assert_eq!(error, invalid);
        assert_eq!(
            (methods.initialize)(
                mini,
                ptr::null_mut(),
                0,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
                &mut root,
                &mut inner,
                ptr::null_mut()
            ),
            E_POINTER
        );
        assert_eq!((table(mini).release)(mini), 0);
    }
    assert_eq!(DllCanUnloadNow(), 0);
}

#[test]
fn transferring_a_server_lock_to_factory_references_never_allows_unload() {
    let _serial = SERIAL.lock().unwrap();
    let class = factory();
    // SAFETY: live factory, establish a lock before dropping its only reference.
    unsafe {
        assert_eq!((factory_table(class).lock)(class, 1), 0);
        (table(class).release)(class);
    }
    let mut incorrectly_unloadable = false;
    std::thread::scope(|scope| {
        let worker = scope.spawn(|| {
            for _ in 0..50000 {
                let class = factory();
                // SAFETY: the factory remains alive while the previous lock is
                // released, and a replacement lock is acquired before Release.
                unsafe {
                    assert_eq!((factory_table(class).lock)(class, 0), 0);
                    assert_eq!((factory_table(class).lock)(class, 1), 0);
                    (table(class).release)(class);
                }
            }
        });
        while !worker.is_finished() {
            incorrectly_unloadable |= DllCanUnloadNow() == 0;
        }
        worker.join().unwrap();
    });
    let class = factory();
    // SAFETY: worker is finished, release its retained lock and our final factory reference.
    unsafe {
        assert_eq!((factory_table(class).lock)(class, 0), 0);
        (table(class).release)(class);
    }
    assert!(
        !incorrectly_unloadable,
        "a lock or a live factory existed throughout the transfer"
    );
    assert_eq!(DllCanUnloadNow(), 0);
}
