//! Bounded flatbed scan jobs. Wire field provenance is recorded in ENG.md.

use crate::protocol::Capabilities;
use std::io;

const MAX_BAND_BYTES: usize = 16 * 1024 * 1024;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorMode {
    Gray,
    Rgb,
}

impl ColorMode {
    fn code(self) -> u8 {
        match self {
            Self::Gray => 3,
            Self::Rgb => 5,
        }
    }
    fn channels(self) -> usize {
        match self {
            Self::Gray => 1,
            Self::Rgb => 3,
        }
    }
}

/// Flatbed geometry in 1/1200 inch. Offsets must be multiples of 12.
#[derive(Clone, Copy, Debug)]
pub struct ScanRequest {
    pub dpi: u32,
    pub mode: ColorMode,
    pub x_units: u32,
    pub y_units: u32,
    pub width_units: u32,
    pub height_units: u32,
}

impl ScanRequest {
    fn command(self, caps: &Capabilities) -> io::Result<[u8; 25]> {
        let resolution = match self.dpi {
            75 => 0,
            100 => 10,
            150 => 2,
            200 => 9,
            300 => 5,
            600 => 7,
            _ => return Err(invalid("Unsupported scan resolution")),
        };
        if !caps.resolutions().contains(&self.dpi)
            || caps.mode_mask & (1 << self.mode.code()) == 0
            || caps.compression_mask & 1 == 0
            || (self.mode == ColorMode::Rgb && caps.line_order > 1)
        {
            return Err(invalid(
                "Requested image format is not supported by reported capabilities",
            ));
        }
        let within = |start: u32, size: u32, limit: u32| {
            size > 0 && start.checked_add(size).is_some_and(|end| end <= limit)
        };
        if !within(self.x_units, self.width_units, caps.width_units)
            || !within(
                self.y_units,
                self.height_units,
                caps.flatbed_length_units.min(caps.length_units),
            )
            || !self.x_units.is_multiple_of(12)
            || !self.y_units.is_multiple_of(12)
            || self.x_units / 1200 > 255
            || self.y_units / 1200 > 255
        {
            return Err(invalid(
                "Scan region is empty, unrepresentable or outside the flatbed",
            ));
        }
        let mut b = [0; 25];
        b[..5].copy_from_slice(&[0x1b, 0xa8, 0x24, 0x13, 0x30]);
        b[5..9].copy_from_slice(&self.width_units.to_be_bytes());
        b[9..13].copy_from_slice(&self.height_units.to_be_bytes());
        b[13] = resolution;
        b[14] = resolution;
        b[15] = (self.x_units / 1200) as u8;
        b[16] = ((self.x_units % 1200) / 12) as u8;
        b[17] = (self.y_units / 1200) as u8;
        b[18] = ((self.y_units % 1200) / 12) as u8;
        b[19] = self.mode.code();
        b[22] = 2;
        b[23] = 0x40;
        Ok(b)
    }
}

#[derive(Debug, PartialEq, Eq)]
enum ReplyStatus {
    Good,
    Busy,
}

fn response_status(b: &[u8], read: bool) -> io::Result<ReplyStatus> {
    if b.len() < 32 || b[0] != 0xa8 || usize::from(b[2]) + 3 != b.len() {
        return Err(invalid("Malformed scanner response"));
    }
    match b[1] {
        0 => Ok(ReplyStatus::Good),
        8 => Ok(ReplyStatus::Busy),
        4 => Err(io::Error::new(
            io::ErrorKind::Interrupted,
            "Scanner cancelled the job",
        )),
        2 => {
            let offset = if read { 12 } else { 4 };
            let state = u16::from_be_bytes([b[offset], b[offset + 1]]);
            if state != 0 && state & !0x481 == 0 && state & 0x480 != 0 {
                Ok(ReplyStatus::Busy)
            } else {
                Err(invalid(format!("Scanner CHECK state 0x{state:04x}")))
            }
        }
        status => Err(invalid(format!("Unknown scanner status 0x{status:02x}"))),
    }
}

struct BandInfo {
    length: usize,
    width: usize,
    rows: usize,
    final_band: bool,
}

impl BandInfo {
    fn parse(b: &[u8], mode: ColorMode) -> io::Result<Self> {
        if response_status(b, true)? != ReplyStatus::Good || !matches!(b[3], 0x80 | 0x81) {
            return Err(invalid("Expected image band metadata"));
        }
        let length = u32::from_be_bytes(b[4..8].try_into().unwrap()) as usize;
        let rows = u16::from_be_bytes([b[8], b[9]]) as usize;
        let width = u16::from_be_bytes([b[10], b[11]]) as usize;
        let pixels = width
            .checked_mul(rows)
            .and_then(|v| v.checked_mul(mode.channels()))
            .ok_or_else(|| invalid("Image size overflow"))?;
        if width == 0
            || rows == 0
            || width * mode.channels() > 65536
            || length > MAX_BAND_BYTES
            || length < pixels
            || length - pixels > 16
        {
            return Err(invalid(format!(
                "Invalid image band: length={length}, width={width}, rows={rows}"
            )));
        }
        Ok(Self {
            length,
            width,
            rows,
            final_band: b[3] == 0x81,
        })
    }
    fn decode(&self, raw: &[u8], mode: ColorMode, order: u8) -> io::Result<Vec<u8>> {
        if raw.len() != self.length || (mode == ColorMode::Rgb && order > 1) {
            return Err(invalid(
                "Incomplete image band or unsupported channel order",
            ));
        }
        let size = self.width * self.rows * mode.channels();
        let mut pixels = raw[..size].to_vec();
        if mode == ColorMode::Rgb && order == 1 {
            for (input, output) in raw[..size]
                .chunks_exact(self.width * 3)
                .zip(pixels.chunks_exact_mut(self.width * 3))
            {
                for x in 0..self.width {
                    for c in 0..3 {
                        output[x * 3 + c] = input[c * self.width + x];
                    }
                }
            }
        }
        Ok(pixels)
    }
}

use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

/// One complete band. Values are unmodified except planar-to-interleaved RGB ordering.
pub struct ImageBand {
    pub width: u32,
    pub rows: u32,
    pub mode: ColorMode,
    pub pixels: Vec<u8>,
    /// Device bytes including trailing padding, retained for pipeline comparison.
    pub wire_data: Vec<u8>,
}

#[derive(Debug)]
pub struct ScanSummary {
    pub width: u32,
    pub height: u32,
    pub bands: u32,
    pub bytes: usize,
}

trait Transport {
    fn write(&mut self, data: &[u8]) -> io::Result<()>;
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize>;
}

#[cfg(windows)]
impl Transport for crate::usb::UsbSession {
    fn write(&mut self, b: &[u8]) -> io::Result<()> {
        self.write(b)
    }
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        self.read(b)
    }
}

/// Run one bounded scan using newly discovered MI_00 and fresh capabilities.
/// Cancellation is checked between transfers; an in-flight band is drained before ABORT.
/// A callback must return promptly. On error discard previously delivered partial bands.
#[cfg(windows)]
pub fn scan_to(
    request: ScanRequest,
    cancel: &AtomicBool,
    mut sink: impl FnMut(&ImageBand) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"));
    }
    run_job(
        &mut crate::usb::UsbSession::open()?,
        request,
        cancel,
        &mut sink,
    )
}

#[cfg(not(windows))]
pub fn scan_to(
    _: ScanRequest,
    _: &AtomicBool,
    _: impl FnMut(&ImageBand) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Scanning requires Windows",
    ))
}

/// Configure and record capabilities from the very session that acquires the image.
/// Preparation happens before RESERVE; its failure never starts the scanner.
#[cfg(windows)]
pub fn scan_with_evidence(
    cancel: &AtomicBool,
    prepare: impl FnOnce(&crate::InquiryEvidence) -> io::Result<ScanRequest>,
    mut sink: impl FnMut(&ImageBand) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"));
    }
    let mut usb = crate::usb::UsbSession::open()?;
    let mut evidence = crate::InquiryEvidence {
        device_descriptor: usb.descriptor,
        interface_descriptor: usb.interface,
        bulk_in: usb.bulk_in,
        bulk_out: usb.bulk_out,
        bulk_in_max_packet: usb.bulk_in_max_packet,
        bulk_out_max_packet: usb.bulk_out_max_packet,
        reply: Vec::new(),
    };
    run_prepared_job(&mut usb, cancel, &mut sink, |reply| {
        evidence.reply = reply.to_vec();
        prepare(&evidence)
    })
}

#[cfg(not(windows))]
pub fn scan_with_evidence(
    _: &AtomicBool,
    _: impl FnOnce(&crate::InquiryEvidence) -> io::Result<ScanRequest>,
    _: impl FnMut(&ImageBand) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Scanning requires Windows",
    ))
}

fn checkpoint(cancel: &AtomicBool, deadline: Instant) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"))
    } else if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Scan exceeded 120 second deadline",
        ))
    } else {
        Ok(())
    }
}

fn exchange(
    usb: &mut impl Transport,
    command: &[u8],
    synchronized: &mut bool,
) -> io::Result<Vec<u8>> {
    *synchronized = false;
    let context =
        |e: io::Error| io::Error::new(e.kind(), format!("Command 0x{:02x}: {e}", command[2]));
    usb.write(command).map_err(context)?;
    let mut b = vec![0; 1024];
    let n = usb.read(&mut b).map_err(context)?;
    if n > b.len() {
        return Err(invalid("Response exceeds receive buffer"));
    }
    b.truncate(n);
    if n < 32 || b[0] != 0xa8 || usize::from(b[2]) + 3 != n {
        return Err(invalid(format!(
            "Command 0x{:02x}: malformed reply {b:02x?}; stream synchronization lost",
            command[2]
        )));
    }
    let expected = match command[2] {
        0x12 => b[3] == 0x10,
        0x28 => matches!(b[3], 0x20 | 0x80 | 0x81),
        0x24 => matches!(b[3], 0 | 0x20 | 0x30),
        _ => matches!(b[3], 0 | 0x20),
    };
    if !expected {
        return Err(invalid(format!(
            "Command 0x{:02x}: unrelated reply message 0x{:02x}; stream synchronization lost",
            command[2], b[3]
        )));
    }
    *synchronized = true;
    Ok(b)
}

fn ready(
    usb: &mut impl Transport,
    opcode: u8,
    synchronized: &mut bool,
    cancel: &AtomicBool,
    deadline: Instant,
) -> io::Result<Vec<u8>> {
    loop {
        checkpoint(cancel, deadline)?;
        let b = exchange(usb, &[0x1b, 0xa8, opcode, 0], synchronized)?;
        if response_status(&b, opcode == 0x28)? == ReplyStatus::Good {
            return Ok(b);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn finish(usb: &mut impl Transport, abort: bool, synchronized: &mut bool) -> io::Result<()> {
    if !*synchronized {
        return Err(io::Error::other(
            "USB reply stream is uncertain; reconnect scanner before another job",
        ));
    }
    let mut errors = vec![];
    for opcode in if abort {
        &[0x06, 0x17][..]
    } else {
        &[0x17][..]
    } {
        let result = exchange(usb, &[0x1b, 0xa8, *opcode, 0], synchronized).and_then(|b| {
            match response_status(&b, false)? {
                ReplyStatus::Good => Ok(()),
                ReplyStatus::Busy => Err(io::Error::other("Cleanup command is busy")),
            }
        });
        if let Err(e) = result {
            errors.push(e.to_string());
        }
        if !*synchronized {
            break;
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "Cleanup unconfirmed: {}; reconnect scanner",
            errors.join("; ")
        )))
    }
}

fn run_job(
    usb: &mut impl Transport,
    request: ScanRequest,
    cancel: &AtomicBool,
    sink: &mut impl FnMut(&ImageBand) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    run_prepared_job(usb, cancel, sink, |_| Ok(request))
}

fn run_prepared_job(
    usb: &mut impl Transport,
    cancel: &AtomicBool,
    sink: &mut impl FnMut(&ImageBand) -> io::Result<()>,
    prepare: impl FnOnce(&[u8]) -> io::Result<ScanRequest>,
) -> io::Result<ScanSummary> {
    let deadline = Instant::now() + Duration::from_secs(120);
    checkpoint(cancel, deadline)?;
    let mut synchronized = true;
    let raw = exchange(usb, &[0x1b, 0xa8, 0x12, 0], &mut synchronized)?;
    let caps = Capabilities::parse(&raw)?;
    let request = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prepare(&raw)))
        .map_err(|_| io::Error::other("Scan preparation panicked"))??;
    let window = request.command(&caps)?;
    ready(usb, 0x16, &mut synchronized, cancel, deadline).map_err(|e| {
        if synchronized {
            e
        } else {
            io::Error::new(
                e.kind(),
                format!("{e}; reservation unconfirmed; reconnect scanner before another job"),
            )
        }
    })?;
    // Ownership is confirmed only after RESERVE succeeds. Never release another owner's busy device.
    let result = (|| {
        checkpoint(cancel, deadline)?;
        response_status(&exchange(usb, &window, &mut synchronized)?, false)?;
        ready(usb, 0x31, &mut synchronized, cancel, deadline)?;
        let mut summary = ScanSummary {
            width: 0,
            height: 0,
            bands: 0,
            bytes: 0,
        };
        loop {
            let b = ready(usb, 0x28, &mut synchronized, cancel, deadline)?;
            let band = BandInfo::parse(&b, request.mode)?;
            if summary.bands >= 4096
                || summary.bytes + band.length > 256 * 1024 * 1024
                || (summary.width != 0 && summary.width != band.width as u32)
            {
                return Err(invalid(
                    "Image exceeds job limits or changes width between bands",
                ));
            }
            synchronized = false;
            usb.write(&[0x1b, 0xa8, 0x29, 0])?;
            let mut raw = Vec::with_capacity(band.length);
            let mut buffer = [0; 65536];
            while raw.len() < band.length {
                // Drain this known band before honoring cancellation, keeping command framing intact.
                if Instant::now() >= deadline {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "Image transfer deadline exceeded",
                    ));
                }
                let remaining = band.length - raw.len();
                let capacity = remaining
                    .div_ceil(1024)
                    .saturating_mul(1024)
                    .min(buffer.len());
                let n = usb.read(&mut buffer[..capacity])?;
                if n == 0 || n > capacity || n > remaining {
                    return Err(invalid(
                        "Image read made no progress or exceeded advertised band length",
                    ));
                }
                raw.extend_from_slice(&buffer[..n]);
            }
            synchronized = true;
            checkpoint(cancel, deadline)?;
            let pixels = band.decode(&raw, request.mode, caps.line_order)?;
            summary.width = band.width as u32;
            summary.height = summary
                .height
                .checked_add(band.rows as u32)
                .ok_or_else(|| invalid("Image height overflow"))?;
            summary.bands += 1;
            summary.bytes += pixels.len();
            let image = ImageBand {
                width: summary.width,
                rows: band.rows as u32,
                mode: request.mode,
                pixels,
                wire_data: raw,
            };
            // A panicking consumer is never called again. The band is fully drained,
            // so the error path can safely cancel and release our reservation.
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(&image)))
                .map_err(|_| io::Error::other("Image consumer panicked; job cancelled"))??;
            checkpoint(cancel, deadline)?;
            if band.final_band {
                return Ok(summary);
            }
        }
    })();
    let cleanup = finish(usb, result.is_err(), &mut synchronized);
    match (result, cleanup) {
        (Ok(summary), Ok(())) => Ok(summary),
        (Err(e), Ok(())) | (Ok(_), Err(e)) => Err(e),
        (Err(e), Err(cleanup)) => Err(io::Error::new(e.kind(), format!("{e}; {cleanup}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> crate::protocol::Capabilities {
        crate::protocol::Capabilities {
            identity: "synthetic".into(),
            resolution_mask: 0x353f,
            mode_mask: 0x29,
            width_units: 10200,
            length_units: 14040,
            flatbed_length_units: 14040,
            line_order: 1,
            compression_mask: 0x2f,
        }
    }
    fn request() -> ScanRequest {
        ScanRequest {
            dpi: 150,
            mode: ColorMode::Gray,
            x_units: 120,
            y_units: 240,
            width_units: 1200,
            height_units: 2400,
        }
    }
    #[test]
    fn window_matches_wire_fields_and_neutral_uncompressed_flatbed_settings() {
        let cmd = request().command(&caps()).unwrap();
        assert_eq!(
            cmd,
            [
                0x1b, 0xa8, 0x24, 0x13, 0x30, 0, 0, 4, 0xb0, 0, 0, 9, 0x60, 2, 2, 0, 10, 0, 20, 3,
                0, 0, 2, 0x40, 0
            ]
        );
    }
    #[test]
    fn settings_reject_unreported_modes_unsupported_geometry_and_overflow() {
        for r in [
            ScanRequest {
                dpi: 1200,
                ..request()
            },
            ScanRequest {
                width_units: 0,
                ..request()
            },
            ScanRequest {
                x_units: 1,
                ..request()
            },
            ScanRequest {
                x_units: u32::MAX,
                ..request()
            },
            ScanRequest {
                height_units: u32::MAX,
                ..request()
            },
            ScanRequest {
                width_units: 10200,
                ..request()
            },
        ] {
            assert!(r.command(&caps()).is_err());
        }
        let mut c = caps();
        c.mode_mask = 1;
        assert!(request().command(&c).is_err());
        c = caps();
        c.compression_mask = 0x40;
        assert!(request().command(&c).is_err());
        c = caps();
        c.line_order = 2;
        assert!(
            ScanRequest {
                mode: ColorMode::Rgb,
                ..request()
            }
            .command(&c)
            .is_err()
        );
    }
    fn reply(message: u8) -> Vec<u8> {
        let mut b = vec![0; 32];
        b[..4].copy_from_slice(&[0xa8, 0, 29, message]);
        b
    }
    struct Synthetic {
        replies: std::collections::VecDeque<Vec<u8>>,
        writes: Vec<u8>,
    }
    impl Transport for Synthetic {
        fn write(&mut self, b: &[u8]) -> io::Result<()> {
            self.writes.push(b[2]);
            Ok(())
        }
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            let next = self
                .replies
                .pop_front()
                .ok_or_else(|| io::Error::other("Synthetic read exhausted"))?;
            b[..next.len()].copy_from_slice(&next);
            Ok(next.len())
        }
    }
    fn synthetic() -> Synthetic {
        let mut inquiry = vec![0; 70];
        inquiry[..4].copy_from_slice(&[0xa8, 0, 67, 0x10]);
        inquiry[4..13].copy_from_slice(b"synthetic");
        inquiry[36..38].copy_from_slice(&0x353fu16.to_be_bytes());
        inquiry[39] = 0x29;
        inquiry[40..44].copy_from_slice(&10200u32.to_be_bytes());
        inquiry[44..48].copy_from_slice(&14040u32.to_be_bytes());
        inquiry[49] = 1;
        inquiry[50] = 0x2f;
        inquiry[60..64].copy_from_slice(&14040u32.to_be_bytes());
        let mut band = reply(0x81);
        band[4..8].copy_from_slice(&18u32.to_be_bytes());
        band[8..10].copy_from_slice(&1u16.to_be_bytes());
        band[10..12].copy_from_slice(&2u16.to_be_bytes());
        let mut raw = vec![42, 190];
        raw.resize(18, 0);
        Synthetic {
            replies: [
                inquiry,
                reply(0),
                reply(0x30),
                reply(0x20),
                band,
                raw,
                reply(0),
            ]
            .into(),
            writes: vec![],
        }
    }
    #[test]
    fn complete_job_streams_actual_values_then_releases() {
        let mut usb = synthetic();
        let mut pixels = vec![];
        let summary = run_job(
            &mut usb,
            request(),
            &std::sync::atomic::AtomicBool::new(false),
            &mut |band| {
                pixels.extend_from_slice(&band.pixels);
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(pixels, [42, 190]);
        assert_eq!((summary.width, summary.height), (2, 1));
        assert_eq!(usb.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29, 0x17]);
    }
    #[test]
    fn consumer_failure_aborts_and_releases_without_success() {
        let mut usb = synthetic();
        usb.replies.push_back(reply(0));
        let result = run_job(
            &mut usb,
            request(),
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| Err(io::Error::other("Consumer disk full")),
        );
        assert!(result.unwrap_err().to_string().contains("disk full"));
        assert_eq!(usb.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29, 0x06, 0x17]);
    }
    #[test]
    fn pre_cancelled_job_never_touches_device() {
        let mut usb = synthetic();
        assert!(
            run_job(
                &mut usb,
                request(),
                &std::sync::atomic::AtomicBool::new(true),
                &mut |_| Ok(())
            )
            .is_err()
        );
        assert!(usb.writes.is_empty());
    }

    #[test]
    fn consumer_panic_is_contained_and_reservation_released() {
        let mut usb = synthetic();
        usb.replies.push_back(reply(0));
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            run_job(&mut usb, request(), &AtomicBool::new(false), &mut |_| {
                panic!("synthetic consumer panic")
            })
        }));
        assert!(outcome.is_ok());
        assert!(
            outcome
                .unwrap()
                .unwrap_err()
                .to_string()
                .contains("panicked")
        );
        assert_eq!(&usb.writes[6..], [0x06, 0x17]);
    }

    #[test]
    fn unrelated_success_reply_cannot_acquire_reservation() {
        let mut usb = synthetic();
        usb.replies[1] = reply(0x81);
        let error = run_job(
            &mut usb,
            request(),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("message"));
        assert_eq!(usb.writes, [0x12, 0x16]);
    }

    #[test]
    fn preparation_uses_the_scan_sessions_inquiry_before_any_reservation() {
        let mut usb = synthetic();
        let error = run_prepared_job(&mut usb, &AtomicBool::new(false), &mut |_| Ok(()), |raw| {
            assert_eq!(Capabilities::parse(raw).unwrap().identity, "synthetic");
            Err(io::Error::other("Evidence disk full"))
        })
        .unwrap_err();
        assert!(error.to_string().contains("Evidence disk full"));
        assert_eq!(usb.writes, [0x12]);
    }
    #[test]
    fn partial_raw_reads_assemble_exact_bytes_and_zero_progress_does_not_send_commands_into_data() {
        let mut usb = synthetic();
        let raw = usb.replies.remove(5).unwrap();
        usb.replies.insert(5, raw[..7].to_vec());
        usb.replies.insert(6, raw[7..].to_vec());
        assert_eq!(
            run_job(
                &mut usb,
                request(),
                &AtomicBool::new(false),
                &mut |_| Ok(())
            )
            .unwrap()
            .bytes,
            2
        );
        let mut usb = synthetic();
        usb.replies[5] = vec![];
        let error = run_job(
            &mut usb,
            request(),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("reconnect"));
        assert_eq!(usb.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29]);
    }
    #[test]
    fn cancellation_from_consumer_is_not_reported_as_complete_and_device_is_released() {
        let mut usb = synthetic();
        usb.replies.push_back(reply(0));
        let cancel = AtomicBool::new(false);
        let error = run_job(&mut usb, request(), &cancel, &mut |_| {
            cancel.store(true, Ordering::Relaxed);
            Ok(())
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(&usb.writes[6..], [0x06, 0x17]);
    }
    #[test]
    fn rejected_reservation_does_not_release_unowned_device_and_cleanup_failure_is_reported() {
        let mut usb = synthetic();
        usb.replies[1][1] = 4;
        assert!(
            run_job(
                &mut usb,
                request(),
                &AtomicBool::new(false),
                &mut |_| Ok(())
            )
            .is_err()
        );
        assert_eq!(usb.writes, [0x12, 0x16]);
        let mut usb = synthetic();
        usb.replies[6][1] = 8;
        let error = run_job(
            &mut usb,
            request(),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("Cleanup unconfirmed"));
    }
    #[test]
    fn multiple_bands_are_streamed_in_order_and_changed_width_is_rejected() {
        let mut usb = synthetic();
        let mut linked = usb.replies[4].clone();
        linked[3] = 0x80;
        usb.replies.insert(4, linked);
        usb.replies.insert(5, vec![20; 18]);
        let summary = run_job(
            &mut usb,
            request(),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .unwrap();
        assert_eq!((summary.bands, summary.height, summary.bytes), (2, 2, 4));
        let mut usb = synthetic();
        let mut linked = usb.replies[4].clone();
        linked[3] = 0x80;
        usb.replies.insert(4, linked);
        usb.replies.insert(5, vec![20; 18]);
        usb.replies[6][11] = 3;
        // Rejected metadata means READ_IMAGE is never sent; no raw data is queued.
        usb.replies.remove(7);
        usb.replies.push_back(reply(0));
        let error = run_job(
            &mut usb,
            request(),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert!(error.to_string().contains("changes width"));
        assert_eq!(&usb.writes[7..], [0x06, 0x17]);
    }
    #[test]
    fn framing_and_status_do_not_treat_unknown_device_errors_as_success() {
        for b in [
            vec![],
            vec![0xa8, 0, 29],
            vec![0xa8, 0, 0, 0],
            vec![0, 0, 29, 0],
        ] {
            assert!(response_status(&b, false).is_err());
        }
        let mut b = reply(0x20);
        b[1] = 8;
        assert_eq!(response_status(&b, false).unwrap(), ReplyStatus::Busy);
        b[1] = 2;
        b[4..6].copy_from_slice(&0x80u16.to_be_bytes());
        assert_eq!(response_status(&b, false).unwrap(), ReplyStatus::Busy);
        b[4..6].copy_from_slice(&0x40u16.to_be_bytes());
        assert!(response_status(&b, false).is_err());
        b[1] = 0xff;
        assert!(response_status(&b, false).is_err());
    }
    #[test]
    fn band_metadata_bounds_and_rgb_reordering_preserve_channel_values() {
        let mut b = reply(0x81);
        b[4..8].copy_from_slice(&22u32.to_be_bytes());
        b[8..10].copy_from_slice(&1u16.to_be_bytes());
        b[10..12].copy_from_slice(&2u16.to_be_bytes());
        let band = BandInfo::parse(&b, ColorMode::Rgb).unwrap();
        let mut wire = vec![10, 20, 30, 40, 50, 60];
        wire.extend_from_slice(&[0; 16]);
        assert_eq!(
            band.decode(&wire, ColorMode::Rgb, 1).unwrap(),
            [10, 30, 50, 20, 40, 60]
        );
        assert!(band.decode(&wire[..21], ColorMode::Rgb, 1).is_err());
        b[4..8].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(BandInfo::parse(&b, ColorMode::Rgb).is_err());
        b[4..8].copy_from_slice(&5u32.to_be_bytes());
        assert!(BandInfo::parse(&b, ColorMode::Rgb).is_err());
        b[3] = 0x10;
        assert!(BandInfo::parse(&b, ColorMode::Rgb).is_err());
    }
}
