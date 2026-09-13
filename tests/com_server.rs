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
