//! Per-acquisition cancellation without holding the native lifecycle mutex.

use super::{E_INVALIDARG, E_NOTIMPL, E_POINTER, Guid, Interface, WIA_ERROR_BUSY};
use std::ffi::c_void;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

pub(super) const CANCEL_IO: Guid = Guid {
    data1: 0xc860f7b8,
    data2: 0x9ccd,
    data3: 0x41ea,
    data4: [0xbb, 0xbf, 0x4d, 0xd0, 0x9c, 0x5b, 0x17, 0x95],
};

pub(super) struct State(Mutex<Option<Active>>);
struct Active {
    device: Vec<u16>,
    flag: Arc<AtomicBool>,
}
pub(super) struct Job<'a> {
    state: &'a State,
    flag: Arc<AtomicBool>,
}

impl State {
    pub(super) const fn new() -> Self {
        Self(Mutex::new(None))
    }
    pub(super) fn begin(&self, device: &[u16]) -> Result<Job<'_>, i32> {
        let mut active = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if active.is_some() {
            return Err(WIA_ERROR_BUSY);
        }
        let flag = Arc::new(AtomicBool::new(false));
        *active = Some(Active {
            device: device.to_vec(),
            flag: flag.clone(),
        });
        Ok(Job { state: self, flag })
    }
    pub(super) fn cancel(&self, device: &[u16]) -> bool {
        let active = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(active) = &*active
            && active.device == device
        {
            active.flag.store(true, Ordering::Relaxed);
            return true;
        }
        false
    }
}
impl Job<'_> {
    pub(super) fn flag(&self) -> &AtomicBool {
        &self.flag
    }
    // Completion and cancellation are ordered by the same small mutex. An
    // event after removal cannot cancel the finished job or leak to the next.
    pub(super) fn finish(self) -> bool {
        self.remove()
    }
    fn remove(&self) -> bool {
        let mut active = self.state.0.lock().unwrap_or_else(|e| e.into_inner());
        if active
            .as_ref()
            .is_some_and(|a| Arc::ptr_eq(&a.flag, &self.flag))
        {
            let cancelled = self.flag.load(Ordering::Relaxed);
            *active = None;
            cancelled
        } else {
            false
        }
    }
}
impl Drop for Job<'_> {
    fn drop(&mut self) {
        self.remove();
    }
}

pub(super) unsafe extern "system" fn notify(
    this: *mut c_void,
    event: *const Guid,
    device: *mut u16,
    reserved: u32,
) -> i32 {
    if this.is_null() || event.is_null() || device.is_null() {
        return E_POINTER;
    }
    if reserved != 0 {
        return E_INVALIDARG;
    }
    // SAFETY: WIA retains receiver, GUID and BSTR for the synchronous call.
    // No USB or external COM call occurs while holding the cancellation mutex.
    super::super::catch_hresult(|| unsafe {
        if *event != CANCEL_IO {
            // Connection events are declared by drvGetCapabilities and raised
            // by Windows itself; the static tree needs no reaction to them.
            return if super::capabilities::declares_event(&*event) {
                0
            } else {
                E_NOTIMPL
            };
        }
        let device = match super::read_bstr(device) {
            Ok(device) => device,
            Err(hr) => return hr,
        };
        (*this.cast::<Interface>()).cancellation.cancel(&device);
        // A valid cancel notification with no current I/O is a successful no-op.
        0
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn cancel_matches_only_the_current_device_and_does_not_poison_next_job() {
        let state = State::new();
        assert!(!state.cancel(&[1]));
        let job = state.begin(&[1]).unwrap();
        assert!(state.begin(&[1]).is_err());
        assert!(!state.cancel(&[2]));
        assert!(!job.flag().load(Ordering::Relaxed));
        assert!(state.cancel(&[1]));
        assert!(job.flag().load(Ordering::Relaxed));
        assert!(job.finish());
        assert!(!state.cancel(&[1]));
        let next = state.begin(&[1]).unwrap();
        assert!(!next.flag().load(Ordering::Relaxed));
        assert!(!next.finish());
    }

    #[test]
    fn event_can_cancel_from_another_thread_and_abandoned_job_is_removed() {
        let state = State::new();
        let job = state.begin(&[65]).unwrap();
        std::thread::scope(|scope| {
            scope.spawn(|| assert!(state.cancel(&[65]))).join().unwrap();
        });
        assert!(job.finish());
        let job = state.begin(&[65]).unwrap();
        drop(job);
        assert!(!state.cancel(&[65]));
        assert!(!state.begin(&[65]).unwrap().finish());
    }

    #[test]
    fn native_event_checks_guid_device_identity_and_reserved_value() {
        use crate::com_server;
        use std::ptr;
        let mut factory = ptr::null_mut();
        let mut mini = ptr::null_mut();
        // SAFETY: all interfaces are owned real driver objects; event strings
        // are true BSTR allocations. No WIA application context is fabricated.
        unsafe {
            assert_eq!(
                com_server::DllGetClassObject(
                    &com_server::DRIVER_CLASS_ID,
                    &com_server::IID_ICLASSFACTORY,
                    &mut factory
                ),
                0
            );
            let f = &**factory.cast::<*const com_server::ClassFactoryVtable>();
            assert_eq!(
                (f.create_instance)(
                    factory,
                    ptr::null_mut(),
                    &com_server::IID_IWIAMINIDRV,
                    &mut mini
                ),
                0
            );
            (f.release)(factory);
            let a = SysAllocStringLen([65u16].as_ptr(), 1);
            let b = SysAllocStringLen([66u16].as_ptr(), 1);
            assert!(!a.is_null() && !b.is_null());
            let job = (*mini.cast::<Interface>())
                .cancellation
                .begin(&[65])
                .unwrap();
            assert_eq!(notify(mini, &CANCEL_IO, b, 0), 0);
            assert!(!job.flag().load(Ordering::Relaxed));
            assert_eq!(notify(mini, &CANCEL_IO, a, 1), E_INVALIDARG);
            assert_eq!(notify(mini, &com_server::DRIVER_CLASS_ID, a, 0), E_NOTIMPL);
            assert_eq!(
                notify(
                    mini,
                    &super::super::capabilities::WIA_EVENT_DEVICE_CONNECTED,
                    a,
                    0
                ),
                0,
                "declared connection events are accepted without touching the job"
            );
            assert_eq!(notify(mini, ptr::null(), a, 0), E_POINTER);
            assert!(!job.flag().load(Ordering::Relaxed));
            assert_eq!(notify(mini, &CANCEL_IO, a, 0), 0);
            assert!(job.finish());
            assert_eq!(notify(mini, &CANCEL_IO, a, 0), 0);
            assert!(
                !(*mini.cast::<Interface>())
                    .cancellation
                    .begin(&[65])
                    .unwrap()
                    .finish()
            );
            SysFreeString(a);
            SysFreeString(b);
            assert_eq!(super::super::release(mini), 0);
        }
    }
    #[link(name = "OleAut32")]
    unsafe extern "system" {
        fn SysAllocStringLen(value: *const u16, length: u32) -> *mut u16;
        fn SysFreeString(value: *mut u16);
    }
}
