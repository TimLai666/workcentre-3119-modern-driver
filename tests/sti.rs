//! Direct SDK IStiUSD contracts. Only the explicitly ignored hardware test opens USB.
#![cfg(windows)]

use std::{
    ffi::c_void,
    ptr,
    sync::atomic::{AtomicI32, AtomicU32, Ordering},
};
use workcentre_3119::com_server::{DRIVER_CLASS_ID, DllGetClassObject, Guid};

#[allow(dead_code)]
#[path = "support/wia_callback.rs"]
mod transfer_fixture;

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
const VERSION: u32 = 0x01000002;
const E_NOTIMPL: i32 = 0x80004001u32 as i32;

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
#[repr(C)]
struct Caps {
    version: u32,
    flags: u32,
}
#[repr(C)]
struct ErrorInfo {
    size: u32,
    generic: u32,
    vendor: u32,
    text: [u16; 255],
}
#[repr(C)]
struct Diagnostic {
    size: u32,
    basic: u32,
    vendor: u32,
    status: u32,
    error: ErrorInfo,
}
fn presence_request() -> Diagnostic {
    Diagnostic {
        size: std::mem::size_of::<Diagnostic>() as u32,
        basic: 1,
        vendor: 0,
        status: u32::MAX,
        error: ErrorInfo {
            size: std::mem::size_of::<ErrorInfo>() as u32,
            generic: u32::MAX,
            vendor: u32::MAX,
            text: [0xffff; 255],
        },
    }
}
#[repr(C)]
struct StiTable {
    unknown: UnknownTable,
    initialize: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut c_void) -> i32,
    capabilities: unsafe extern "system" fn(*mut c_void, *mut Caps) -> i32,
    status: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    reset: unsafe extern "system" fn(*mut c_void) -> i32,
    diagnostic: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    escape: unsafe extern "system" fn(
        *mut c_void,
        u32,
        *mut c_void,
        u32,
        *mut c_void,
        u32,
        *mut u32,
    ) -> i32,
    last_error: unsafe extern "system" fn(*mut c_void, *mut u32) -> i32,
    lock: unsafe extern "system" fn(*mut c_void) -> i32,
    unlock: unsafe extern "system" fn(*mut c_void) -> i32,
    read: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut u32, *mut c_void) -> i32,
    write: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut c_void) -> i32,
    read_command: unsafe extern "system" fn(*mut c_void, *mut c_void, *mut u32, *mut c_void) -> i32,
    write_command: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut c_void) -> i32,
    notification_handle: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    notification_data: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
    error_info: unsafe extern "system" fn(*mut c_void, *mut c_void) -> i32,
}

struct Owned(*mut c_void);
impl Drop for Owned {
    fn drop(&mut self) {
        // SAFETY: this guard owns one valid COM reference with an IUnknown prefix.
        unsafe {
            ((**self.0.cast::<*const UnknownTable>()).release)(self.0);
        }
    }
}
fn object() -> Owned {
    let mut raw = ptr::null_mut();
    assert_eq!(
        // SAFETY: live GUID and writable output, returned reference is immediately guarded.
        unsafe { DllGetClassObject(&DRIVER_CLASS_ID, &FACTORY, &mut raw) },
        0
    );
    let factory = Owned(raw);
    let mut output = ptr::null_mut();
    assert_eq!(
        // SAFETY: live factory with the independent SDK ABI declaration.
        unsafe {
            ((**factory.0.cast::<*const FactoryTable>()).create)(
                factory.0,
                ptr::null_mut(),
                &STI,
                &mut output,
            )
        },
        0
    );
    assert!(!output.is_null());
    Owned(output)
}
unsafe fn methods(object: &Owned) -> &StiTable {
    // SAFETY: object() only returns successful IID_IStiUSD references.
    unsafe { &**object.0.cast::<*const StiTable>() }
}

// IStiDeviceControl::GetMyDevicePortName is SDK slot 10, after GetLastError.
#[repr(C)]
struct ControlTable {
    unknown: UnknownTable,
    unused: [usize; 7],
    port: unsafe extern "system" fn(*mut c_void, *mut u16, u32) -> i32,
    remaining: [usize; 3],
}
#[repr(C)]
struct Helper {
    table: *const ControlTable,
    refs: AtomicU32,
    result: AtomicI32,
    port: Vec<u16>,
}
impl Helper {
    fn new() -> Self {
        Self {
            table: &CONTROL,
            refs: AtomicU32::new(1),
            result: AtomicI32::new(0),
            port:
                r"\\?\usb#vid_0924&pid_4265&mi_00#synthetic#{c4147e4a-9c41-4846-a53c-5e625c68021a}"
                    .encode_utf16()
                    .chain([0])
                    .collect(),
        }
    }
    fn raw(&mut self) -> *mut c_void {
        ptr::from_mut(self).cast()
    }
}
unsafe extern "system" fn helper_query(
    _: *mut c_void,
    _: *const Guid,
    out: *mut *mut c_void,
) -> i32 {
    if !out.is_null() {
        // SAFETY: COM caller provides writable pointer storage.
        unsafe {
            *out = ptr::null_mut();
        }
    }
    0x80004002u32 as i32
}
unsafe extern "system" fn helper_add(this: *mut c_void) -> u32 {
    // SAFETY: helper is stack-owned by the test until the driver releases it.
    unsafe {
        (&*this.cast::<Helper>())
            .refs
            .fetch_add(1, Ordering::SeqCst)
            + 1
    }
}
unsafe extern "system" fn helper_release(this: *mut c_void) -> u32 {
    // SAFETY: a counted helper reference is being released; the test owns its allocation.
    unsafe {
        (&*this.cast::<Helper>())
            .refs
            .fetch_sub(1, Ordering::SeqCst)
            - 1
    }
}
unsafe extern "system" fn helper_port(this: *mut c_void, out: *mut u16, count: u32) -> i32 {
    // SAFETY: test helper remains live during the synchronous call.
    let helper = unsafe { &*this.cast::<Helper>() };
    let result = helper.result.load(Ordering::SeqCst);
    if result != 0 {
        return result;
    }
    if out.is_null() || (count as usize) < helper.port.len() {
        return 0x8007007au32 as i32;
    }
    // SAFETY: capacity was checked; source and destination do not overlap.
    unsafe {
        ptr::copy_nonoverlapping(helper.port.as_ptr(), out, helper.port.len());
    }
    0
}
static CONTROL: ControlTable = ControlTable {
    unknown: UnknownTable {
        query: helper_query,
        add: helper_add,
        release: helper_release,
    },
    unused: [0; 7],
    port: helper_port,
    remaining: [0; 3],
};

#[test]
fn sti_identity_initialization_and_helper_ownership() {
    let mut helper = Helper::new();
    let device = object();
    // SAFETY: live driver/helper, borrowed key is intentionally never used by this interface.
    unsafe {
        let m = methods(&device);
        let mut alias = ptr::null_mut();
        assert_eq!((m.unknown.query)(device.0, &UNKNOWN, &mut alias), 0);
        assert_eq!(alias, device.0);
        drop(Owned(alias));
        assert!((m.initialize)(device.0, ptr::null_mut(), VERSION, ptr::dangling_mut()) < 0);
        assert!((m.initialize)(device.0, helper.raw(), 1, ptr::dangling_mut()) < 0);
        assert_eq!(helper.refs.load(Ordering::SeqCst), 1);
        assert_eq!(
            (m.initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
        assert_eq!(helper.refs.load(Ordering::SeqCst), 2);
        assert_eq!(
            (m.initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0x800704dfu32 as i32
        );
        assert_eq!(helper.refs.load(Ordering::SeqCst), 2);
        let mut caps = Caps {
            version: 0,
            flags: u32::MAX,
        };
        assert_eq!((m.capabilities)(device.0, &mut caps), 0);
        assert_eq!(caps.version, VERSION);
        assert_eq!(
            caps.flags, 0,
            "do not advertise IWiaMiniDrv or notifications before implementation"
        );
    }
    drop(device);
    assert_eq!(helper.refs.load(Ordering::SeqCst), 1);
}

#[test]
fn uninitialized_lock_and_unlock_return_without_deadlock() {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let device = object();
        // SAFETY: thread owns the live object; uninitialized calls must not open USB.
        let result = unsafe {
            let m = methods(&device);
            ((m.lock)(device.0), (m.unlock)(device.0))
        };
        send.send(result).unwrap();
    });
    let (lock, unlock) = receive
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("uninitialized lock/unlock must not deadlock");
    assert!(lock < 0);
    assert!(unlock < 0);
}

fn scan_settings() -> workcentre_3119::wia::FlatbedSettings {
    workcentre_3119::wia::FlatbedSettings {
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
        format: workcentre_3119::wia::BMP_FORMAT,
    }
}

#[test]
fn locked_scan_rejects_unlocked_and_precancel_before_touching_output() {
    use std::{
        io::{self, Cursor},
        sync::atomic::AtomicBool,
    };
    use workcentre_3119::com_server::scan_locked_bmp;
    let mut helper = Helper::new();
    let device = object();
    let mut output = Cursor::new(Vec::new());
    let cancel = AtomicBool::new(false);
    // SAFETY: live IStiUSD from this factory; helper and destination outlive the call.
    unsafe {
        let m = methods(&device);
        assert_eq!(
            (m.initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
        assert!(scan_locked_bmp(device.0, scan_settings(), &cancel, &mut output).is_err());
        assert!(output.get_ref().is_empty());
        cancel.store(true, Ordering::SeqCst);
        assert_eq!(
            scan_locked_bmp(device.0, scan_settings(), &cancel, &mut output)
                .unwrap_err()
                .kind(),
            io::ErrorKind::Interrupted
        );
        assert!(output.get_ref().is_empty());
    }
}

#[test]
fn callback_transfer_checks_settings_and_lock_before_querying_callback() {
    use std::{io, sync::atomic::AtomicBool};
    use workcentre_3119::{com_server::transfer_locked_bmp, wia_transfer::TransferOutcome};
    let device = object();
    let mut helper = Helper::new();
    let mut settings = scan_settings();
    settings.x_extent = 0;
    // SAFETY: live owned driver; null callback must not be accessed by preflight.
    unsafe {
        assert_eq!(
            transfer_locked_bmp(
                device.0,
                settings,
                &AtomicBool::new(false),
                ptr::null_mut(),
                "Flatbed",
                "Root\\Flatbed"
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::InvalidInput
        );
        assert!(matches!(
            transfer_locked_bmp(
                device.0,
                scan_settings(),
                &AtomicBool::new(true),
                ptr::null_mut(),
                "Flatbed",
                "Root\\Flatbed"
            )
            .unwrap(),
            TransferOutcome::Cancelled
        ));
        assert_eq!(
            transfer_locked_bmp(
                device.0,
                scan_settings(),
                &AtomicBool::new(false),
                ptr::null_mut(),
                "Flatbed",
                "Root\\Flatbed"
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotConnected
        );
        assert_eq!(
            (methods(&device).initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
        assert_eq!(
            transfer_locked_bmp(
                device.0,
                scan_settings(),
                &AtomicBool::new(false),
                ptr::null_mut(),
                "Flatbed",
                "Root\\Flatbed"
            )
            .unwrap_err()
            .kind(),
            io::ErrorKind::NotConnected
        );
    }
    drop(device);
    assert_eq!(helper.refs.load(Ordering::SeqCst), 1);
}

#[test]
fn invalid_status_size_does_not_overwrite_output() {
    let mut helper = Helper::new();
    let device = object();
    // Full SDK-size allocation with a deliberately short declared length.
    let mut status = [u32::MAX; 6];
    status[0] = 4;
    // SAFETY: live object, helper and full aligned STI_DEVICE_STATUS buffer.
    unsafe {
        let m = methods(&device);
        assert_eq!(
            (m.initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
        assert!((m.status)(device.0, status.as_mut_ptr().cast()) < 0);
    }
    assert_eq!(
        status,
        [4, u32::MAX, u32::MAX, u32::MAX, u32::MAX, u32::MAX]
    );
}

#[test]
fn failed_helper_initialization_can_retry_without_leaking() {
    let mut helper = Helper::new();
    let device = object();
    helper.result.store(0x80070005u32 as i32, Ordering::SeqCst);
    // SAFETY: all inputs remain live, helper returns a synthetic HRESULT without USB.
    unsafe {
        let m = methods(&device);
        assert_eq!(
            (m.initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0x80070005u32 as i32
        );
        assert_eq!(helper.refs.load(Ordering::SeqCst), 1);
        helper.result.store(0, Ordering::SeqCst);
        assert_eq!(
            (m.initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
    }
    drop(device);
    assert_eq!(helper.refs.load(Ordering::SeqCst), 1);
}

#[test]
fn unsupported_raw_and_reset_do_not_claim_device_work() {
    let mut helper = Helper::new();
    let device = object();
    // SAFETY: initialized driver, valid bounded buffers; none of these methods may touch USB.
    unsafe {
        let m = methods(&device);
        assert_eq!(
            (m.initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
        assert_eq!((m.reset)(device.0), E_NOTIMPL);
        let mut error = 0;
        assert_eq!((m.last_error)(device.0, &mut error), 0);
        assert_eq!(error, E_NOTIMPL as u32);
        let mut buffer = [0x5au8; 8];
        let mut bytes = 8;
        assert_eq!(
            (m.read)(
                device.0,
                buffer.as_mut_ptr().cast(),
                &mut bytes,
                ptr::null_mut()
            ),
            E_NOTIMPL
        );
        assert_eq!(bytes, 0);
        assert_eq!(buffer, [0x5a; 8]);
        assert_eq!(
            (m.write)(device.0, buffer.as_mut_ptr().cast(), 8, ptr::null_mut()),
            E_NOTIMPL
        );
        let mut actual = 8;
        assert_eq!(
            (m.escape)(
                device.0,
                123,
                ptr::null_mut(),
                0,
                buffer.as_mut_ptr().cast(),
                8,
                &mut actual
            ),
            E_NOTIMPL
        );
        assert_eq!(actual, 0);
        assert_eq!(buffer, [0x5a; 8]);
    }
}

#[test]
fn invalid_diagnostic_and_error_details_are_bounded() {
    let mut helper = Helper::new();
    let device = object();
    assert_eq!(std::mem::size_of::<ErrorInfo>(), 524);
    assert_eq!(std::mem::size_of::<Diagnostic>(), 540);
    // SAFETY: all buffers have the full SDK layout. Rejected requests must not open USB.
    unsafe {
        let m = methods(&device);
        assert_eq!(
            (m.initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
        let mut request = presence_request();
        request.size = 4;
        assert!((m.diagnostic)(device.0, ptr::from_mut(&mut request).cast()) < 0);
        assert_eq!(
            request.error.generic,
            u32::MAX,
            "a short declared structure must not receive nested output"
        );
        let mut request = presence_request();
        request.basic = 99;
        assert!((m.diagnostic)(device.0, ptr::from_mut(&mut request).cast()) < 0);
        assert_ne!(request.error.generic, 0);
        let mut info = presence_request().error;
        assert_eq!((m.error_info)(device.0, ptr::from_mut(&mut info).cast()), 0);
        let mut code = 0;
        assert_eq!((m.last_error)(device.0, &mut code), 0);
        assert_eq!(info.generic, code);
        assert!(info.text.contains(&0));
    }
}

#[test]
#[ignore = "Hardware: set WC3119_TEST_STI_PATH to a freshly enumerated MI_00 WinUSB path; sends INQUIRY and holds the device exclusively"]
fn actual_sti_device_lock_presence_and_release() {
    use std::os::windows::ffi::OsStrExt;
    let path = std::env::var_os("WC3119_TEST_STI_PATH").expect("set WC3119_TEST_STI_PATH");
    let mut helper = Helper::new();
    helper.port = path.encode_wide().chain([0]).collect();
    let first = object();
    let second = object();
    let mut missing_helper = Helper::new();
    let missing = object();
    // SAFETY: both drivers and helper remain live. This explicitly opted-in test accesses USB.
    unsafe {
        let a = methods(&first);
        let b = methods(&second);
        let absent = methods(&missing);
        assert_eq!(
            (absent.initialize)(
                missing.0,
                missing_helper.raw(),
                VERSION,
                ptr::dangling_mut()
            ),
            0
        );
        assert!(
            (absent.lock)(missing.0) < 0,
            "a missing target must not fall back to the connected scanner"
        );
        let mut diagnostic = presence_request();
        assert!((absent.diagnostic)(missing.0, ptr::from_mut(&mut diagnostic).cast()) < 0);
        assert_ne!(diagnostic.error.generic, 0);
        assert_eq!(
            (a.initialize)(first.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
        assert_eq!(
            (b.initialize)(second.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
        assert_eq!((a.lock)(first.0), 0);
        assert!(
            (b.lock)(second.0) < 0,
            "second object must not acquire the same USB device"
        );
        let mut diagnostic = presence_request();
        assert_eq!(
            (a.diagnostic)(first.0, ptr::from_mut(&mut diagnostic).cast()),
            0
        );
        assert_eq!(diagnostic.error.generic, 0);
        assert_eq!((a.unlock)(first.0), 0);
        assert_eq!((b.lock)(second.0), 0);
        let mut diagnostic = presence_request();
        assert_eq!(
            (b.diagnostic)(second.0, ptr::from_mut(&mut diagnostic).cast()),
            0
        );
        assert_eq!(diagnostic.error.generic, 0);
        drop(second); // Release must close an owned USB session even without explicit Unlock.
        assert_eq!((a.lock)(first.0), 0);
        assert_eq!((a.unlock)(first.0), 0);
    }
    drop(first);
    assert_eq!(helper.refs.load(Ordering::SeqCst), 1);
}

#[test]
#[ignore = "Hardware scan: WC3119_TEST_STI_PATH must be fresh; WC3119_TEST_OUTPUT_DIR must not exist. Performs gray/RGB75 and cancellation through one locked object"]
fn actual_locked_object_scans_cancels_and_rescans_with_reentrant_output() {
    use std::{
        io::{self, Cursor, Seek, SeekFrom, Write},
        os::windows::ffi::OsStrExt,
        sync::atomic::AtomicBool,
    };
    use workcentre_3119::com_server::scan_locked_bmp;
    struct Output<'a> {
        bytes: Cursor<Vec<u8>>,
        device: &'a Owned,
        cancel: &'a AtomicBool,
        cancel_on_write: bool,
        checked: bool,
    }
    impl Write for Output<'_> {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if !self.checked {
                self.checked = true;
                // SAFETY: caller owns the COM reference throughout this synchronous callback.
                unsafe {
                    let m = methods(self.device);
                    let mut error = 0;
                    assert_eq!((m.last_error)(self.device.0, &mut error), 0);
                    assert!(
                        (m.unlock)(self.device.0) < 0,
                        "must not unlock an active scan"
                    );
                    let mut nested = Cursor::new(Vec::new());
                    let result = scan_locked_bmp(
                        self.device.0,
                        scan_settings(),
                        &AtomicBool::new(false),
                        &mut nested,
                    );
                    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::WouldBlock);
                    assert!(nested.get_ref().is_empty());
                }
                if self.cancel_on_write {
                    self.cancel.store(true, Ordering::SeqCst);
                }
            }
            self.bytes.write(bytes)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    impl Seek for Output<'_> {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.bytes.seek(position)
        }
    }
    let directory = std::path::PathBuf::from(
        std::env::var_os("WC3119_TEST_OUTPUT_DIR").expect("set WC3119_TEST_OUTPUT_DIR"),
    );
    std::fs::create_dir(&directory).expect("output directory must be new");
    let path = std::env::var_os("WC3119_TEST_STI_PATH").expect("set WC3119_TEST_STI_PATH");
    let mut helper = Helper::new();
    helper.port = path.encode_wide().chain([0]).collect();
    let device = object();
    // SAFETY: helper and device are live and owned until every synchronous call ends.
    unsafe {
        let m = methods(&device);
        assert_eq!(
            (m.initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
        assert_eq!((m.lock)(device.0), 0);
        for (name, color, cancelled) in [
            ("gray75.bmp", false, false),
            ("cancelled-rgb75.partial", true, true),
            ("rgb75-rescan.bmp", true, false),
        ] {
            let settings = if color {
                workcentre_3119::wia::FlatbedSettings {
                    data_type: 3,
                    depth: 24,
                    ..scan_settings()
                }
            } else {
                scan_settings()
            };
            let cancel = AtomicBool::new(false);
            let mut output = Output {
                bytes: Cursor::new(Vec::new()),
                device: &device,
                cancel: &cancel,
                cancel_on_write: cancelled,
                checked: false,
            };
            let result = scan_locked_bmp(device.0, settings, &cancel, &mut output);
            assert!(output.checked, "the image callback must have run");
            if cancelled {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
                assert!(!output.bytes.get_ref().starts_with(b"BM"));
            } else {
                let summary = result.expect("locked scan must complete");
                let bmp = output.bytes.get_ref();
                assert!(bmp.starts_with(b"BM"));
                assert_eq!(
                    u32::from_le_bytes(bmp[2..6].try_into().unwrap()) as usize,
                    bmp.len()
                );
                assert_eq!(
                    u32::from_le_bytes(bmp[18..22].try_into().unwrap()),
                    summary.width
                );
                assert_eq!(
                    i32::from_le_bytes(bmp[22..26].try_into().unwrap()),
                    -(summary.height as i32)
                );
                println!(
                    "{name}: {}x{}, {} bytes, {} bands",
                    summary.width, summary.height, summary.bytes, summary.bands
                );
            }
            let mut file = std::fs::File::create_new(directory.join(name)).unwrap();
            file.write_all(output.bytes.get_ref()).unwrap();
            file.sync_all().unwrap();
            let mut diagnostic = presence_request();
            assert_eq!(
                (m.diagnostic)(device.0, ptr::from_mut(&mut diagnostic).cast()),
                0,
                "same session must remain usable after cleanup"
            );
        }
        assert_eq!((m.unlock)(device.0), 0);
    }
    drop(device);
    assert_eq!(helper.refs.load(Ordering::SeqCst), 1);
    std::fs::File::create_new(directory.join("complete.txt"))
        .unwrap()
        .sync_all()
        .unwrap();
}

#[test]
#[ignore = "Hardware scan: fresh WC3119_TEST_STI_PATH and new WC3119_TEST_OUTPUT_DIR required; native callback gray/RGB cancel/rescan"]
fn actual_callback_transfer_scans_cancels_and_rescans() {
    use std::{
        io::{Cursor, Write},
        os::windows::ffi::OsStrExt,
        sync::atomic::AtomicBool,
    };
    use transfer_fixture::{ComApartment, FakeTransferCallback, SendMessagePlan};
    use workcentre_3119::{
        com_server::{scan_locked_bmp, transfer_locked_bmp},
        wia_transfer::TransferOutcome,
    };
    let _com = ComApartment::new();
    let directory = std::path::PathBuf::from(
        std::env::var_os("WC3119_TEST_OUTPUT_DIR").expect("set WC3119_TEST_OUTPUT_DIR"),
    );
    std::fs::create_dir(&directory).expect("output directory must be new");
    let path = std::env::var_os("WC3119_TEST_STI_PATH").expect("set WC3119_TEST_STI_PATH");
    let mut helper = Helper::new();
    helper.port = path.encode_wide().chain([0]).collect();
    let device = object();
    // SAFETY: owned driver/helper and initialized apartment outlive all callbacks.
    unsafe {
        let m = methods(&device);
        assert_eq!(
            (m.initialize)(device.0, helper.raw(), VERSION, ptr::dangling_mut()),
            0
        );
        assert_eq!((m.lock)(device.0), 0);
        for (name, color, cancelled) in [
            ("gray75.bmp", false, false),
            ("cancelled-rgb75.partial", true, true),
            ("rgb75-rescan.bmp", true, false),
        ] {
            let mut fake = FakeTransferCallback::new();
            if cancelled {
                fake.set_send_plan(SendMessagePlan::CancelAt(2));
            }
            let raw_device = device.0;
            fake.set_hook(Box::new(move || {
                let table = &**raw_device.cast::<*const StiTable>();
                let mut last = 0;
                assert_eq!((table.last_error)(raw_device, &mut last), 0);
                assert!(
                    (table.unlock)(raw_device) < 0,
                    "callback must retain exclusive operation"
                );
                let mut nested = Cursor::new(Vec::new());
                let error = scan_locked_bmp(
                    raw_device,
                    scan_settings(),
                    &AtomicBool::new(false),
                    &mut nested,
                )
                .unwrap_err();
                assert_eq!(error.kind(), std::io::ErrorKind::WouldBlock);
                assert!(nested.get_ref().is_empty());
            }));
            let settings = if color {
                workcentre_3119::wia::FlatbedSettings {
                    data_type: 3,
                    depth: 24,
                    ..scan_settings()
                }
            } else {
                scan_settings()
            };
            let result = transfer_locked_bmp(
                device.0,
                settings,
                &AtomicBool::new(false),
                fake.as_raw(),
                "Flatbed",
                "Root\\Flatbed",
            );
            let bytes = fake.stream_bytes().expect("callback must create stream");
            let mut file = std::fs::File::create_new(directory.join(name)).unwrap();
            file.write_all(&bytes).unwrap();
            file.sync_all().unwrap();
            let mut log = std::fs::File::create_new(directory.join(format!("{name}.txt"))).unwrap();
            writeln!(log, "{result:?}\n{:?}", fake.messages()).unwrap();
            log.sync_all().unwrap();
            assert_eq!(fake.reference_count(), 1);
            assert_eq!(fake.release_calls(), 1);
            assert_eq!(fake.stream_reference_count(), 1);
            assert!(
                fake.messages()
                    .iter()
                    .all(|m| m.message == 1 && m.flags == 0)
            );
            if cancelled {
                assert!(matches!(result.unwrap(), TransferOutcome::Cancelled));
                assert!(!bytes.starts_with(b"BM"));
                assert!(fake.messages().iter().all(|m| m.percent < 100));
            } else {
                let TransferOutcome::Completed(summary) = result.unwrap() else {
                    panic!("expected complete image")
                };
                assert!(bytes.starts_with(b"BM"));
                assert_eq!(
                    u32::from_le_bytes(bytes[2..6].try_into().unwrap()) as usize,
                    bytes.len()
                );
                assert_eq!(fake.messages().last().unwrap().percent, 100);
                assert_eq!(fake.messages().last().unwrap().bytes, bytes.len() as u64);
                println!("{name}: {summary:?}, file_bytes={}", bytes.len());
            }
            let mut diagnostic = presence_request();
            assert_eq!(
                (m.diagnostic)(device.0, ptr::from_mut(&mut diagnostic).cast()),
                0
            );
        }
        assert_eq!((m.unlock)(device.0), 0);
    }
    drop(device);
    assert_eq!(helper.refs.load(Ordering::SeqCst), 1);
    std::fs::File::create_new(directory.join("complete.txt"))
        .unwrap()
        .sync_all()
        .unwrap();
}
