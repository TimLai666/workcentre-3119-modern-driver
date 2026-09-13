//! WorkCentre 3119 hardware identification and diagnostics.
//!
//! Transport readiness does not imply that scanning or WIA integration works.

pub mod bitmap;
pub mod protocol;
pub mod scan;

#[cfg(windows)]
mod usb;

/// Query reported scanner capabilities. This sends INQUIRY but does not start a scan.
#[cfg(windows)]
pub fn inquiry() -> std::io::Result<protocol::Capabilities> {
    protocol::Capabilities::parse(&usb::inquiry()?)
}

/// Non-identifying USB metadata and raw INQUIRY for reproducible diagnostics.
#[derive(Debug)]
pub struct InquiryEvidence {
    pub device_descriptor: [u8; 18],
    pub interface_descriptor: [u8; 9],
    pub bulk_in: u8,
    pub bulk_out: u8,
    pub bulk_in_max_packet: u16,
    pub bulk_out_max_packet: u16,
    pub reply: Vec<u8>,
}

#[cfg(windows)]
pub fn inquiry_evidence() -> std::io::Result<InquiryEvidence> {
    let mut session = usb::UsbSession::open()?;
    session.write(&[0x1b, 0xa8, 0x12, 0])?;
    let mut reply = vec![0; 1024];
    let n = session.read(&mut reply)?;
    reply.truncate(n);
    protocol::Capabilities::parse(&reply)?;
    Ok(InquiryEvidence {
        device_descriptor: session.descriptor,
        interface_descriptor: session.interface,
        bulk_in: session.bulk_in,
        bulk_out: session.bulk_out,
        bulk_in_max_packet: session.bulk_in_max_packet,
        bulk_out_max_packet: session.bulk_out_max_packet,
        reply,
    })
}

#[cfg(not(windows))]
pub fn inquiry_evidence() -> std::io::Result<InquiryEvidence> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "USB inquiry requires Windows",
    ))
}

#[cfg(not(windows))]
pub fn inquiry() -> std::io::Result<protocol::Capabilities> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "USB inquiry requires Windows",
    ))
}

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::discover;

#[cfg(not(windows))]
pub fn discover() -> std::io::Result<Vec<Device>> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "Device discovery requires Windows",
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Interface {
    Composite,
    Scanner,
    Printer,
}

/// Match hardware IDs exactly. Never match the printer or composite parent
/// when deciding which interface may eventually receive a scanning driver.
pub fn identify(instance_id: &str) -> Option<Interface> {
    let mut parts = instance_id.split('\\');
    if !parts.next()?.eq_ignore_ascii_case("USB") {
        return None;
    }
    let hardware = parts.next()?;
    if parts.next()?.is_empty() || parts.next().is_some() {
        return None;
    }
    if hardware.eq_ignore_ascii_case("VID_0924&PID_4265&MI_00") {
        Some(Interface::Scanner)
    } else if hardware.eq_ignore_ascii_case("VID_0924&PID_4265&MI_01") {
        Some(Interface::Printer)
    } else if hardware.eq_ignore_ascii_case("VID_0924&PID_4265") {
        Some(Interface::Composite)
    } else {
        None
    }
}

/// Device identifiers and serial numbers are deliberately excluded from reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub interface: Interface,
    pub service: Option<String>,
    pub problem_code: u32,
    pub started: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Readiness {
    NotScanner,
    DriverMissing,
    DeviceProblem(u32),
    NotStarted,
    OtherDriver,
    DriverStarted,
}

impl Device {
    pub fn readiness(&self) -> Readiness {
        if self.interface != Interface::Scanner {
            Readiness::NotScanner
        } else if self.problem_code == 28 {
            Readiness::DriverMissing
        } else if self.problem_code != 0 {
            Readiness::DeviceProblem(self.problem_code)
        } else if !self.started {
            Readiness::NotStarted
        } else if self
            .service
            .as_deref()
            .is_some_and(|s| s.eq_ignore_ascii_case("WinUSB"))
        {
            Readiness::DriverStarted
        } else {
            Readiness::OtherDriver
        }
    }
}
