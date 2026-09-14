//! Native IWiaMiniDrv ABI and identity. Service property integration is in progress.

use super::{Guid, Instance};
use std::{
    ffi::c_void,
    mem::offset_of,
    ptr,
    sync::{Mutex, MutexGuard},
};
mod acquire;
mod cancel;
mod formats;
mod locking;
mod properties;
mod tree;

const E_NOTIMPL: i32 = 0x80004001u32 as i32;
const E_POINTER: i32 = 0x80004003u32 as i32;
const E_INVALIDARG: i32 = 0x80070057u32 as i32;
const E_UNEXPECTED: i32 = 0x8000ffffu32 as i32;
const WIA_ERROR_BUSY: i32 = 0x80210006u32 as i32;

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

// SDK 10.0.26100.0 wiamindr_lh.h, IWiaMiniDrvVtbl. Opaque service structures
// remain pointers; never invent their contents or alias them to driver context.
#[repr(C)]
struct Vtable {
    query: super::QueryInterfaceFn,
    add_ref: super::AddRefFn,
    release: super::ReleaseFn,
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
    lock: ItemMethod,
    unlock: ItemMethod,
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
    delete: ItemMethod,
    free_context: unsafe extern "system" fn(*mut c_void, i32, *mut u8, *mut i32) -> i32,
    formats: ListMethod,
    notify: unsafe extern "system" fn(*mut c_void, *const Guid, *mut u16, u32) -> i32,
    uninitialize: unsafe extern "system" fn(*mut c_void, *mut u8) -> i32,
}

#[repr(C)]
pub(super) struct Interface {
    vtable: *const Vtable,
    lifecycle: Mutex<Lifecycle>,
    cancellation: cancel::State,
}
impl Interface {
    pub(super) const fn new() -> Self {
        Self {
            vtable: &VTABLE,
            lifecycle: Mutex::new(Lifecycle::Idle),
            cancellation: cancel::State::new(),
        }
    }
    fn state(&self) -> MutexGuard<'_, Lifecycle> {
        self.lifecycle.lock().unwrap_or_else(|e| e.into_inner())
    }

    unsafe fn initialize_tree(
        &self,
        this: *mut c_void,
        device: Vec<u16>,
        name: Vec<u16>,
        sti: *mut c_void,
    ) -> Result<*mut c_void, i32> {
        if device.is_empty() || name.is_empty() || device.contains(&0) || name.contains(&0) {
            return Err(E_INVALIDARG);
        }
        {
            let mut state = self.state();
            match &mut *state {
                Lifecycle::Connected(connection) => {
                    if connection.device != device || connection.name != name {
                        return Err(E_INVALIDARG);
                    }
                    connection.clients = connection.clients.checked_add(1).ok_or(E_UNEXPECTED)?;
                    // The service links application items to this driver-owned tree.
                    // This output follows drvInitializeWia, not an owned QI result.
                    return Ok(connection.tree.raw());
                }
                Lifecycle::Busy => return Err(WIA_ERROR_BUSY),
                Lifecycle::Failed => return Err(E_UNEXPECTED),
                Lifecycle::Idle => *state = Lifecycle::Busy,
            }
        }
        // Never invoke native COM methods while holding the lifecycle mutex.
        // SAFETY: caller keeps its driver reference, STI object and apartment live.
        let result = unsafe { tree::Tree::create(this, &name) };
        match result {
            Ok(tree) => {
                // SAFETY: optional STI interface is a live borrowed IStiDevice;
                // take one retained reference for the connection's lifetime.
                let helper = unsafe { Helper::retain(sti) };
                let root = tree.raw();
                *self.state() = Lifecycle::Connected(Connection {
                    tree,
                    _helper: helper,
                    device,
                    name,
                    clients: 1,
                });
                Ok(root)
            }
            Err(error) => {
                *self.state() = Lifecycle::Idle;
                Err(error)
            }
        }
    }

    fn disconnect_client(&self) -> i32 {
        let connection = {
            let mut state = self.state();
            match &mut *state {
                Lifecycle::Idle => return E_UNEXPECTED,
                Lifecycle::Busy => return WIA_ERROR_BUSY,
                Lifecycle::Failed => return E_UNEXPECTED,
                Lifecycle::Connected(connection) if connection.clients > 1 => {
                    connection.clients -= 1;
                    return 0;
                }
                Lifecycle::Connected(_) => {}
            }
            let Lifecycle::Connected(connection) = std::mem::replace(&mut *state, Lifecycle::Busy)
            else {
                unreachable!()
            };
            connection
        };
        let Connection {
            tree,
            _helper: helper,
            ..
        } = connection;
        let result = tree.close();
        drop(helper);
        *self.state() = if result.is_ok() {
            Lifecycle::Idle
        } else {
            Lifecycle::Failed
        };
        result.err().unwrap_or(0)
    }
}

enum Lifecycle {
    Idle,
    Busy,
    Connected(Connection),
    Failed,
}
struct Connection {
    tree: tree::Tree,
    _helper: Option<Helper>,
    device: Vec<u16>,
    name: Vec<u16>,
    clients: u32,
}
struct Helper(*mut c_void);
impl Helper {
    unsafe fn retain(raw: *mut c_void) -> Option<Self> {
        if raw.is_null() {
            return None;
        }
        // SAFETY: caller guarantees the borrowed STI object's IUnknown prefix.
        unsafe {
            ((**raw.cast::<*const UnknownVtable>()).add_ref)(raw);
        }
        Some(Self(raw))
    }
}
#[repr(C)]
struct UnknownVtable {
    query: super::QueryInterfaceFn,
    add_ref: super::AddRefFn,
    release: super::ReleaseFn,
}
impl Drop for Helper {
    fn drop(&mut self) {
        // SAFETY: one retained reference balances the connection's AddRef.
        unsafe {
            ((**self.0.cast::<*const UnknownVtable>()).release)(self.0);
        }
    }
}

unsafe fn owner(this: *mut c_void) -> *mut c_void {
    if this.is_null() {
        return ptr::null_mut();
    }
    // SAFETY: only our IWiaMiniDrv entry points call this with a live embedded
    // Interface pointer; subtracting its repr(C) field offset recovers Instance.
    unsafe { this.cast::<u8>().sub(offset_of!(Instance, mini)).cast() }
}
unsafe extern "system" fn query(
    this: *mut c_void,
    iid: *const Guid,
    output: *mut *mut c_void,
) -> i32 {
    // SAFETY: receiver is our secondary interface, outputs follow IUnknown.
    unsafe { super::instance_query_interface(owner(this), iid, output) }
}
unsafe extern "system" fn add_ref(this: *mut c_void) -> u32 {
    // SAFETY: receiver is our live secondary interface.
    unsafe { super::instance_add_ref(owner(this)) }
}
unsafe extern "system" fn release(this: *mut c_void) -> u32 {
    // SAFETY: caller owns the reference being released on our secondary interface.
    unsafe { super::instance_release(owner(this)) }
}

unsafe fn report(error: *mut i32, result: i32) -> i32 {
    if error.is_null() {
        return E_POINTER;
    }
    // SAFETY: COM caller supplies writable LONG output storage when non-null.
    unsafe {
        *error = if result < 0 { result } else { 0 };
    }
    result
}

unsafe extern "system" fn initialize(
    this: *mut c_void,
    context: *mut u8,
    flags: i32,
    device_id: *mut u16,
    full_name: *mut u16,
    sti: *mut c_void,
    _outer: *mut c_void,
    root: *mut *mut c_void,
    inner: *mut *mut c_void,
    error: *mut i32,
) -> i32 {
    // SAFETY: caller supplies our live interface, real service context, BSTRs
    // and writable outputs. The context is never fabricated or dereferenced here.
    let result = super::catch_hresult(|| unsafe {
        if !root.is_null() {
            *root = ptr::null_mut();
        }
        if !inner.is_null() {
            *inner = ptr::null_mut();
        }
        if error.is_null() {
            return E_POINTER;
        }
        if this.is_null() || context.is_null() || root.is_null() || flags != 0 {
            return report(error, E_INVALIDARG);
        }
        let result = (|| {
            let device = read_bstr(device_id)?;
            let name = read_bstr(full_name)?;
            (*this.cast::<Interface>()).initialize_tree(this, device, name, sti)
        })();
        match result {
            Ok(item) => {
                *root = item;
                report(error, 0)
            }
            Err(hr) => report(error, hr),
        }
    });
    // SAFETY: report checks null and writes the caller's error output, including
    // E_UNEXPECTED if a Rust panic was contained by catch_hresult.
    unsafe { report(error, result) }
}

unsafe fn read_bstr(value: *mut u16) -> Result<Vec<u16>, i32> {
    if value.is_null() {
        return Err(E_INVALIDARG);
    }
    // SAFETY: non-null input is a live BSTR for the duration of the COM call.
    let count = unsafe { SysStringLen(value) } as usize;
    if count == 0 || count > 16 * 1024 {
        return Err(E_INVALIDARG);
    }
    // SAFETY: allocator length covers count UTF-16 units; copy before returning.
    let words = unsafe { std::slice::from_raw_parts(value, count) };
    if words.contains(&0) {
        return Err(E_INVALIDARG);
    }
    Ok(words.to_vec())
}
#[link(name = "OleAut32")]
unsafe extern "system" {
    fn SysStringLen(value: *mut u16) -> u32;
}
unsafe extern "system" fn unsupported_item(
    _this: *mut c_void,
    _context: *mut u8,
    _flags: i32,
    error: *mut i32,
) -> i32 {
    // SAFETY: error is the method's writable output, checked by report.
    unsafe { report(error, E_NOTIMPL) }
}
unsafe extern "system" fn unsupported_properties(
    _this: *mut c_void,
    _context: *mut u8,
    _flags: i32,
    _count: u32,
    _properties: *const c_void,
    error: *mut i32,
) -> i32 {
    // SAFETY: error is the method's writable output, checked by report.
    unsafe { report(error, E_NOTIMPL) }
}
unsafe extern "system" fn unsupported_transfer(
    _this: *mut c_void,
    _context: *mut u8,
    _flags: i32,
    _transfer: *mut c_void,
    error: *mut i32,
) -> i32 {
    // SAFETY: error is the method's writable output, checked by report.
    unsafe { report(error, E_NOTIMPL) }
}
unsafe extern "system" fn error_string(
    _this: *mut c_void,
    _flags: i32,
    _code: i32,
    text: *mut *mut u16,
    error: *mut i32,
) -> i32 {
    // SAFETY: optional writable string output belongs to caller.
    unsafe {
        if !text.is_null() {
            *text = ptr::null_mut();
        }
        report(error, E_NOTIMPL)
    }
}
unsafe extern "system" fn command(
    _this: *mut c_void,
    _context: *mut u8,
    _flags: i32,
    _command: *const Guid,
    item: *mut *mut c_void,
    error: *mut i32,
) -> i32 {
    // SAFETY: optional writable item output belongs to caller.
    unsafe {
        if !item.is_null() {
            *item = ptr::null_mut();
        }
        report(error, E_NOTIMPL)
    }
}
unsafe extern "system" fn unsupported_list(
    _this: *mut c_void,
    _context: *mut u8,
    _flags: i32,
    count: *mut i32,
    list: *mut *mut c_void,
    error: *mut i32,
) -> i32 {
    // SAFETY: clear caller's valid outputs before reporting unsupported work.
    unsafe {
        if !count.is_null() {
            *count = 0;
        }
        if !list.is_null() {
            *list = ptr::null_mut();
        }
        report(error, E_NOTIMPL)
    }
}
unsafe extern "system" fn free_context(
    _this: *mut c_void,
    flags: i32,
    _context: *mut u8,
    error: *mut i32,
) -> i32 {
    // SAFETY: context contains no driver-owned allocation. Windows owns the block
    // itself; this notification must not free the service's context storage.
    unsafe { report(error, if flags == 0 { 0 } else { E_INVALIDARG }) }
}
unsafe extern "system" fn uninitialize(this: *mut c_void, context: *mut u8) -> i32 {
    if this.is_null() || context.is_null() {
        return E_INVALIDARG;
    }
    // SAFETY: caller retains our native interface throughout cleanup callbacks.
    super::catch_hresult(|| unsafe { (*this.cast::<Interface>()).disconnect_client() })
}

static VTABLE: Vtable = Vtable {
    query,
    add_ref,
    release,
    initialize,
    acquire: acquire::entry,
    init_properties: properties::init_entry,
    validate_properties: properties::validate_entry,
    write_properties: unsupported_transfer,
    read_properties: unsupported_properties,
    lock: locking::lock,
    unlock: locking::unlock,
    analyze: unsupported_item,
    error_string,
    command,
    capabilities: unsupported_list,
    delete: unsupported_item,
    free_context,
    formats: formats::entry,
    notify: cancel::notify,
    uninitialize,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    // Synthetic STI helper IUnknown prefix. It reenters only the driver's
    // lifecycle coordinator, never fabricates a WIA application context.
    #[repr(C)]
    struct ReentrantHelper {
        vtable: *const TestDeviceTable,
        interface: *const Interface,
        refs: Cell<u32>,
        entries: Cell<u32>,
        lock_result: Cell<i32>,
        unlock_result: Cell<i32>,
        lock_calls: Cell<u32>,
        unlock_calls: Cell<u32>,
    }
    unsafe extern "system" fn helper_query(
        _: *mut c_void,
        _: *const Guid,
        output: *mut *mut c_void,
    ) -> i32 {
        if output.is_null() {
            return E_POINTER;
        }
        // SAFETY: valid caller output; no interface is provided by this test path.
        unsafe {
            *output = ptr::null_mut();
        }
        super::super::E_NOINTERFACE
    }
    unsafe extern "system" fn helper_add(raw: *mut c_void) -> u32 {
        // SAFETY: test keeps both helper and driver alive; synchronous reentry
        // must observe Busy without a lifecycle mutex being held.
        unsafe {
            let helper = &*raw.cast::<ReentrantHelper>();
            assert_eq!((*helper.interface).disconnect_client(), WIA_ERROR_BUSY);
            helper.entries.set(helper.entries.get() + 1);
            helper.refs.set(helper.refs.get() + 1);
            helper.refs.get()
        }
    }
    unsafe extern "system" fn helper_release(raw: *mut c_void) -> u32 {
        // SAFETY: test retains helper storage until the connection releases it.
        unsafe {
            let helper = &*raw.cast::<ReentrantHelper>();
            assert_eq!((*helper.interface).disconnect_client(), WIA_ERROR_BUSY);
            helper.entries.set(helper.entries.get() + 1);
            helper.refs.set(helper.refs.get() - 1);
            helper.refs.get()
        }
    }
    #[repr(C)]
    struct TestDeviceTable {
        unknown: UnknownVtable,
        unused: [usize; 7],
        lock: unsafe extern "system" fn(*mut c_void, u32) -> i32,
        unlock: unsafe extern "system" fn(*mut c_void) -> i32,
    }
    unsafe extern "system" fn helper_lock(raw: *mut c_void, timeout: u32) -> i32 {
        // SAFETY: fixture storage and driver are held throughout synchronous calls.
        unsafe {
            let helper = &*raw.cast::<ReentrantHelper>();
            assert!(timeout > 0 && timeout <= 5000);
            assert_eq!((*helper.interface).disconnect_client(), WIA_ERROR_BUSY);
            assert_eq!(locking::dispatch(&*helper.interface, true), WIA_ERROR_BUSY);
            helper.lock_calls.set(helper.lock_calls.get() + 1);
            helper.lock_result.get()
        }
    }
    unsafe extern "system" fn helper_unlock(raw: *mut c_void) -> i32 {
        // SAFETY: fixture is live, including synchronous driver reentry.
        unsafe {
            let helper = &*raw.cast::<ReentrantHelper>();
            assert_eq!((*helper.interface).disconnect_client(), WIA_ERROR_BUSY);
            assert_eq!(locking::dispatch(&*helper.interface, false), WIA_ERROR_BUSY);
            helper.unlock_calls.set(helper.unlock_calls.get() + 1);
            helper.unlock_result.get()
        }
    }
    static HELPER_VTABLE: TestDeviceTable = TestDeviceTable {
        unknown: UnknownVtable {
            query: helper_query,
            add_ref: helper_add,
            release: helper_release,
        },
        unused: [0; 7],
        lock: helper_lock,
        unlock: helper_unlock,
    };
    #[test]
    fn native_flatbed_tree_names_flags_and_cleanup() {
        // Independently compiled C11 assertions against SDK 10.0.26100.0
        // establish these native x64 sizes and method offsets.
        assert_eq!(std::mem::size_of::<Vtable>(), 160);
        assert_eq!(offset_of!(Vtable, initialize), 24);
        assert_eq!(offset_of!(Vtable, acquire), 32);
        assert_eq!(offset_of!(Vtable, free_context), 128);
        assert_eq!(offset_of!(Vtable, uninitialize), 152);
        // This is a synthetic item identity, never a registered WIA device or
        // fabricated WIA service context. The item objects are Windows objects.
        let mut class = ptr::null_mut();
        let mut mini = ptr::null_mut();
        // SAFETY: owned COM references, valid outputs, and real SDK item creation.
        unsafe {
            assert_eq!(CoInitializeEx(ptr::null_mut(), 0), 0);
            assert_eq!(
                super::super::DllGetClassObject(
                    &super::super::DRIVER_CLASS_ID,
                    &super::super::IID_ICLASSFACTORY,
                    &mut class
                ),
                0
            );
            let factory = &**class.cast::<*const super::super::ClassFactoryVtable>();
            assert_eq!(
                (factory.create_instance)(
                    class,
                    ptr::null_mut(),
                    &super::super::IID_IWIAMINIDRV,
                    &mut mini
                ),
                0
            );
            (factory.release)(class);
            let before = (*owner(mini).cast::<Instance>())
                .refs
                .load(std::sync::atomic::Ordering::SeqCst);
            for _ in 0..3 {
                let tree =
                    tree::Tree::create(mini, &"synthetic\\Root".encode_utf16().collect::<Vec<_>>())
                        .unwrap();
                tree.verify_flatbed().unwrap();
                tree.close().unwrap();
            }
            let interface = &*mini.cast::<Interface>();
            let device: Vec<_> = "synthetic-device".encode_utf16().collect();
            let name: Vec<_> = "synthetic\\Root".encode_utf16().collect();
            let mut helper = ReentrantHelper {
                vtable: &HELPER_VTABLE,
                interface,
                refs: Cell::new(1),
                entries: Cell::new(0),
                lock_result: Cell::new(0),
                unlock_result: Cell::new(0),
                lock_calls: Cell::new(0),
                unlock_calls: Cell::new(0),
            };
            let helper_raw = ptr::from_mut(&mut helper).cast();
            // Internal initialization seam takes actual names and mini interface,
            // without inventing a WIA application context to call the service API.
            assert_eq!(
                interface
                    .initialize_tree(mini, vec![], name.clone(), ptr::null_mut())
                    .unwrap_err(),
                E_INVALIDARG
            );
            assert_eq!(
                interface
                    .initialize_tree(mini, device.clone(), vec![65; 16 * 1024], ptr::null_mut())
                    .unwrap_err(),
                E_INVALIDARG
            );
            for _ in 0..3 {
                let root = interface
                    .initialize_tree(mini, device.clone(), name.clone(), helper_raw)
                    .unwrap();
                let shared = interface
                    .initialize_tree(mini, device.clone(), name.clone(), ptr::null_mut())
                    .unwrap();
                assert_eq!(root, shared);
                assert_eq!(helper.refs.get(), 2);
                // Synthetic property/result producers exercise native lifecycle
                // ordering without fabricating a service property context.
                assert_eq!(
                    acquire::dispatch(
                        interface,
                        || Err(E_INVALIDARG),
                        |_, _| { panic!("failed properties must not start a transfer") }
                    ),
                    E_INVALIDARG
                );
                for cancellation_case in 0..3 {
                    assert_eq!(
                        acquire::dispatch(
                            interface,
                            || {
                                assert_eq!(interface.disconnect_client(), WIA_ERROR_BUSY);
                                assert_eq!(locking::dispatch(interface, false), WIA_ERROR_BUSY);
                                Ok(properties::Snapshot {
                                    settings: crate::wia::FlatbedSettings {
                                        x_resolution: 75,
                                        y_resolution: 75,
                                        x_position: 0,
                                        y_position: 0,
                                        x_extent: 600,
                                        y_extent: 800,
                                        data_type: 2,
                                        depth: 8,
                                        brightness: 0,
                                        contrast: 0,
                                        compression: 0,
                                        format: crate::wia::BMP_FORMAT,
                                    },
                                    item: "Flatbed".into(),
                                    full_item: "synthetic\\Root\\Flatbed".into(),
                                })
                            },
                            |snapshot, cancel| {
                                assert!(!cancel.load(std::sync::atomic::Ordering::Relaxed));
                                assert_eq!(snapshot.settings.x_resolution, 75);
                                assert_eq!(snapshot.item, "Flatbed");
                                assert_eq!(interface.disconnect_client(), WIA_ERROR_BUSY);
                                assert_eq!(locking::dispatch(interface, true), WIA_ERROR_BUSY);
                                if cancellation_case != 0 {
                                    assert!(interface.cancellation.cancel(&device));
                                    assert!(cancel.load(std::sync::atomic::Ordering::Relaxed));
                                }
                                match cancellation_case {
                                    0 => Ok(crate::wia_transfer::TransferOutcome::Cancelled),
                                    1 => Ok(crate::wia_transfer::TransferOutcome::Completed(
                                        crate::scan::ScanSummary {
                                            width: 600,
                                            height: 800,
                                            bands: 1,
                                            bytes: 480_000,
                                        },
                                    )),
                                    _ => Err(std::io::Error::from_raw_os_error(5)),
                                }
                            }
                        ),
                        if cancellation_case == 2 {
                            0x80070005u32 as i32
                        } else {
                            1
                        }
                    );
                    assert!(!interface.cancellation.cancel(&device));
                }
                assert!(matches!(*interface.state(), Lifecycle::Connected(_)));
                for result in [WIA_ERROR_BUSY, 0x80070005u32 as i32, 1, 0] {
                    helper.lock_result.set(result);
                    // Unlock failures quarantine the connection and are covered
                    // by locking's dedicated failure/lifetime tests.
                    helper.unlock_result.set(0);
                    let expected = if result > 0 { E_UNEXPECTED } else { result };
                    assert_eq!(locking::dispatch(interface, true), expected);
                    assert_eq!(locking::dispatch(interface, false), 0);
                    assert!(matches!(*interface.state(), Lifecycle::Connected(_)));
                    assert_eq!(helper.refs.get(), 2);
                }
                assert_eq!(
                    interface
                        .initialize_tree(mini, vec![88], name.clone(), ptr::null_mut())
                        .unwrap_err(),
                    E_INVALIDARG
                );
                assert_eq!(interface.disconnect_client(), 0);
                assert_eq!(helper.refs.get(), 2);
                let state = interface.state();
                let Lifecycle::Connected(connection) = &*state else {
                    panic!("first disconnect must preserve second client");
                };
                connection.tree.verify_flatbed().unwrap();
                drop(state);
                assert_eq!(interface.disconnect_client(), 0);
                assert!(matches!(*interface.state(), Lifecycle::Idle));
                assert_eq!(helper.refs.get(), 1);
                assert_eq!(interface.disconnect_client(), E_UNEXPECTED);
            }
            assert_eq!(helper.entries.get(), 6);
            assert_eq!(helper.lock_calls.get(), 12);
            assert_eq!(helper.unlock_calls.get(), 12);
            assert_eq!(locking::dispatch(interface, true), E_UNEXPECTED);
            interface
                .initialize_tree(mini, device, name, ptr::null_mut())
                .unwrap();
            assert_eq!(locking::dispatch(interface, true), E_UNEXPECTED);
            assert_eq!(locking::dispatch(interface, false), E_UNEXPECTED);
            assert!(matches!(*interface.state(), Lifecycle::Failed));
            assert_eq!(interface.disconnect_client(), E_UNEXPECTED);
            assert_eq!(
                (*owner(mini).cast::<Instance>())
                    .refs
                    .load(std::sync::atomic::Ordering::SeqCst),
                before
            );
            assert_eq!(release(mini), 0);
            CoUninitialize();
        }
    }
    #[link(name = "Ole32")]
    unsafe extern "system" {
        fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
        fn CoUninitialize();
    }
}
