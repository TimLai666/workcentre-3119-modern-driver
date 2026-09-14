//! Windows COM server identity and lifetime for the scanner integration.
//!
//! The instance supports `IUnknown` and `IStiUSD`; WIA image transfer through
//! `IWiaMiniDrv` and system registration are still under development.

mod session;
mod sti;

use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::atomic::{AtomicU32, Ordering},
};

const S_OK: i32 = 0;
const S_FALSE: i32 = 1;
const E_NOINTERFACE: i32 = 0x8000_4002u32 as i32;
const E_POINTER: i32 = 0x8000_4003u32 as i32;
const E_UNEXPECTED: i32 = 0x8000_ffffu32 as i32;
const CLASS_E_NOAGGREGATION: i32 = 0x8004_0110u32 as i32;

/// Stream a BMP through an existing, locked driver object without reopening USB.
///
/// This Rust integration entry point is used while the WIA COM transfer adapter
/// is under development. It does not register a device or implement IWiaMiniDrv.
/// Any error invalidates the destination image. The caller must keep its owned
/// COM reference alive throughout the call and all synchronous output callbacks.
///
/// # Safety
/// `device` must be a live IStiUSD or IUnknown pointer returned by this module's
/// class factory, not an unrelated COM object's pointer. Null is rejected.
pub unsafe fn scan_locked_bmp<W: std::io::Write + std::io::Seek>(
    device: *mut c_void,
    settings: crate::wia::FlatbedSettings,
    cancel: &std::sync::atomic::AtomicBool,
    output: &mut W,
) -> std::io::Result<crate::scan::ScanSummary> {
    if device.is_null() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Driver object is null",
        ));
    }
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: caller owns a reference from our factory for this entire call.
        unsafe { &(*device.cast::<Instance>()).state }.scan_bmp(settings, cancel, output)
    }))
    .unwrap_or_else(|_| {
        Err(std::io::Error::other(
            "Locked scan panicked; destination must be discarded",
        ))
    })
}
const CLASS_E_CLASSNOTAVAILABLE: i32 = 0x8004_0111u32 as i32;

/// Transfer one flatbed BMP into a native WIA transfer callback's IStream.
///
/// Uses the already initialized and locked object's USB session. Callback
/// QueryInterface, GetNextStream, progress and Release all run inside its
/// exclusive operation lease, outside the state mutex. No WIA registration or
/// IWiaMiniDrv property/context implementation is provided by this Rust entry.
/// Discard the destination unless the result is `TransferOutcome::Completed`.
///
/// # Safety
/// `device` must be a live IUnknown/IStiUSD returned by this module's factory,
/// with an owned reference held throughout the call and synchronous callbacks.
/// `callback`, if non-null, must be a live IUnknown-compatible COM interface
/// callable in the current apartment until this function returns. The caller
/// must keep that apartment initialized. Null pointers are rejected safely.
pub unsafe fn transfer_locked_bmp(
    device: *mut c_void,
    settings: crate::wia::FlatbedSettings,
    cancel: &std::sync::atomic::AtomicBool,
    callback: *mut c_void,
    item: &str,
    full_item: &str,
) -> std::io::Result<crate::wia_transfer::TransferOutcome> {
    if device.is_null() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Driver object is null",
        ));
    }
    catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: both COM references and the apartment are kept alive by caller.
        unsafe {
            (*device.cast::<Instance>())
                .state
                .transfer_bmp(settings, cancel, callback, item, full_item)
        }
    }))
    .unwrap_or_else(|_| {
        Err(std::io::Error::other(
            "WIA transfer panicked; discard destination",
        ))
    })
}

/// ABI-compatible Windows GUID storage.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guid {
    pub data1: u32,
    pub data2: u16,
    pub data3: u16,
    pub data4: [u8; 8],
}

/// Class identifier `{F71A8435-AA10-40A6-8334-49EEC8FE9C63}`.
pub const DRIVER_CLASS_ID: Guid = Guid {
    data1: 0xf71a_8435,
    data2: 0xaa10,
    data3: 0x40a6,
    data4: [0x83, 0x34, 0x49, 0xee, 0xc8, 0xfe, 0x9c, 0x63],
};

const IID_IUNKNOWN: Guid = Guid {
    data1: 0,
    data2: 0,
    data3: 0,
    data4: [0xc0, 0, 0, 0, 0, 0, 0, 0x46],
};
const IID_ICLASSFACTORY: Guid = Guid {
    data1: 1,
    ..IID_IUNKNOWN
};
const IID_ISTIUSD: Guid = Guid {
    data1: 0x0c9b_b460,
    data2: 0x51ac,
    data3: 0x11d0,
    data4: [0x90, 0xea, 0x00, 0xaa, 0x00, 0x60, 0xf8, 0x6c],
};

type QueryInterfaceFn =
    unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> i32;
type AddRefFn = unsafe extern "system" fn(*mut c_void) -> u32;
type ReleaseFn = unsafe extern "system" fn(*mut c_void) -> u32;

// Field order follows the C declarations in Windows SDK unknwnbase.h
// (IUnknown lines 198-223 and IClassFactory lines 477-517). These vtables
// expose only the interfaces actually supported.
#[repr(C)]
struct ClassFactoryVtable {
    query_interface: QueryInterfaceFn,
    add_ref: AddRefFn,
    release: ReleaseFn,
    create_instance:
        unsafe extern "system" fn(*mut c_void, *mut c_void, *const Guid, *mut *mut c_void) -> i32,
    lock_server: unsafe extern "system" fn(*mut c_void, i32) -> i32,
}

#[repr(C)]
struct Factory {
    vtable: *const ClassFactoryVtable,
    refs: AtomicU32,
}

#[repr(C)]
struct Instance {
    vtable: *const sti::Vtable,
    refs: AtomicU32,
    state: sti::State,
}

// One hold covers every live factory/instance and every balanced LockServer
// lock. DllCanUnloadNow reads this single atomic so its decision cannot combine
// snapshots from independent counters. The second counter is only for
// rejecting unmatched unlocks; it is never consulted by DllCanUnloadNow.
struct ModuleState {
    holds: AtomicU32,
    server_locks: AtomicU32,
}

impl ModuleState {
    const fn new() -> Self {
        Self {
            holds: AtomicU32::new(0),
            server_locks: AtomicU32::new(0),
        }
    }

    fn acquire_object(&self) -> bool {
        increment_count(&self.holds)
    }

    fn release_object(&self) {
        let _ = decrement_count(&self.holds);
    }

    fn lock_server(&self, lock: bool) -> i32 {
        if lock {
            if !increment_count(&self.holds) {
                E_UNEXPECTED
            } else if increment_count(&self.server_locks) {
                S_OK
            } else {
                let _ = decrement_count(&self.holds);
                E_UNEXPECTED
            }
        } else if decrement_count(&self.server_locks) {
            let _ = decrement_count(&self.holds);
            S_OK
        } else {
            E_UNEXPECTED
        }
    }

    fn can_unload(&self) -> bool {
        self.holds.load(Ordering::Acquire) == 0
    }
}

static MODULE_STATE: ModuleState = ModuleState::new();

static FACTORY_VTABLE: ClassFactoryVtable = ClassFactoryVtable {
    query_interface: factory_query_interface,
    add_ref: factory_add_ref,
    release: factory_release,
    create_instance: factory_create_instance,
    lock_server: factory_lock_server,
};

fn catch_hresult(f: impl FnOnce() -> i32) -> i32 {
    catch_unwind(AssertUnwindSafe(f)).unwrap_or(E_UNEXPECTED)
}

fn increment_count(counter: &AtomicU32) -> bool {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_add(1)
        })
        .is_ok()
}

fn decrement_count(counter: &AtomicU32) -> bool {
    counter
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
            value.checked_sub(1)
        })
        .is_ok()
}

fn increment_ref(counter: &AtomicU32) -> u32 {
    loop {
        let current = counter.load(Ordering::Acquire);
        if current == u32::MAX {
            // AddRef has no error return. A saturated count is therefore a
            // permanent pin; Release preserves MAX rather than pretending to
            // remove a reference that could not be counted.
            return u32::MAX;
        }
        if counter
            .compare_exchange_weak(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return current + 1;
        }
    }
}

fn release_ref(counter: &AtomicU32) -> Option<u32> {
    loop {
        let current = counter.load(Ordering::Acquire);
        if current == 0 {
            return None;
        }
        if current == u32::MAX {
            // MAX denotes the permanent pin established by increment_ref.
            return Some(u32::MAX);
        }
        if counter
            .compare_exchange_weak(current, current - 1, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Some(current - 1);
        }
    }
}

unsafe fn clear_output(output: *mut *mut c_void) -> Result<(), i32> {
    if output.is_null() {
        return Err(E_POINTER);
    }
    // SAFETY: callers of these COM entry points must provide writable output
    // storage when the pointer is non-null; the public entry-point contract
    // documents that requirement.
    unsafe { *output = ptr::null_mut() };
    Ok(())
}

unsafe extern "system" fn factory_query_interface(
    this: *mut c_void,
    riid: *const Guid,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: COM supplies a live interface pointer and the implementation's
    // helper validates the output and input pointers before dereferencing them.
    catch_hresult(|| unsafe { factory_query_interface_impl(this, riid, output) })
}

unsafe fn factory_query_interface_impl(
    this: *mut c_void,
    riid: *const Guid,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: this helper is called only from the COM entry point, whose
    // contract requires writable output storage when output is non-null.
    if let Err(error) = unsafe { clear_output(output) } {
        return error;
    }
    if this.is_null() || riid.is_null() {
        return E_POINTER;
    }
    // SAFETY: this and riid are valid for the duration of this COM call under
    // the interface contract, and this points to a Factory object.
    let requested = unsafe { *riid };
    if requested != IID_IUNKNOWN && requested != IID_ICLASSFACTORY {
        return E_NOINTERFACE;
    }
    // SAFETY: this is a live Factory interface pointer; each successful
    // QueryInterface returns one additional owned reference.
    let factory = unsafe { &*this.cast::<Factory>() };
    increment_ref(&factory.refs);
    // SAFETY: output was checked and cleared above.
    unsafe { *output = this };
    S_OK
}

unsafe extern "system" fn factory_add_ref(this: *mut c_void) -> u32 {
    if this.is_null() {
        return 0;
    }
    // SAFETY: a non-null AddRef receiver is a live Factory pointer under the
    // IUnknown ownership contract. This path contains no panicking operation.
    unsafe { increment_ref(&(*this.cast::<Factory>()).refs) }
}

unsafe extern "system" fn factory_release(this: *mut c_void) -> u32 {
    if this.is_null() {
        return 0;
    }
    // SAFETY: a non-null Release receiver is a live Factory pointer while the
    // caller owns the reference being released. This path contains no
    // panicking operation.
    unsafe {
        let factory = &*this.cast::<Factory>();
        let Some(remaining) = release_ref(&factory.refs) else {
            return 0;
        };
        if remaining == 0 {
            // Keep the module held while the allocation is being destroyed.
            drop(Box::from_raw(this.cast::<Factory>()));
            MODULE_STATE.release_object();
        }
        remaining
    }
}

unsafe extern "system" fn factory_create_instance(
    this: *mut c_void,
    outer: *mut c_void,
    riid: *const Guid,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: COM supplies the raw arguments; the helper validates pointer
    // presence before reading or writing any of them.
    catch_hresult(|| unsafe { factory_create_instance_impl(this, outer, riid, output) })
}

unsafe fn factory_create_instance_impl(
    this: *mut c_void,
    outer: *mut c_void,
    riid: *const Guid,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: this helper is called only from the COM entry point, whose
    // contract requires writable output storage when output is non-null.
    if let Err(error) = unsafe { clear_output(output) } {
        return error;
    }
    if this.is_null() {
        return E_POINTER;
    }
    // Aggregation is deliberately unsupported. Check only pointer presence,
    // never dereference or query the outer object.
    if !outer.is_null() {
        return CLASS_E_NOAGGREGATION;
    }
    if riid.is_null() {
        return E_POINTER;
    }
    // SAFETY: riid is non-null and valid for the duration of this call under
    // the IClassFactory contract.
    let requested = unsafe { *riid };
    if requested != IID_IUNKNOWN && requested != IID_ISTIUSD {
        return E_NOINTERFACE;
    }
    let object = Box::new(Instance {
        vtable: &sti::VTABLE,
        refs: AtomicU32::new(1),
        state: sti::State::new(),
    });
    if !MODULE_STATE.acquire_object() {
        return E_UNEXPECTED;
    }
    // SAFETY: output was checked and cleared above; the caller receives one
    // owned reference and must release it exactly once.
    unsafe { *output = Box::into_raw(object).cast::<c_void>() };
    S_OK
}

unsafe extern "system" fn factory_lock_server(this: *mut c_void, lock: i32) -> i32 {
    catch_hresult(|| {
        if this.is_null() {
            return E_POINTER;
        }
        MODULE_STATE.lock_server(lock != 0)
    })
}

unsafe extern "system" fn instance_query_interface(
    this: *mut c_void,
    riid: *const Guid,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: COM supplies a live interface pointer and the implementation's
    // helper validates the output and input pointers before dereferencing them.
    catch_hresult(|| unsafe { instance_query_interface_impl(this, riid, output) })
}

unsafe fn instance_query_interface_impl(
    this: *mut c_void,
    riid: *const Guid,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: this helper is called only from the COM entry point, whose
    // contract requires writable output storage when output is non-null.
    if let Err(error) = unsafe { clear_output(output) } {
        return error;
    }
    if this.is_null() || riid.is_null() {
        return E_POINTER;
    }
    // SAFETY: this and riid are valid for this COM call under the interface
    // contract, and this points to an Instance object.
    let requested = unsafe { *riid };
    if requested != IID_IUNKNOWN && requested != IID_ISTIUSD {
        return E_NOINTERFACE;
    }
    // SAFETY: this is a live Instance interface pointer; successful identity
    // queries add one owned reference.
    let instance = unsafe { &*this.cast::<Instance>() };
    increment_ref(&instance.refs);
    // SAFETY: output was checked and cleared above.
    unsafe { *output = this };
    S_OK
}

unsafe extern "system" fn instance_add_ref(this: *mut c_void) -> u32 {
    if this.is_null() {
        return 0;
    }
    // SAFETY: a non-null AddRef receiver is a live Instance pointer under the
    // IUnknown ownership contract. This path contains no panicking operation.
    unsafe { increment_ref(&(*this.cast::<Instance>()).refs) }
}

unsafe extern "system" fn instance_release(this: *mut c_void) -> u32 {
    if this.is_null() {
        return 0;
    }
    // SAFETY: a non-null Release receiver is a live Instance pointer while the
    // caller owns the reference being released. This path contains no
    // panicking operation.
    unsafe {
        let instance = &*this.cast::<Instance>();
        let Some(remaining) = release_ref(&instance.refs) else {
            return 0;
        };
        if remaining == 0 {
            // Keep the module held while the allocation is being destroyed.
            drop(Box::from_raw(this.cast::<Instance>()));
            MODULE_STATE.release_object();
        }
        remaining
    }
}

/// Returns `S_OK` when no factory/object reference or server lock remains.
#[unsafe(no_mangle)]
pub extern "system" fn DllCanUnloadNow() -> i32 {
    if MODULE_STATE.can_unload() {
        S_OK
    } else {
        S_FALSE
    }
}

/// Return a class factory for [`DRIVER_CLASS_ID`].
///
/// The returned pointer carries one owned `IClassFactory` reference. The
/// caller must invoke its vtable `Release` exactly once after it is finished.
/// The factory supports `IUnknown` and `IClassFactory`; instances support
/// `IUnknown` and `IStiUSD`. `IWiaMiniDrv` remains unavailable.
///
/// # Safety
/// `class_id` and `interface_id`, when non-null, must point to readable GUIDs
/// that remain valid for this synchronous call. `output` must be non-null and
/// point to writable pointer storage. A successful call writes one owned COM
/// interface reference to `output`, which the caller must release exactly once.
#[unsafe(no_mangle)]
pub unsafe extern "system" fn DllGetClassObject(
    class_id: *const Guid,
    interface_id: *const Guid,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: the helper validates pointer presence before dereferencing the
    // caller-provided GUIDs or output storage.
    catch_hresult(|| unsafe { dll_get_class_object_impl(class_id, interface_id, output) })
}

unsafe fn dll_get_class_object_impl(
    class_id: *const Guid,
    interface_id: *const Guid,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: this helper is called only from the public entry point, whose
    // contract requires writable output storage when output is non-null.
    if let Err(error) = unsafe { clear_output(output) } {
        return error;
    }
    if class_id.is_null() || interface_id.is_null() {
        return E_POINTER;
    }
    // SAFETY: non-null input GUIDs are readable for this synchronous call by
    // the public function's safety contract.
    if unsafe { *class_id } != DRIVER_CLASS_ID {
        return CLASS_E_CLASSNOTAVAILABLE;
    }
    // SAFETY: interface_id is non-null and valid under DllGetClassObject's
    // public safety contract.
    let requested = unsafe { *interface_id };
    if requested != IID_IUNKNOWN && requested != IID_ICLASSFACTORY {
        return E_NOINTERFACE;
    }
    let factory = Box::new(Factory {
        vtable: &FACTORY_VTABLE,
        refs: AtomicU32::new(1),
    });
    if !MODULE_STATE.acquire_object() {
        return E_UNEXPECTED;
    }
    // SAFETY: output was checked and cleared above; the caller receives the
    // initial owned factory reference.
    unsafe { *output = Box::into_raw(factory).cast::<c_void>() };
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_hold_survives_lock_to_factory_handoff() {
        let state = ModuleState::new();
        assert_eq!(state.lock_server(true), S_OK);
        assert_eq!(state.holds.load(Ordering::Acquire), 1);

        // Model the real production operations: a factory hold is acquired
        // before another thread releases the server lock.
        assert!(state.acquire_object());
        assert_eq!(state.lock_server(false), S_OK);
        assert_eq!(state.holds.load(Ordering::Acquire), 1);
        assert!(
            !state.can_unload(),
            "a live factory must prevent unload after the lock handoff"
        );

        state.release_object();
        assert!(state.can_unload());
    }

    #[test]
    fn saturated_reference_is_a_permanent_pin() {
        let refs = AtomicU32::new(u32::MAX - 1);
        assert_eq!(increment_ref(&refs), u32::MAX);
        assert_eq!(refs.load(Ordering::Acquire), u32::MAX);
        assert_eq!(release_ref(&refs), Some(u32::MAX));
        assert_eq!(refs.load(Ordering::Acquire), u32::MAX);
    }
}
