//! Opt-in hardware proof of the WIA -> IStiDevice -> IStiUSD lock path.
//! Service helpers are synthetic; USB, driver items and INQUIRY are real.
use super::*;
use crate::com_server::{self, minidrv, sti};
use std::{cell::Cell, os::windows::ffi::OsStrExt, ptr};

use crate::wia_transfer::fixture;

#[repr(C)]
struct ControlTable {
    unknown: minidrv::UnknownVtable,
    unused: [usize; 7],
    port: unsafe extern "system" fn(*mut c_void, *mut u16, u32) -> i32,
}
#[repr(C)]
struct Control {
    table: *const ControlTable,
    refs: Cell<u32>,
    port: Vec<u16>,
}
unsafe extern "system" fn query(
    _: *mut c_void,
    _: *const com_server::Guid,
    out: *mut *mut c_void,
) -> i32 {
    if out.is_null() {
        return E_POINTER;
    }
    // SAFETY: fixture caller provides writable output.
    unsafe {
        *out = ptr::null_mut();
    }
    com_server::E_NOINTERFACE
}
unsafe extern "system" fn control_add(raw: *mut c_void) -> u32 {
    // SAFETY: fixture retains storage until the driver's final Release.
    let c = unsafe { &*raw.cast::<Control>() };
    c.refs.set(c.refs.get() + 1);
    c.refs.get()
}
unsafe extern "system" fn control_release(raw: *mut c_void) -> u32 {
    // SAFETY: fixture retains storage until the driver's final Release.
    let c = unsafe { &*raw.cast::<Control>() };
    c.refs.set(c.refs.get() - 1);
    c.refs.get()
}
unsafe extern "system" fn port(raw: *mut c_void, out: *mut u16, count: u32) -> i32 {
    // SAFETY: held fixture receiver and caller-owned UTF-16 output buffer.
    unsafe {
        let c = &*raw.cast::<Control>();
        if out.is_null() {
            return E_POINTER;
        }
        if (count as usize) < c.port.len() {
            return 0x8007007au32 as i32;
        }
        ptr::copy_nonoverlapping(c.port.as_ptr(), out, c.port.len());
    }
    0
}
static CONTROL: ControlTable = ControlTable {
    unknown: minidrv::UnknownVtable {
        query,
        add_ref: control_add,
        release: control_release,
    },
    unused: [0; 7],
    port,
};

#[repr(C)]
struct Device {
    table: *const DevicePrefix,
    refs: Cell<u32>,
    owner: *mut c_void,
    mini: *const Interface,
    lock_calls: Cell<u32>,
    unlock_calls: Cell<u32>,
}
unsafe extern "system" fn device_add(raw: *mut c_void) -> u32 {
    // SAFETY: fixture storage is kept live by test, not COM-allocated.
    let d = unsafe { &*raw.cast::<Device>() };
    d.refs.set(d.refs.get() + 1);
    d.refs.get()
}
unsafe extern "system" fn device_release(raw: *mut c_void) -> u32 {
    // SAFETY: fixture storage is kept live through item tree teardown.
    let d = unsafe { &*raw.cast::<Device>() };
    d.refs.set(d.refs.get() - 1);
    d.refs.get()
}
unsafe extern "system" fn lock_device(raw: *mut c_void, timeout: u32) -> i32 {
    // SAFETY: test keeps driver and helper alive; this models service forwarding,
    // not actual Windows service lock ownership or apartment marshaling.
    unsafe {
        let d = &*raw.cast::<Device>();
        assert_eq!(timeout, LOCK_TIMEOUT_MS);
        assert_eq!((*d.mini).disconnect_client(), WIA_ERROR_BUSY);
        d.lock_calls.set(d.lock_calls.get() + 1);
        (sti::VTABLE.lock_device)(d.owner)
    }
}
unsafe extern "system" fn unlock_device(raw: *mut c_void) -> i32 {
    // SAFETY: driver and fixture stay live through synchronous forwarding.
    unsafe {
        let d = &*raw.cast::<Device>();
        assert_eq!((*d.mini).disconnect_client(), WIA_ERROR_BUSY);
        d.unlock_calls.set(d.unlock_calls.get() + 1);
        (sti::VTABLE.un_lock_device)(d.owner)
    }
}
static DEVICE: DevicePrefix = DevicePrefix {
    unknown: minidrv::UnknownVtable {
        query,
        add_ref: device_add,
        release: device_release,
    },
    unused: [0; 7],
    lock: lock_device,
    unlock: unlock_device,
};

#[test]
#[ignore = "Hardware: set freshly enumerated WC3119_TEST_STI_PATH; locks USB and sends INQUIRY. No scans or registry writes."]
fn actual_wia_lock_routes_through_sti_and_releases() {
    exercise(false);
}

#[test]
#[ignore = "Hardware scan: fresh WC3119_TEST_STI_PATH and new WC3119_TEST_OUTPUT_DIR; WIA dispatch gray/cancel/rescan, synthetic property snapshot."]
fn actual_wia_dispatch_scans_cancels_and_rescans() {
    exercise(true);
}

#[test]
#[ignore = "Hardware scan: fresh WC3119_TEST_STI_PATH and new WC3119_TEST_OUTPUT_DIR; parallel Rust cancellation, partial output, RGB rescan."]
fn actual_wia_dispatch_cancels_from_parallel_thread_and_rescans() {
    exercise_async_cancel();
}

fn exercise(scan: bool) {
    exercise_mode(scan, false);
}

fn exercise_async_cancel() {
    exercise_mode(true, true);
}

fn exercise_mode(scan: bool, async_cancel: bool) {
    let output = scan.then(|| {
        let path = std::path::PathBuf::from(
            std::env::var_os("WC3119_TEST_OUTPUT_DIR").expect("set new WC3119_TEST_OUTPUT_DIR"),
        );
        std::fs::create_dir(&path).expect("output directory must be new");
        path
    });
    let path = std::env::var_os("WC3119_TEST_STI_PATH").expect("set fresh WC3119_TEST_STI_PATH");
    let mut control = Control {
        table: &CONTROL,
        refs: Cell::new(1),
        port: path.encode_wide().chain([0]).collect(),
    };
    let mut factory = ptr::null_mut();
    let mut mini = ptr::null_mut();
    // SAFETY: caller keeps all fixture storage and COM apartment alive. Only
    // this explicitly ignored test opens the actual caller-selected scanner.
    unsafe {
        assert_eq!(CoInitializeEx(ptr::null_mut(), 0), 0);
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
        let owner = minidrv::owner(mini);
        assert_eq!(
            (sti::VTABLE.initialize)(
                owner,
                ptr::from_mut(&mut control).cast(),
                sti::STI_VERSION,
                ptr::null_mut()
            ),
            0
        );
        let interface = &*mini.cast::<Interface>();
        let mut device = Device {
            table: &DEVICE,
            refs: Cell::new(1),
            owner,
            mini: interface,
            lock_calls: Cell::new(0),
            unlock_calls: Cell::new(0),
        };
        interface
            .initialize_tree(
                mini,
                vec![65],
                "synthetic\\Root".encode_utf16().collect(),
                ptr::from_mut(&mut device).cast(),
            )
            .unwrap();
        for round in 0..3 {
            assert_eq!(dispatch(interface, true), 0);
            let mut diagnostic = sti::StiDiag {
                dw_size: std::mem::size_of::<sti::StiDiag>() as u32,
                dw_basic_diag_code: 1,
                dw_vendor_diag_code: 0,
                dw_status_mask: 0,
                s_error_info: sti::StiErrorInfo::default(),
            };
            assert_eq!((sti::VTABLE.diagnostic)(owner, &mut diagnostic), 0);
            // A second lock cannot reopen or replace the held USB handle.
            assert!(dispatch(interface, true) < 0);
            if let Some(output) = &output {
                // Start asynchronous cancellation from an idle device. A
                // previous completed scan may still return RESERVE Busy while
                // the carriage returns; cancelling that unconfirmed ownership
                // intentionally quarantines the session instead of sending ABORT.
                if async_cancel && round == 0 {
                    scan_one_async_cancel(mini, interface, output);
                } else if async_cancel {
                    scan_one(mini, if round == 1 { 2 } else { 0 }, output);
                } else {
                    scan_one(mini, round, output);
                }
                assert_eq!((sti::VTABLE.diagnostic)(owner, &mut diagnostic), 0);
            }
            assert_eq!(dispatch(interface, false), 0);
        }
        assert_eq!(device.lock_calls.get(), 6);
        assert_eq!(device.unlock_calls.get(), 3);
        assert_eq!(interface.disconnect_client(), 0);
        assert_eq!(device.refs.get(), 1);
        assert_eq!(minidrv::release(mini), 0);
        assert_eq!(control.refs.get(), 1);
        CoUninitialize();
    }
    if let Some(output) = output {
        std::fs::File::create_new(output.join("complete.txt"))
            .unwrap()
            .sync_all()
            .unwrap();
    }
}

unsafe fn scan_one(mini: *mut c_void, round: usize, output: &std::path::Path) {
    use std::io::Write;
    let mut callback = fixture::FakeTransferCallback::new();
    if round == 1 {
        callback.set_send_plan(fixture::SendMessagePlan::CancelAt(2));
    }
    // SAFETY: exercise retains mini and its COM apartment until callback drops.
    unsafe {
        callback.set_hook(Box::new(move || {
            let interface = &*mini.cast::<Interface>();
            assert_eq!(interface.disconnect_client(), WIA_ERROR_BUSY);
            assert_eq!(dispatch(interface, false), WIA_ERROR_BUSY);
            assert_eq!(
                minidrv::acquire::dispatch(
                    interface,
                    || panic!("nested read"),
                    |_, _| panic!("nested scan")
                ),
                WIA_ERROR_BUSY
            );
        }));
        let result = minidrv::acquire::dispatch(
            &*mini.cast::<Interface>(),
            || {
                // Synthetic property values, never a fake WIA service context.
                Ok(minidrv::properties::Snapshot {
                    settings: crate::wia::FlatbedSettings {
                        x_resolution: 75,
                        y_resolution: 75,
                        x_position: 0,
                        y_position: 0,
                        x_extent: 600,
                        y_extent: 800,
                        data_type: if round == 0 { 2 } else { 3 },
                        depth: if round == 0 { 8 } else { 24 },
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
                (*minidrv::owner(mini).cast::<com_server::Instance>())
                    .state
                    .transfer_bmp(
                        snapshot.settings,
                        cancel,
                        callback.as_raw(),
                        &snapshot.item,
                        &snapshot.full_item,
                    )
                    .inspect_err(|error| {
                        eprintln!("scan round {round} error: {error}");
                        let mut log = std::fs::File::create_new(
                            output.join(format!("scan-round-{round}-error.txt")),
                        )
                        .expect("new error evidence");
                        writeln!(log, "{error}").unwrap();
                        log.sync_all().unwrap();
                    })
            },
        );
        let bytes = callback.stream_bytes().unwrap();
        let name = ["gray75.bmp", "cancelled-rgb75.partial", "rgb75-rescan.bmp"][round];
        let mut file = std::fs::File::create_new(output.join(name)).unwrap();
        file.write_all(&bytes).unwrap();
        file.sync_all().unwrap();
        let mut log = std::fs::File::create_new(output.join(format!("{name}.txt"))).unwrap();
        writeln!(log, "HRESULT={result:#010x}\n{:?}", callback.messages()).unwrap();
        log.sync_all().unwrap();
        assert_eq!(callback.reference_count(), 1);
        assert_eq!(callback.stream_reference_count(), 1);
        assert_eq!(result, if round == 1 { 1 } else { 0 });
        assert_eq!(bytes.starts_with(b"BM"), round != 1);
        if round == 1 {
            assert!(callback.messages().iter().all(|m| m.percent < 100));
        } else {
            assert_eq!(callback.messages().last().unwrap().percent, 100);
            assert_eq!(
                callback.messages().last().unwrap().bytes,
                bytes.len() as u64
            );
        }
        println!("{name}: HRESULT={result}, bytes={}", bytes.len());
    }
}

unsafe fn scan_one_async_cancel(
    mini: *mut c_void,
    interface: &Interface,
    output: &std::path::Path,
) {
    use std::{
        io::Write,
        sync::mpsc,
        time::{Duration, Instant},
    };

    let mut callback = fixture::FakeTransferCallback::new();
    let (registered_tx, registered_rx) = mpsc::sync_channel(0);
    let (done_tx, done_rx) = mpsc::sync_channel(0);
    let started = Instant::now();

    // SAFETY: only the pure-Rust cancellation state is borrowed by the worker;
    // all native COM objects and the USB-backed interface stay on this thread.
    let result = std::thread::scope(|scope| {
        let cancel_state = &interface.cancellation;
        let cancel_thread = scope.spawn(move || {
            registered_rx
                .recv_timeout(Duration::from_secs(5))
                .expect("scan did not register its cancellation job");
            match done_rx.recv_timeout(Duration::from_millis(500)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Disconnected) => {}
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    assert!(
                        cancel_state.cancel(&[65]),
                        "registered scan disappeared before asynchronous cancellation"
                    );
                }
            }
        });

        let dispatch_result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            // SAFETY: exercise retains mini and its COM apartment until callback drops.
            unsafe {
                callback.set_hook(Box::new(move || {
                    let interface = &*mini.cast::<Interface>();
                    assert_eq!(interface.disconnect_client(), WIA_ERROR_BUSY);
                    assert_eq!(dispatch(interface, false), WIA_ERROR_BUSY);
                    assert_eq!(
                        minidrv::acquire::dispatch(
                            interface,
                            || panic!("nested read"),
                            |_, _| panic!("nested scan")
                        ),
                        WIA_ERROR_BUSY
                    );
                }));
                minidrv::acquire::dispatch(
                    interface,
                    || {
                        // The job is registered before this closure runs. The
                        // zero-capacity channel makes the timer handshake explicit.
                        registered_tx
                            .send(())
                            .expect("cancellation worker stopped before registration");
                        // Synthetic property values, never a fake WIA service context.
                        Ok(minidrv::properties::Snapshot {
                            settings: crate::wia::FlatbedSettings {
                                x_resolution: 75,
                                y_resolution: 75,
                                x_position: 0,
                                y_position: 0,
                                x_extent: 600,
                                y_extent: 800,
                                data_type: 3,
                                depth: 24,
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
                        (*minidrv::owner(mini).cast::<com_server::Instance>())
                            .state
                            .transfer_bmp(
                                snapshot.settings,
                                cancel,
                                callback.as_raw(),
                                &snapshot.item,
                                &snapshot.full_item,
                            )
                            .inspect_err(|error| {
                                eprintln!("asynchronous scan error: {error}");
                                let mut log =
                                    std::fs::File::create_new(output.join("scan-error.txt"))
                                        .expect("new error evidence");
                                writeln!(log, "{error}").unwrap();
                                log.sync_all().unwrap();
                            })
                    },
                )
            }
        }));
        let _ = done_tx.send(());
        cancel_thread
            .join()
            .expect("asynchronous cancellation worker panicked");
        match dispatch_result {
            Ok(result) => result,
            Err(payload) => std::panic::resume_unwind(payload),
        }
    });

    let elapsed = started.elapsed();
    let bytes = callback
        .stream_bytes()
        .expect("cancelled scan must leave an inspectable callback stream");
    let name = "cancelled-rgb75.partial";
    let mut file = std::fs::File::create_new(output.join(name)).unwrap();
    file.write_all(&bytes).unwrap();
    file.sync_all().unwrap();
    let callback_count = callback.messages().len();
    let mut log = std::fs::File::create_new(output.join(format!("{name}.txt"))).unwrap();
    writeln!(
        log,
        "HRESULT={result:#010x}\ncancel_requested_after_ms=500\nelapsed_ms={}\ncallback_count={callback_count}\nbytes={}\n{:?}",
        elapsed.as_millis(),
        bytes.len(),
        callback.messages()
    )
    .unwrap();
    log.sync_all().unwrap();
    assert_eq!(callback.reference_count(), 1);
    assert_eq!(callback.stream_reference_count(), 1);
    assert_eq!(result, 1);
    assert!(
        !bytes.starts_with(b"BM"),
        "cancelled output must remain partial"
    );
    assert!(callback.messages().iter().all(|m| m.percent < 100));
    println!(
        "{name}: HRESULT={result}, bytes={}, callbacks={callback_count}, elapsed_ms={}",
        bytes.len(),
        elapsed.as_millis()
    );
}

#[link(name = "Ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, flags: u32) -> i32;
    fn CoUninitialize();
}
