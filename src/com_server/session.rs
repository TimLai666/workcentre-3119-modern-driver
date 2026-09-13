//! Exclusive resource ownership across callbacks; no external call holds the mutex.

use std::{
    io,
    sync::{Mutex, MutexGuard},
};

#[derive(Debug)]
pub(super) enum AccessError {
    Busy,
    NotLocked,
    Quarantined,
    Open(io::Error),
}

enum Connection<T> {
    Unlocked,
    Opening,
    Closing,
    Locked(T),
    InUse,
    Quarantined,
}

pub(super) struct SessionSlot<T> {
    state: Mutex<Connection<T>>,
}

impl<T> SessionSlot<T> {
    pub(super) const fn new() -> Self {
        Self {
            state: Mutex::new(Connection::Unlocked),
        }
    }

    fn state(&self) -> MutexGuard<'_, Connection<T>> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub(super) fn open(&self, open: impl FnOnce() -> io::Result<T>) -> Result<(), AccessError> {
        {
            let mut state = self.state();
            match &*state {
                Connection::Unlocked => *state = Connection::Opening,
                Connection::Quarantined => return Err(AccessError::Quarantined),
                _ => return Err(AccessError::Busy),
            }
        }
        let mut opening = TransitionGuard {
            slot: self,
            rollback: Some(Connection::Unlocked),
        };
        // Caller code and resource acquisition never execute under the mutex.
        let resource = open().map_err(AccessError::Open)?;
        {
            let mut state = self.state();
            // Disarm before publication: a later opener must not be undone by this guard.
            opening.rollback = None;
            *state = Connection::Locked(resource);
        }
        drop(opening);
        Ok(())
    }

    pub(super) fn unlock(&self) -> Result<(), AccessError> {
        let previous = {
            let mut state = self.state();
            match &*state {
                Connection::Unlocked => return Err(AccessError::NotLocked),
                Connection::Opening | Connection::Closing | Connection::InUse => {
                    return Err(AccessError::Busy);
                }
                Connection::Quarantined => return Err(AccessError::Quarantined),
                Connection::Locked(_) => std::mem::replace(&mut *state, Connection::Closing),
            }
        };
        let mut closing = TransitionGuard {
            slot: self,
            rollback: Some(Connection::Quarantined),
        };
        drop(previous);
        {
            let mut state = self.state();
            closing.rollback = None;
            *state = Connection::Unlocked;
        }
        Ok(())
    }

    /// `reusable` must be true only after the protocol confirms a safe command boundary.
    /// An unwinding operation defaults to quarantine and closes the resource.
    pub(super) fn with_session<R>(
        &self,
        operation: impl FnOnce(&mut T) -> (R, bool),
    ) -> Result<R, AccessError> {
        let resource = {
            let mut state = self.state();
            match &*state {
                Connection::Unlocked => return Err(AccessError::NotLocked),
                Connection::Opening | Connection::Closing | Connection::InUse => {
                    return Err(AccessError::Busy);
                }
                Connection::Quarantined => return Err(AccessError::Quarantined),
                Connection::Locked(_) => {}
            }
            match std::mem::replace(&mut *state, Connection::InUse) {
                Connection::Locked(resource) => resource,
                _ => unreachable!("locked state was checked under the same mutex"),
            }
        };
        let mut lease = Lease {
            slot: self,
            resource: Some(resource),
            reusable: false,
        };
        let (result, reusable) = operation(lease.resource.as_mut().expect("lease owns resource"));
        lease.reusable = reusable;
        drop(lease);
        Ok(result)
    }
}

struct TransitionGuard<'a, T> {
    slot: &'a SessionSlot<T>,
    rollback: Option<Connection<T>>,
}
impl<T> Drop for TransitionGuard<'_, T> {
    fn drop(&mut self) {
        if let Some(rollback) = self.rollback.take() {
            *self.slot.state() = rollback;
        }
    }
}

struct Lease<'a, T> {
    slot: &'a SessionSlot<T>,
    resource: Option<T>,
    reusable: bool,
}
impl<T> Drop for Lease<'_, T> {
    fn drop(&mut self) {
        {
            let mut state = self.slot.state();
            *state = if self.reusable {
                match self.resource.take() {
                    Some(resource) => Connection::Locked(resource),
                    None => Connection::Quarantined,
                }
            } else {
                Connection::Quarantined
            };
        }
        // A failed resource is dropped only after the mutex has been released.
        drop(self.resource.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        Arc,
        atomic::{AtomicU32, Ordering},
    };

    struct Resource(Arc<AtomicU32>, u32);
    impl Drop for Resource {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn callbacks_can_reenter_without_unlocking_or_replacing_the_owned_resource() {
        let drops = Arc::new(AtomicU32::new(0));
        let slot = SessionSlot::new();
        slot.open(|| Ok(Resource(drops.clone(), 42))).unwrap();
        let value = slot
            .with_session(|resource| {
                assert!(matches!(slot.unlock(), Err(AccessError::Busy)));
                assert!(matches!(
                    slot.open(|| panic!("must not reopen")),
                    Err(AccessError::Busy)
                ));
                assert!(matches!(
                    slot.with_session::<()>(|_| panic!("must not reenter")),
                    Err(AccessError::Busy)
                ));
                resource.1 += 1;
                (resource.1, true)
            })
            .unwrap();
        assert_eq!(value, 43);
        assert_eq!(
            slot.with_session(|resource| (resource.1, true)).unwrap(),
            43
        );
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        slot.unlock().unwrap();
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn failed_or_panicking_operation_quarantines_and_drops_exactly_once() {
        for panic in [false, true] {
            let drops = Arc::new(AtomicU32::new(0));
            let slot = SessionSlot::new();
            slot.open(|| Ok(Resource(drops.clone(), 0))).unwrap();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                slot.with_session(|_| {
                    assert!(!panic, "synthetic callback panic");
                    ((), false)
                })
            }));
            assert_eq!(result.is_err(), panic);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
            assert!(matches!(
                slot.with_session::<()>(|_| panic!("quarantined")),
                Err(AccessError::Quarantined)
            ));
            assert!(matches!(
                slot.open(|| panic!("no implicit recovery")),
                Err(AccessError::Quarantined)
            ));
            assert!(matches!(slot.unlock(), Err(AccessError::Quarantined)));
            drop(slot);
            assert_eq!(drops.load(Ordering::SeqCst), 1);
        }
    }

    #[test]
    fn failed_open_and_open_panic_leave_slot_available() {
        let slot = SessionSlot::<u32>::new();
        assert!(matches!(
            slot.open(|| Err(std::io::Error::other("synthetic open failure"))),
            Err(AccessError::Open(_))
        ));
        assert!(std::panic::catch_unwind(|| slot.open(|| panic!("synthetic open panic"))).is_err());
        slot.open(|| {
            assert!(matches!(slot.unlock(), Err(AccessError::Busy)));
            Ok(7)
        })
        .unwrap();
        assert_eq!(slot.with_session(|value| (*value, true)).unwrap(), 7);
        slot.unlock().unwrap();
        assert!(matches!(slot.unlock(), Err(AccessError::NotLocked)));
    }

    #[test]
    fn concurrent_calls_fail_promptly_while_callback_owns_resource() {
        let slot = Arc::new(SessionSlot::new());
        slot.open(|| Ok(42u32)).unwrap();
        let (entered, ready) = std::sync::mpsc::channel();
        let (release, proceed) = std::sync::mpsc::channel();
        let owner = slot.clone();
        let active = std::thread::spawn(move || {
            owner
                .with_session(|value| {
                    entered.send(()).unwrap();
                    proceed.recv().unwrap();
                    (*value, true)
                })
                .unwrap()
        });
        ready
            .recv_timeout(std::time::Duration::from_secs(2))
            .unwrap();
        let contender = slot.clone();
        let (sent, response) = std::sync::mpsc::channel();
        let other = std::thread::spawn(move || {
            let busy = matches!(contender.unlock(), Err(AccessError::Busy))
                && matches!(
                    contender.with_session::<()>(|_| panic!("already in use")),
                    Err(AccessError::Busy)
                );
            sent.send(busy).unwrap();
        });
        let result = response.recv_timeout(std::time::Duration::from_secs(2));
        // Release the owner before assertions so a failing wait cannot strand the test.
        release.send(()).unwrap();
        assert_eq!(active.join().unwrap(), 42);
        other.join().unwrap();
        assert!(result.unwrap());
        slot.unlock().unwrap();
    }

    #[test]
    fn unlock_does_not_allow_reopen_until_resource_is_closed() {
        struct ClosingResource(std::sync::Weak<SessionSlot<ClosingResource>>);
        impl Drop for ClosingResource {
            fn drop(&mut self) {
                let slot = self.0.upgrade().unwrap();
                assert!(matches!(
                    slot.open(|| panic!("old resource is still closing")),
                    Err(AccessError::Busy)
                ));
            }
        }
        let slot = Arc::new(SessionSlot::new());
        slot.open(|| Ok(ClosingResource(Arc::downgrade(&slot))))
            .unwrap();
        slot.unlock().unwrap();
        slot.open(|| Ok(ClosingResource(Arc::downgrade(&slot))))
            .unwrap();
        slot.unlock().unwrap();
    }
}
