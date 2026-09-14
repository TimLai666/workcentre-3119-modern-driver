//! WIA service locking through its retained IStiDevice, never around it.

use super::{
    Connection, E_INVALIDARG, E_POINTER, E_UNEXPECTED, Interface, Lifecycle, WIA_ERROR_BUSY,
};
use std::{
    ffi::c_void,
    panic::{AssertUnwindSafe, catch_unwind},
};

// IStiDevice::LockDevice takes milliseconds. Bound waiting for another owner;
// this is not a scan deadline and does not terminate an existing transfer.
const LOCK_TIMEOUT_MS: u32 = 5000;

// SDK sti.h: IUnknown, Initialize, GetCapabilities, GetStatus, DeviceReset,
// Diagnostic, Escape, GetLastError, LockDevice, UnLockDevice.
#[repr(C)]
struct DevicePrefix {
    unknown: super::UnknownVtable,
    unused: [usize; 7],
    lock: unsafe extern "system" fn(*mut c_void, u32) -> i32,
    unlock: unsafe extern "system" fn(*mut c_void) -> i32,
}

pub(super) struct Borrow<'a> {
    interface: &'a Interface,
    connection: Option<Connection>,
    failed: bool,
}
impl<'a> Borrow<'a> {
    pub(super) fn device(&self) -> &[u16] {
        &self.connection.as_ref().unwrap().device
    }
    pub(super) fn take(interface: &'a Interface) -> Result<Self, i32> {
        let mut state = interface.state();
        match &*state {
            Lifecycle::Busy => return Err(WIA_ERROR_BUSY),
            Lifecycle::Connected(_) => {}
            _ => return Err(E_UNEXPECTED),
        }
        let Lifecycle::Connected(connection) = std::mem::replace(&mut *state, Lifecycle::Busy)
        else {
            unreachable!()
        };
        Ok(Self {
            interface,
            connection: Some(connection),
            failed: false,
        })
    }

    fn quarantine(&mut self, error: i32) -> i32 {
        self.failed = true;
        // Keep the lifecycle Busy while releasing COM resources. Their
        // destructors may reenter the driver, and must never run under the
        // lifecycle mutex.
        let connection = self.connection.take();
        drop(connection);
        *self.interface.state() = Lifecycle::Failed;
        error
    }

    fn call_helper(&self, lock: bool) -> Result<(), i32> {
        let Some(helper) = self.connection.as_ref().unwrap()._helper.as_ref() else {
            return Err(E_UNEXPECTED);
        };
        // SAFETY: drvInitializeWia supplies an IStiDevice callable in this COM
        // apartment. Borrow owns its retained reference across the entire call;
        // Busy prevents reentrant disconnect, and no mutex is held here.
        let hr = unsafe {
            let methods = &**helper.0.cast::<*const DevicePrefix>();
            if lock {
                (methods.lock)(helper.0, LOCK_TIMEOUT_MS)
            } else {
                (methods.unlock)(helper.0)
            }
        };
        if hr == 0 {
            Ok(())
        } else if hr < 0 {
            Err(hr)
        } else {
            // IStiDevice documents only S_OK as success. Unknown positive
            // results must not tell the service it has acquired the lock.
            Err(E_UNEXPECTED)
        }
    }
}
impl Drop for Borrow<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() || self.failed {
            // Keep Busy while dropping resources which may reenter the driver.
            drop(self.connection.take());
            *self.interface.state() = Lifecycle::Failed;
        } else if let Some(connection) = self.connection.take() {
            *self.interface.state() = Lifecycle::Connected(connection);
        }
    }
}

pub(super) fn dispatch(interface: &Interface, lock: bool) -> i32 {
    super::super::catch_hresult(|| {
        let mut borrowed = match Borrow::take(interface) {
            Ok(borrowed) => borrowed,
            Err(hr) => return hr,
        };
        match borrowed.call_helper(lock) {
            Ok(()) => 0,
            Err(error) if !lock => borrowed.quarantine(error),
            Err(error) => error,
        }
    })
}

struct ServiceLock<'borrow, 'interface> {
    borrowed: &'borrow mut Borrow<'interface>,
    armed: bool,
}

impl<'borrow, 'interface> ServiceLock<'borrow, 'interface> {
    fn acquire(
        borrowed: &'borrow mut Borrow<'interface>,
    ) -> Result<ServiceLock<'borrow, 'interface>, i32> {
        borrowed.call_helper(true)?;
        Ok(Self {
            borrowed,
            armed: true,
        })
    }

    fn release(mut self) -> Result<(), i32> {
        self.armed = false;
        match self.borrowed.call_helper(false) {
            Ok(()) => Ok(()),
            Err(error) => Err(self.borrowed.quarantine(error)),
        }
    }
}

impl Drop for ServiceLock<'_, '_> {
    fn drop(&mut self) {
        if self.armed {
            self.armed = false;
            // Cleanup runs during unwinding as well. A second Rust panic while
            // invoking the helper must not replace the original panic.
            match catch_unwind(AssertUnwindSafe(|| self.borrowed.call_helper(false))) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    self.borrowed.quarantine(error);
                }
                Err(_) => {
                    self.borrowed.quarantine(E_UNEXPECTED);
                }
            }
        }
    }
}

fn with_locked_capabilities<T>(
    borrowed: &mut Borrow<'_>,
    query: impl FnOnce() -> Result<crate::protocol::Capabilities, i32>,
    publish: impl FnOnce(crate::protocol::Capabilities) -> Result<T, i32>,
) -> Result<T, i32> {
    let service_lock = ServiceLock::acquire(borrowed)?;
    let capabilities = query();
    let unlock = service_lock.release();

    let capabilities = match capabilities {
        Ok(capabilities) => {
            unlock?;
            capabilities
        }
        Err(primary) => {
            // Preserve the command/query failure when cleanup also fails.
            let _ = unlock;
            return Err(primary);
        }
    };
    publish(capabilities)
}

/// Lock the retained WIA service device, query capabilities through the
/// already locked STI session, unlock it, and keep the WIA connection borrowed
/// while the caller publishes the snapshot into the service item.
///
/// The callback runs after a successful unlock but before `Borrow` is dropped,
/// so reentrant lifecycle calls continue to observe `Busy`. The callback is
/// skipped when locking, the capability query, or cleanup fails.
///
/// # Safety
/// `interface` must be the embedded `Interface` field of the live
/// `com_server::Instance` returned by this crate's class factory. The caller
/// must keep the containing COM object and its apartment alive for the entire
/// call, including `publish`.
pub(super) unsafe fn with_live_capabilities<T>(
    interface: &Interface,
    publish: impl FnOnce(crate::protocol::Capabilities) -> Result<T, i32>,
) -> Result<T, i32> {
    catch_unwind(AssertUnwindSafe(|| {
        let mut borrowed = Borrow::take(interface)?;
        with_locked_capabilities(
            &mut borrowed,
            || {
                // SAFETY: this API is called only with the embedded Interface
                // belonging to our live Instance. `owner` recovers that
                // containing object from the repr(C) field offset, and the
                // caller's COM reference keeps it alive.
                let owner = unsafe { super::owner(interface as *const Interface as *mut c_void) };
                // SAFETY: `owner` is the containing Instance recovered from its
                // live embedded minidriver interface; State remains alive for
                // this call.
                unsafe {
                    (&*owner.cast::<super::super::Instance>())
                        .state
                        .live_capabilities()
                }
                .map_err(|(code, _text)| code)
            },
            publish,
        )
    }))
    .unwrap_or(Err(E_UNEXPECTED))
}

unsafe fn entry(
    this: *mut c_void,
    context: *mut u8,
    flags: i32,
    error: *mut i32,
    lock: bool,
) -> i32 {
    if error.is_null() {
        return E_POINTER;
    }
    let result = if this.is_null() || context.is_null() || flags != 0 {
        E_INVALIDARG
    } else {
        // SAFETY: COM caller retains our live secondary interface. The real
        // service context is only checked for presence; never fabricated/read.
        dispatch(unsafe { &*this.cast::<Interface>() }, lock)
    };
    // SAFETY: non-null output is valid writable LONG under the COM contract.
    unsafe { super::report(error, result) }
}

pub(super) unsafe extern "system" fn lock(
    this: *mut c_void,
    context: *mut u8,
    flags: i32,
    error: *mut i32,
) -> i32 {
    // SAFETY: forward the original native COM receiver and outputs.
    unsafe { entry(this, context, flags, error, true) }
}

pub(super) unsafe extern "system" fn unlock(
    this: *mut c_void,
    context: *mut u8,
    flags: i32,
    error: *mut i32,
) -> i32 {
    // SAFETY: forward the original native COM receiver and outputs.
    unsafe { entry(this, context, flags, error, false) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::com_server::{self, sti};
    use std::{
        cell::Cell,
        panic::{AssertUnwindSafe, catch_unwind},
        ptr,
        sync::atomic::AtomicU32,
    };

    #[repr(C)]
    struct TestDevice {
        vtable: *const DevicePrefix,
        refs: Cell<u32>,
        lock_result: Cell<i32>,
        unlock_result: Cell<i32>,
        lock_calls: Cell<u32>,
        unlock_calls: Cell<u32>,
        interface: *const Interface,
    }

    unsafe extern "system" fn device_query(
        _this: *mut c_void,
        _iid: *const com_server::Guid,
        output: *mut *mut c_void,
    ) -> i32 {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: the test passes writable query output storage.
        unsafe { *output = ptr::null_mut() };
        0x8000_4002u32 as i32
    }

    unsafe extern "system" fn device_add_ref(this: *mut c_void) -> u32 {
        // SAFETY: the fixture keeps the stack/heap object alive for every
        // retained reference released by the driver.
        let device = unsafe { &*this.cast::<TestDevice>() };
        let next = device.refs.get() + 1;
        device.refs.set(next);
        next
    }

    unsafe extern "system" fn device_release(this: *mut c_void) -> u32 {
        // SAFETY: the fixture keeps the object alive until the driver releases
        // the retained connection reference.
        let device = unsafe { &*this.cast::<TestDevice>() };
        // SAFETY: the pointer targets the fixture's live embedded Interface.
        let reentrant_result = unsafe { (*device.interface).disconnect_client() };
        assert_eq!(reentrant_result, WIA_ERROR_BUSY);
        let next = device.refs.get() - 1;
        device.refs.set(next);
        next
    }

    unsafe extern "system" fn device_lock(this: *mut c_void, timeout: u32) -> i32 {
        // SAFETY: the fixture and embedded interface remain live throughout
        // the synchronous service call.
        let device = unsafe { &*this.cast::<TestDevice>() };
        assert_eq!(timeout, LOCK_TIMEOUT_MS);
        // SAFETY: the pointer targets the fixture's live embedded Interface.
        let reentrant_result = unsafe { (*device.interface).disconnect_client() };
        assert_eq!(reentrant_result, WIA_ERROR_BUSY);
        device.lock_calls.set(device.lock_calls.get() + 1);
        device.lock_result.get()
    }

    unsafe extern "system" fn device_unlock(this: *mut c_void) -> i32 {
        // SAFETY: the fixture and embedded interface remain live throughout
        // the synchronous service call.
        let device = unsafe { &*this.cast::<TestDevice>() };
        // SAFETY: the pointer targets the fixture's live embedded Interface.
        let reentrant_result = unsafe { (*device.interface).disconnect_client() };
        assert_eq!(reentrant_result, WIA_ERROR_BUSY);
        device.unlock_calls.set(device.unlock_calls.get() + 1);
        device.unlock_result.get()
    }

    static DEVICE_VTABLE: DevicePrefix = DevicePrefix {
        unknown: super::super::UnknownVtable {
            query: device_query,
            add_ref: device_add_ref,
            release: device_release,
        },
        unused: [0; 7],
        lock: device_lock,
        unlock: device_unlock,
    };

    struct Fixture {
        device: Box<TestDevice>,
        instance: Box<super::super::super::Instance>,
        _apartment: ComApartment,
    }

    struct ComApartment;

    impl ComApartment {
        fn new() -> Self {
            // SAFETY: this test owns the current thread's COM apartment and
            // balances the successful initialization in Drop.
            let result = unsafe { CoInitializeEx(ptr::null_mut(), 0) };
            assert!(
                result == 0 || result == 1,
                "CoInitializeEx failed: {result:#x}"
            );
            Self
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            // SAFETY: new() only returns after COM initialization succeeded.
            unsafe { CoUninitialize() };
        }
    }

    impl Fixture {
        fn new() -> Self {
            let apartment = ComApartment::new();
            let instance = Box::new(super::super::super::Instance {
                vtable: &sti::VTABLE,
                mini: Interface::new(),
                refs: AtomicU32::new(1),
                state: sti::State::new(),
            });
            let interface = &instance.mini as *const Interface;
            let mut device = Box::new(TestDevice {
                vtable: &DEVICE_VTABLE,
                refs: Cell::new(1),
                lock_result: Cell::new(0),
                unlock_result: Cell::new(0),
                lock_calls: Cell::new(0),
                unlock_calls: Cell::new(0),
                interface,
            });
            // SAFETY: this uses real Windows WIA item objects with synthetic
            // names and a synthetic IStiDevice helper, without a service context
            // or USB access.
            unsafe {
                instance
                    .mini
                    .initialize_tree(
                        interface.cast_mut().cast(),
                        vec![65],
                        "synthetic\\Root".encode_utf16().collect(),
                        ptr::from_mut(device.as_mut()).cast(),
                    )
                    .unwrap();
            }
            Self {
                device,
                instance,
                _apartment: apartment,
            }
        }

        fn interface(&self) -> &Interface {
            &self.instance.mini
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = self.instance.mini.disconnect_client();
        }
    }

    #[link(name = "Ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
        fn CoUninitialize();
    }

    fn capabilities() -> crate::protocol::Capabilities {
        crate::protocol::Capabilities {
            identity: "synthetic".to_owned(),
            resolution_mask: 1,
            mode_mask: 2,
            width_units: 1,
            length_units: 1,
            flatbed_length_units: 1,
            line_order: 0,
            compression_mask: 0,
        }
    }

    fn run_with_fixture<T>(
        fixture: &Fixture,
        query: impl FnOnce() -> Result<crate::protocol::Capabilities, i32>,
        publish: impl FnOnce(crate::protocol::Capabilities) -> Result<T, i32>,
    ) -> Result<T, i32> {
        let mut borrowed = Borrow::take(fixture.interface()).unwrap();
        with_locked_capabilities(&mut borrowed, query, publish)
    }

    #[test]
    fn borrow_rejects_idle_before_unsafe_container_of() {
        let interface = Interface::new();
        assert!(matches!(Borrow::take(&interface), Err(E_UNEXPECTED)));
    }

    #[test]
    fn live_capabilities_uses_embedded_state_and_cleans_up_without_initialization() {
        let fixture = Fixture::new();
        // SAFETY: Fixture owns the live Instance and its embedded Interface.
        let result: Result<(), i32> = unsafe {
            with_live_capabilities(fixture.interface(), |_| {
                panic!("publish must not run before STI initialization")
            })
        };
        assert_eq!(result, Err(0x8007_0015u32 as i32));
        assert_eq!(fixture.device.lock_calls.get(), 1);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
    }

    #[test]
    fn live_capabilities_success_unlocks_before_publish_and_keeps_borrow_busy() {
        let fixture = Fixture::new();
        let result = run_with_fixture(
            &fixture,
            || Ok(capabilities()),
            |caps| {
                assert_eq!(fixture.device.lock_calls.get(), 1);
                assert_eq!(fixture.device.unlock_calls.get(), 1);
                assert_eq!(fixture.interface().disconnect_client(), WIA_ERROR_BUSY);
                Ok(caps.identity)
            },
        );
        assert_eq!(result.unwrap(), "synthetic");
        assert_eq!(fixture.device.lock_calls.get(), 1);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
    }

    #[test]
    fn live_capabilities_lock_failure_does_not_unlock_or_query() {
        let fixture = Fixture::new();
        fixture.device.lock_result.set(0x8007_0005u32 as i32);
        let result: Result<(), i32> = run_with_fixture(
            &fixture,
            || -> Result<crate::protocol::Capabilities, i32> {
                panic!("query must not run after lock failure")
            },
            |_| panic!("publish must not run after lock failure"),
        );
        assert_eq!(result, Err(0x8007_0005u32 as i32));
        assert_eq!(fixture.device.lock_calls.get(), 1);
        assert_eq!(fixture.device.unlock_calls.get(), 0);
    }

    #[test]
    fn live_capabilities_query_failure_still_unlocks_and_preserves_primary() {
        let fixture = Fixture::new();
        let primary = 0x8000_4005u32 as i32;
        fixture.device.unlock_result.set(0x8007_0006u32 as i32);
        let result: Result<(), i32> = run_with_fixture(
            &fixture,
            || Err(primary),
            |_| panic!("publish must not run after query failure"),
        );
        assert_eq!(result, Err(primary));
        assert_eq!(fixture.device.lock_calls.get(), 1);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
        assert!(matches!(*fixture.interface().state(), Lifecycle::Failed));
    }

    #[test]
    fn live_capabilities_cleanup_failure_skips_publish() {
        let fixture = Fixture::new();
        let cleanup = 0x8007_0006u32 as i32;
        fixture.device.unlock_result.set(cleanup);
        let result: Result<(), i32> = run_with_fixture(
            &fixture,
            || Ok(capabilities()),
            |_| panic!("publish must not run after unlock failure"),
        );
        assert_eq!(result, Err(cleanup));
        assert_eq!(fixture.device.lock_calls.get(), 1);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
        assert!(matches!(*fixture.interface().state(), Lifecycle::Failed));
        assert_eq!(dispatch(fixture.interface(), false), E_UNEXPECTED);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
    }

    #[test]
    fn live_capabilities_positive_cleanup_failure_quarantines_without_retry() {
        let fixture = Fixture::new();
        fixture.device.unlock_result.set(1);
        let result: Result<(), i32> = run_with_fixture(
            &fixture,
            || Ok(capabilities()),
            |_| panic!("publish must not run after unlock failure"),
        );
        assert_eq!(result, Err(E_UNEXPECTED));
        assert_eq!(fixture.device.lock_calls.get(), 1);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
        assert!(matches!(*fixture.interface().state(), Lifecycle::Failed));
        assert_eq!(dispatch(fixture.interface(), false), E_UNEXPECTED);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
    }

    #[test]
    fn dispatch_negative_unlock_failure_quarantines_without_retry() {
        let fixture = Fixture::new();
        let error = 0x8007_0006u32 as i32;
        fixture.device.unlock_result.set(error);
        assert_eq!(dispatch(fixture.interface(), false), error);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
        assert!(matches!(*fixture.interface().state(), Lifecycle::Failed));
        assert_eq!(dispatch(fixture.interface(), false), E_UNEXPECTED);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
    }

    #[test]
    fn dispatch_positive_unlock_failure_quarantines_without_retry() {
        let fixture = Fixture::new();
        fixture.device.unlock_result.set(1);
        assert_eq!(dispatch(fixture.interface(), false), E_UNEXPECTED);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
        assert!(matches!(*fixture.interface().state(), Lifecycle::Failed));
        assert_eq!(dispatch(fixture.interface(), false), E_UNEXPECTED);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
    }

    #[test]
    fn live_capabilities_query_panic_still_unlocks_once() {
        let fixture = Fixture::new();
        let result: Result<Result<(), i32>, _> = catch_unwind(AssertUnwindSafe(|| {
            let mut borrowed = Borrow::take(fixture.interface()).unwrap();
            with_locked_capabilities::<()>(
                &mut borrowed,
                || -> Result<crate::protocol::Capabilities, i32> {
                    panic!("synthetic query panic")
                },
                |_| panic!("publish must not run after query panic"),
            )
        }));
        assert!(result.is_err());
        assert_eq!(fixture.device.lock_calls.get(), 1);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
    }

    #[test]
    fn live_capabilities_publish_panic_drops_connection_after_unlock() {
        let fixture = Fixture::new();
        let result = catch_unwind(AssertUnwindSafe(|| {
            let mut borrowed = Borrow::take(fixture.interface()).unwrap();
            with_locked_capabilities(
                &mut borrowed,
                || Ok(capabilities()),
                |_| -> Result<(), i32> { panic!("synthetic publish panic") },
            )
        }));
        assert!(result.is_err());
        assert_eq!(fixture.device.lock_calls.get(), 1);
        assert_eq!(fixture.device.unlock_calls.get(), 1);
        assert!(matches!(*fixture.interface().state(), Lifecycle::Failed));
    }

    #[test]
    fn native_lock_layout_and_absent_context_are_checked() {
        assert_eq!(std::mem::offset_of!(DevicePrefix, lock), 80);
        assert_eq!(std::mem::offset_of!(DevicePrefix, unlock), 88);
        let mut error = 7;
        // SAFETY: only null input pointers and valid output storage are used.
        unsafe {
            assert_eq!(
                lock(std::ptr::null_mut(), std::ptr::null_mut(), 0, &mut error),
                E_INVALIDARG
            );
            assert_eq!(error, E_INVALIDARG);
            assert_eq!(
                unlock(
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut()
                ),
                E_POINTER
            );
        }
    }
}

#[cfg(test)]
mod hardware;
