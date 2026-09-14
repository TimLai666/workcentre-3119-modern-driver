//! WIA service locking through its retained IStiDevice, never around it.

use super::{
    Connection, E_INVALIDARG, E_POINTER, E_UNEXPECTED, Interface, Lifecycle, WIA_ERROR_BUSY,
};
use std::ffi::c_void;

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
        })
    }
}
impl Drop for Borrow<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
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
        let borrowed = match Borrow::take(interface) {
            Ok(borrowed) => borrowed,
            Err(hr) => return hr,
        };
        let Some(helper) = borrowed.connection.as_ref().unwrap()._helper.as_ref() else {
            return E_UNEXPECTED;
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
        // Borrow restores the connection even when the helper returns an error.
        // LockDevice documents only S_OK as success. Unknown positive results
        // must not tell the service it has acquired the device lock.
        if hr > 0 { E_UNEXPECTED } else { hr }
    })
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
