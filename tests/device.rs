use workcentre_3119::{Device, Interface, Readiness, identify};

#[test]
fn identifies_only_exact_workcentre_usb_interfaces() {
    assert_eq!(
        identify(r"USB\VID_0924&PID_4265&MI_00\instance"),
        Some(Interface::Scanner)
    );
    assert_eq!(
        identify(r"usb\vid_0924&pid_4265&mi_01\instance"),
        Some(Interface::Printer)
    );
    assert_eq!(
        identify(r"USB\VID_0924&PID_4265\serial"),
        Some(Interface::Composite)
    );
    for id in [
        "",
        r"USB\VID_0924&PID_42650&MI_00\x",
        r"USB\VID_0924&PID_4265&MI_02\x",
        r"USBPRINT\VID_0924&PID_4265\x",
        r"USB\VID_0924&PID_4265&MI_00",
        r"USB\VID_0924&PID_4265&MI_00\",
        r"USB\VID_0924&PID_4265&MI_00\x\y",
    ] {
        assert_eq!(identify(id), None, "must reject {id:?}");
    }
}

fn scanner(service: Option<&str>, problem_code: u32, started: bool) -> Device {
    Device {
        interface: Interface::Scanner,
        service: service.map(str::to_owned),
        problem_code,
        started,
    }
}

#[test]
fn diagnosis_distinguishes_missing_driver_from_ready_usb_transport() {
    assert_eq!(
        scanner(None, 28, false).readiness(),
        Readiness::DriverMissing
    );
    assert_eq!(
        scanner(Some("WinUSB"), 0, true).readiness(),
        Readiness::DriverStarted
    );
    assert_eq!(
        scanner(Some("winusb"), 10, false).readiness(),
        Readiness::DeviceProblem(10)
    );
    assert_eq!(
        scanner(Some("usbscan"), 0, true).readiness(),
        Readiness::OtherDriver
    );
    assert_eq!(scanner(None, 0, false).readiness(), Readiness::NotStarted);
    assert_eq!(
        scanner(Some("winusb"), 0, false).readiness(),
        Readiness::NotStarted
    );
    assert_eq!(scanner(None, 0, true).readiness(), Readiness::OtherDriver);
}

#[test]
fn printer_and_parent_can_never_be_reported_as_scanner_transport() {
    for interface in [Interface::Printer, Interface::Composite] {
        let device = Device {
            interface,
            service: Some("WinUSB".into()),
            problem_code: 0,
            started: true,
        };
        assert_eq!(device.readiness(), Readiness::NotScanner);
    }
}
