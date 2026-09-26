//! Bounded flatbed scan jobs. Wire field provenance is recorded in ENG.md.

use crate::protocol::Capabilities;
use std::{fmt, io};

/// An explicit host cancellation checkpoint, distinct from interrupted I/O.
#[derive(Debug)]
pub(crate) struct HostCancelled;

impl fmt::Display for HostCancelled {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Scan cancelled")
    }
}
impl std::error::Error for HostCancelled {}

#[derive(Debug)]
struct DiagnosticError {
    original: io::Error,
    context: String,
}
impl fmt::Display for DiagnosticError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.context, self.original)
    }
}
impl std::error::Error for DiagnosticError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.original)
    }
}

pub(crate) fn is_host_cancelled(error: &io::Error) -> bool {
    let Some(inner) = error.get_ref() else {
        return false;
    };
    if inner.is::<HostCancelled>() {
        return true;
    }
    inner
        .downcast_ref::<DiagnosticError>()
        .is_some_and(|detail| is_host_cancelled(&detail.original))
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum SessionHealth {
    Ready,
    NeedsReconnect,
}

/// Host-observed USB API durations, including device waiting and failed calls.
/// These overlap the stage totals; they do not measure physical bus bandwidth.
#[derive(Debug, Default)]
pub struct UsbProfile {
    pub read_calls: u64,
    pub write_calls: u64,
    pub read_time: Duration,
    pub write_time: Duration,
}

/// Development measurements, retained on both success and failure.
/// Stage names match failure diagnostics. Stage times partition the job after
/// opening USB; USB and Busy sleep times are subsets, not additional costs.
/// Consumer time includes whatever the caller does (verification, output, etc.).
/// This record contains no device identity or image bytes.
#[derive(Debug, Default)]
pub struct ScanProfile {
    pub stages: std::collections::BTreeMap<&'static str, Duration>,
    pub usb: UsbProfile,
    pub read_buffer_bytes: usize,
    pub usb_read_limit: Option<usize>,
    pub busy_replies: u64,
    pub busy_sleep: Duration,
    pub total: Duration,
}

/// Development-only transfer experiment. Defaults preserve 100 ms READ Busy
/// polling and 64 KiB reads. Buffer sizes must be multiples of 1024 bytes in
/// 1 KiB..=1 MiB and no larger than the current WinUSB pipe's reported limit.
/// Larger reads can defer cancellation until the active USB call returns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanTuning {
    pub read_poll_interval: Duration,
    pub read_buffer_bytes: usize,
}

impl Default for ScanTuning {
    fn default() -> Self {
        Self {
            read_poll_interval: Duration::from_millis(100),
            read_buffer_bytes: 65_536,
        }
    }
}

/// Preserve callers that pass only the READ Busy interval.
impl From<Duration> for ScanTuning {
    fn from(read_poll_interval: Duration) -> Self {
        Self {
            read_poll_interval,
            ..Self::default()
        }
    }
}

impl ScanTuning {
    fn validate(self) -> io::Result<()> {
        validate_poll_interval(self.read_poll_interval)?;
        if !(1024..=1024 * 1024).contains(&self.read_buffer_bytes)
            || !self.read_buffer_bytes.is_multiple_of(1024)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Read buffer must be a multiple of 1024 bytes within 1 KiB..=1 MiB",
            ));
        }
        Ok(())
    }
}

#[cfg(any(windows, test))]
fn validate_read_buffer_limit(requested: usize, limit: usize) -> io::Result<()> {
    if requested > limit {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Read buffer {requested} exceeds the WinUSB pipe limit {limit}"),
        ));
    }
    Ok(())
}

#[cfg(any(windows, test))]
fn record_read_buffer_limit(
    profile: &mut ScanProfile,
    requested: usize,
    limit: usize,
) -> io::Result<()> {
    profile.read_buffer_bytes = requested;
    profile.usb_read_limit = Some(limit);
    validate_read_buffer_limit(requested, limit)
}

#[cfg(windows)]
fn preflight_read_buffer(
    usb: &crate::usb::UsbSession,
    requested: usize,
    profile: &mut ScanProfile,
) -> io::Result<usize> {
    let limit = usb.maximum_read_transfer_size()?;
    record_read_buffer_limit(profile, requested, limit)?;
    Ok(limit)
}

#[derive(Clone, Copy)]
enum ScanStage {
    Inquiry,
    Prepare,
    Reserve,
    SetWindow,
    Start,
    ReadMetadata,
    ReadImage,
    Decode,
    Consumer,
    Cleanup,
}

impl ScanStage {
    fn name(self) -> &'static str {
        match self {
            Self::Inquiry => "inquiry",
            Self::Prepare => "prepare",
            Self::Reserve => "reserve",
            Self::SetWindow => "set-window",
            Self::Start => "start",
            Self::ReadMetadata => "read-metadata",
            Self::ReadImage => "read-image",
            Self::Decode => "decode",
            Self::Consumer => "consumer",
            Self::Cleanup => "cleanup",
        }
    }
}

struct ScanDiagnostics {
    started: Instant,
    stage: ScanStage,
    completed_bands: u32,
    pixel_bytes: usize,
    opcode: Option<u8>,
    busy_replies: u32,
    last_busy_status: Option<u8>,
    last_busy_state: Option<u16>,
    stage_started: Instant,
    profile: ScanProfile,
    read_poll_interval: Duration,
    read_buffer_bytes: usize,
}

impl ScanDiagnostics {
    fn new() -> Self {
        Self {
            started: Instant::now(),
            stage: ScanStage::Inquiry,
            completed_bands: 0,
            pixel_bytes: 0,
            opcode: None,
            busy_replies: 0,
            last_busy_status: None,
            last_busy_state: None,
            stage_started: Instant::now(),
            profile: ScanProfile::default(),
            read_poll_interval: Duration::from_millis(100),
            read_buffer_bytes: 65_536,
        }
    }

    fn set_stage(&mut self, stage: ScanStage) {
        let now = Instant::now();
        *self.profile.stages.entry(self.stage.name()).or_default() += now - self.stage_started;
        self.stage_started = now;
        self.stage = stage;
    }

    fn record_band(&mut self, bands: u32, pixel_bytes: usize) {
        self.completed_bands = bands;
        self.pixel_bytes = pixel_bytes;
    }

    fn set_opcode(&mut self, opcode: u8) {
        self.opcode = Some(opcode);
        self.busy_replies = 0;
        self.last_busy_status = None;
        self.last_busy_state = None;
    }

    fn record_busy_reply(&mut self, opcode: u8, reply: &[u8], read: bool) {
        let Some(status) = reply.get(1).copied() else {
            return;
        };
        if !matches!(status, 0x02 | 0x08) {
            return;
        }
        self.opcode = Some(opcode);
        self.busy_replies = self.busy_replies.saturating_add(1);
        self.profile.busy_replies = self.profile.busy_replies.saturating_add(1);
        self.last_busy_status = Some(status);
        self.last_busy_state = None;
        if status == 0x02 && reply.get(3) == Some(&0x20) {
            let offset = if read { 12 } else { 4 };
            self.last_busy_state = reply
                .get(offset)
                .zip(reply.get(offset + 1))
                .map(|(&high, &low)| u16::from_be_bytes([high, low]));
        }
    }

    fn error(&self, error: io::Error, detail: impl Into<String>) -> io::Error {
        let detail = detail.into();
        let detail = if detail.is_empty() {
            String::new()
        } else {
            format!(" {detail}")
        };
        io::Error::new(
            error.kind(),
            DiagnosticError {
                context: format!(
                    "stage={} elapsed_ms={} completed_bands={} pixel_bytes={} opcode={} last_busy_status={} last_busy_state={} busy_replies={}{}",
                    self.stage.name(),
                    self.started.elapsed().as_millis(),
                    self.completed_bands,
                    self.pixel_bytes,
                    match self.opcode {
                        Some(opcode) => format!("0x{opcode:02x}"),
                        None => "unknown".into(),
                    },
                    match self.last_busy_status {
                        Some(status) => format!("0x{status:02x}"),
                        None => "unknown".into(),
                    },
                    match self.last_busy_state {
                        Some(state) => format!("0x{state:04x}"),
                        None => "unknown".into(),
                    },
                    self.busy_replies,
                    detail,
                ),
                original: error,
            },
        )
    }
}

trait Transport {
    fn write(&mut self, data: &[u8]) -> io::Result<()>;
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize>;
}

struct TimedTransport<'a, T> {
    inner: &'a mut T,
    metrics: UsbProfile,
}

impl<T: Transport> Transport for TimedTransport<'_, T> {
    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        let started = Instant::now();
        let result = self.inner.write(data);
        self.metrics.write_time += started.elapsed();
        self.metrics.write_calls = self.metrics.write_calls.saturating_add(1);
        result
    }
    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        let started = Instant::now();
        let result = self.inner.read(data);
        self.metrics.read_time += started.elapsed();
        self.metrics.read_calls = self.metrics.read_calls.saturating_add(1);
        result
    }
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
    sink: impl FnMut(&ImageBand) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    scan_with_evidence(cancel, |_| Ok(request), sink)
}

#[cfg(windows)]
pub(crate) fn scan_in_session(
    usb: &mut crate::usb::UsbSession,
    request: ScanRequest,
    cancel: &AtomicBool,
    mut sink: impl FnMut(&ImageBand) -> io::Result<()>,
) -> (io::Result<ScanSummary>, SessionHealth) {
    if cancel.load(Ordering::Relaxed) {
        return (
            Err(io::Error::new(io::ErrorKind::Interrupted, HostCancelled)),
            SessionHealth::Ready,
        );
    }
    let mut profile = ScanProfile::default();
    let read_limit =
        match preflight_read_buffer(usb, ScanTuning::default().read_buffer_bytes, &mut profile) {
            Ok(limit) => limit,
            Err(error) => return (Err(error), SessionHealth::Ready),
        };
    let result = run_prepared_job_profiled_with_health(
        usb,
        cancel,
        &mut sink,
        |_| Ok(request),
        &mut profile,
        ScanTuning::default(),
    );
    profile.usb_read_limit = Some(read_limit);
    result
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
    sink: impl FnMut(&ImageBand) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    scan_with_profile(
        cancel,
        prepare,
        sink,
        &mut ScanProfile::default(),
        Duration::from_millis(100),
    )
}

/// Development-only timing and transfer experiment.
/// `profile` is reset before validation and retained on every ordinary return.
/// `tuning` accepts a READ Busy Duration (1..=1000 ms, 64 KiB buffer) or
/// `ScanTuning` with a bounded read buffer size. Other commands retain 100 ms, and all scan
/// settings, transfer policies and job/drain deadlines remain unchanged.
/// Parameter validation and pre-cancellation precede USB open. The live pipe
/// limit is checked before INQUIRY or RESERVE. Open and limit-query time are
/// excluded; their failures have no stage timings. No image or identity is recorded.
#[cfg(windows)]
pub fn scan_with_profile(
    cancel: &AtomicBool,
    prepare: impl FnOnce(&crate::InquiryEvidence) -> io::Result<ScanRequest>,
    mut sink: impl FnMut(&ImageBand) -> io::Result<()>,
    profile: &mut ScanProfile,
    tuning: impl Into<ScanTuning>,
) -> io::Result<ScanSummary> {
    *profile = ScanProfile::default();
    let tuning = tuning.into();
    tuning.validate()?;
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, HostCancelled));
    }
    let mut usb = crate::usb::UsbSession::open()?;
    let read_limit = preflight_read_buffer(&usb, tuning.read_buffer_bytes, profile)?;
    let mut evidence = crate::InquiryEvidence {
        device_descriptor: usb.descriptor,
        interface_descriptor: usb.interface,
        bulk_in: usb.bulk_in,
        bulk_out: usb.bulk_out,
        bulk_in_max_packet: usb.bulk_in_max_packet,
        bulk_out_max_packet: usb.bulk_out_max_packet,
        reply: Vec::new(),
    };
    let result = run_prepared_job_profiled(
        &mut usb,
        cancel,
        &mut sink,
        |reply| {
            evidence.reply = reply.to_vec();
            prepare(&evidence)
        },
        profile,
        tuning,
    );
    profile.usb_read_limit = Some(read_limit);
    result
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

#[cfg(not(windows))]
pub fn scan_with_profile(
    _: &AtomicBool,
    _: impl FnOnce(&crate::InquiryEvidence) -> io::Result<ScanRequest>,
    _: impl FnMut(&ImageBand) -> io::Result<()>,
    profile: &mut ScanProfile,
    tuning: impl Into<ScanTuning>,
) -> io::Result<ScanSummary> {
    *profile = ScanProfile::default();
    tuning.into().validate()?;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Scanning requires Windows",
    ))
}

fn validate_poll_interval(interval: Duration) -> io::Result<()> {
    if !(Duration::from_millis(1)..=Duration::from_millis(1000)).contains(&interval) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "READ poll interval must be within 1..=1000 ms",
        ));
    }
    Ok(())
}

fn busy_poll_interval(opcode: u8, read_interval: Duration) -> Duration {
    if opcode == 0x28 {
        read_interval
    } else {
        Duration::from_millis(100)
    }
}

fn checkpoint_deadline(deadline: Instant) -> io::Result<()> {
    if Instant::now() >= deadline {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Scan exceeded 120 second deadline",
        ))
    } else {
        Ok(())
    }
}

fn checkpoint(cancel: &AtomicBool, deadline: Instant) -> io::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(io::Error::new(io::ErrorKind::Interrupted, HostCancelled))
    } else {
        checkpoint_deadline(deadline)
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
    let advertised_length = b
        .get(2)
        .map(|length| usize::from(*length).saturating_add(3));
    if n < 32 || b.first() != Some(&0xa8) || advertised_length != Some(n) {
        let expected_length = advertised_length
            .map(|length| length.to_string())
            .unwrap_or_else(|| "unavailable".into());
        return Err(invalid(format!(
            "Command 0x{:02x}: malformed reply received={n} expected_framing=header(0xa8)+length+3 expected_length={expected_length}; stream synchronization lost",
            command[2],
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
    diagnostics: &mut ScanDiagnostics,
) -> io::Result<Vec<u8>> {
    ready_with_attempt(
        usb,
        opcode,
        synchronized,
        Some(cancel),
        deadline,
        diagnostics,
        &mut false,
    )
}

fn ready_with_attempt(
    usb: &mut impl Transport,
    opcode: u8,
    synchronized: &mut bool,
    cancel: Option<&AtomicBool>,
    deadline: Instant,
    diagnostics: &mut ScanDiagnostics,
    attempted: &mut bool,
) -> io::Result<Vec<u8>> {
    *attempted = false;
    diagnostics.set_opcode(opcode);
    loop {
        if let Some(cancel) = cancel {
            checkpoint(cancel, deadline)?;
        } else {
            checkpoint_deadline(deadline)?;
        }
        // Set before write: even a failed USB call may have reached the device.
        // Keep true across Busy replies and later cancellation checkpoints.
        *attempted = true;
        let b = exchange(usb, &[0x1b, 0xa8, opcode, 0], synchronized)?;
        match response_status(&b, opcode == 0x28)? {
            ReplyStatus::Good => return Ok(b),
            ReplyStatus::Busy => {
                diagnostics.record_busy_reply(opcode, &b, opcode == 0x28);
            }
        }
        let sleeping = Instant::now();
        std::thread::sleep(busy_poll_interval(opcode, diagnostics.read_poll_interval));
        diagnostics.profile.busy_sleep += sleeping.elapsed();
    }
}

fn finish(
    usb: &mut impl Transport,
    abort: bool,
    synchronized: &mut bool,
    diagnostics: &mut ScanDiagnostics,
) -> io::Result<()> {
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
        diagnostics.set_opcode(*opcode);
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

#[cfg(test)]
fn run_job(
    usb: &mut impl Transport,
    request: ScanRequest,
    cancel: &AtomicBool,
    sink: &mut impl FnMut(&ImageBand) -> io::Result<()>,
) -> io::Result<ScanSummary> {
    run_prepared_job(usb, cancel, sink, |_| Ok(request))
}

#[cfg(test)]
fn run_job_with_health(
    usb: &mut impl Transport,
    request: ScanRequest,
    cancel: &AtomicBool,
    sink: &mut impl FnMut(&ImageBand) -> io::Result<()>,
) -> (io::Result<ScanSummary>, SessionHealth) {
    let mut profile = ScanProfile::default();
    run_prepared_job_profiled_with_health(
        usb,
        cancel,
        sink,
        |_| Ok(request),
        &mut profile,
        ScanTuning::default(),
    )
}

/// Drain only data whose exact remaining length is known. An opaque USB error
/// may have consumed unknown bytes, so it cannot be recovered by this routine.
fn read_image_band(
    usb: &mut impl Transport,
    length: usize,
    cancel: &AtomicBool,
    deadline: Instant,
    synchronized: &mut bool,
    diagnostics: &mut ScanDiagnostics,
) -> io::Result<Vec<u8>> {
    diagnostics.set_stage(ScanStage::ReadImage);
    let failed = |original: &Option<io::Error>,
                  error: io::Error,
                  received: usize,
                  last_progress: Instant| {
        let error = match original {
            Some(original) => io::Error::new(
                original.kind(),
                format!("{original}; image drain failed: {error}"),
            ),
            None => error,
        };
        diagnostics.error(
            error,
            format!(
                "received={received}/{length} since_progress_ms={}",
                last_progress.elapsed().as_millis()
            ),
        )
    };
    *synchronized = false;
    let mut raw = Vec::with_capacity(length);
    let mut buffer = vec![0; diagnostics.read_buffer_bytes];
    let mut failure = None;
    let mut drain_deadline = None;
    let mut empty_reads = 0;
    let mut last_progress = Instant::now();
    while raw.len() < length {
        if failure.is_none() {
            failure = checkpoint(cancel, deadline).err();
        }
        if failure.is_some() && drain_deadline.is_none() {
            drain_deadline = Some(Instant::now() + Duration::from_secs(10));
        }
        if drain_deadline.is_some_and(|end| Instant::now() >= end) {
            return Err(failed(
                &failure,
                io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Image drain exceeded 10 seconds; stream synchronization lost",
                ),
                raw.len(),
                last_progress,
            ));
        }
        let remaining = length - raw.len();
        let capacity = remaining
            .div_ceil(1024)
            .saturating_mul(1024)
            .min(buffer.len());
        let count = match usb.read(&mut buffer[..capacity]) {
            Ok(count) => count,
            Err(error) => return Err(failed(&failure, error, raw.len(), last_progress)),
        };
        if count > capacity || count > remaining {
            return Err(failed(
                &failure,
                invalid("Image read exceeded advertised band length; stream synchronization lost"),
                raw.len(),
                last_progress,
            ));
        }
        if count == 0 {
            empty_reads += 1;
            if failure.is_none() {
                failure = Some(invalid("Image read made no progress"));
            }
            if empty_reads >= 2 {
                return Err(failed(
                    &failure,
                    invalid("Image drain made no progress twice; stream synchronization lost"),
                    raw.len(),
                    last_progress,
                ));
            }
            continue;
        }
        raw.extend_from_slice(&buffer[..count]);
        last_progress = Instant::now();
    }
    *synchronized = true;
    if let Some(error) = failure {
        return Err(diagnostics.error(
            error,
            format!(
                "received={}/{length} since_progress_ms={}",
                raw.len(),
                last_progress.elapsed().as_millis()
            ),
        ));
    }
    checkpoint(cancel, deadline).map_err(|error| {
        diagnostics.error(
            error,
            format!(
                "received={}/{length} since_progress_ms={}",
                raw.len(),
                last_progress.elapsed().as_millis()
            ),
        )
    })?;
    Ok(raw)
}

#[cfg(test)]
fn run_prepared_job(
    usb: &mut impl Transport,
    cancel: &AtomicBool,
    sink: &mut impl FnMut(&ImageBand) -> io::Result<()>,
    prepare: impl FnOnce(&[u8]) -> io::Result<ScanRequest>,
) -> io::Result<ScanSummary> {
    run_prepared_job_profiled(
        usb,
        cancel,
        sink,
        prepare,
        &mut ScanProfile::default(),
        Duration::from_millis(100),
    )
}

fn run_prepared_job_profiled(
    usb: &mut impl Transport,
    cancel: &AtomicBool,
    sink: &mut impl FnMut(&ImageBand) -> io::Result<()>,
    prepare: impl FnOnce(&[u8]) -> io::Result<ScanRequest>,
    profile: &mut ScanProfile,
    tuning: impl Into<ScanTuning>,
) -> io::Result<ScanSummary> {
    run_prepared_job_profiled_with_health(usb, cancel, sink, prepare, profile, tuning).0
}

fn run_prepared_job_profiled_with_health(
    usb: &mut impl Transport,
    cancel: &AtomicBool,
    sink: &mut impl FnMut(&ImageBand) -> io::Result<()>,
    prepare: impl FnOnce(&[u8]) -> io::Result<ScanRequest>,
    profile: &mut ScanProfile,
    tuning: impl Into<ScanTuning>,
) -> (io::Result<ScanSummary>, SessionHealth) {
    *profile = ScanProfile::default();
    let tuning = tuning.into();
    if let Err(error) = tuning.validate() {
        return (Err(error), SessionHealth::Ready);
    }
    let mut diagnostics = ScanDiagnostics::new();
    diagnostics.read_poll_interval = tuning.read_poll_interval;
    diagnostics.read_buffer_bytes = tuning.read_buffer_bytes;
    diagnostics.profile.read_buffer_bytes = tuning.read_buffer_bytes;
    let mut timed = TimedTransport {
        inner: usb,
        metrics: UsbProfile::default(),
    };
    let (result, health) =
        run_prepared_job_observed_with_health(&mut timed, cancel, sink, prepare, &mut diagnostics);
    diagnostics.set_stage(diagnostics.stage);
    diagnostics.profile.total = diagnostics.started.elapsed();
    diagnostics.profile.usb = timed.metrics;
    *profile = diagnostics.profile;
    (result, health)
}

fn run_prepared_job_observed_with_health(
    usb: &mut impl Transport,
    cancel: &AtomicBool,
    sink: &mut impl FnMut(&ImageBand) -> io::Result<()>,
    prepare: impl FnOnce(&[u8]) -> io::Result<ScanRequest>,
    diagnostics: &mut ScanDiagnostics,
) -> (io::Result<ScanSummary>, SessionHealth) {
    let deadline = Instant::now() + Duration::from_secs(120);
    diagnostics.set_stage(ScanStage::Inquiry);
    diagnostics.set_opcode(0x12);
    if let Err(error) = checkpoint(cancel, deadline) {
        return (
            Err(diagnostics.error(error, "before inquiry")),
            SessionHealth::Ready,
        );
    }
    let mut synchronized = true;
    let raw = match exchange(usb, &[0x1b, 0xa8, 0x12, 0], &mut synchronized) {
        Ok(raw) => raw,
        Err(error) => {
            return (
                Err(diagnostics.error(error, "INQUIRY")),
                SessionHealth::NeedsReconnect,
            );
        }
    };
    let caps = match Capabilities::parse(&raw) {
        Ok(caps) => caps,
        Err(error) => {
            return (
                Err(diagnostics.error(error, "capability response")),
                SessionHealth::NeedsReconnect,
            );
        }
    };
    diagnostics.set_stage(ScanStage::Prepare);
    diagnostics.set_opcode(0x24);
    let request = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| prepare(&raw))) {
        Ok(Ok(request)) => request,
        Ok(Err(error)) => {
            return (
                Err(diagnostics.error(error, "scan request")),
                SessionHealth::Ready,
            );
        }
        Err(_) => {
            return (
                Err(diagnostics.error(
                    io::Error::other("Scan preparation panicked"),
                    "scan request",
                )),
                SessionHealth::NeedsReconnect,
            );
        }
    };
    let window = match request.command(&caps) {
        Ok(window) => window,
        Err(error) => {
            return (
                Err(diagnostics.error(error, "SET_WINDOW command construction")),
                SessionHealth::Ready,
            );
        }
    };
    diagnostics.set_stage(ScanStage::Reserve);
    let mut reservation_attempted = false;
    let reservation = ready_with_attempt(
        usb,
        0x16,
        &mut synchronized,
        Some(cancel),
        deadline,
        diagnostics,
        &mut reservation_attempted,
    )
    .map_err(|error| {
        if synchronized {
            error
        } else {
            io::Error::new(
                error.kind(),
                format!("{error}; reservation unconfirmed; reconnect scanner before another job"),
            )
        }
    });
    if let Err(error) = reservation {
        return (
            Err(diagnostics.error(error, "RESERVE")),
            if reservation_attempted {
                SessionHealth::NeedsReconnect
            } else {
                SessionHealth::Ready
            },
        );
    }
    // Ownership is confirmed only after RESERVE succeeds. Never release another owner's busy device.
    let mut first_metadata_failed_after_deferred_cancel = false;
    let result = (|| {
        diagnostics.set_stage(ScanStage::SetWindow);
        diagnostics.set_opcode(0x24);
        if let Err(error) = checkpoint(cancel, deadline) {
            return Err(diagnostics.error(error, "before SET_WINDOW"));
        }
        let window_reply = match exchange(usb, &window, &mut synchronized) {
            Ok(reply) => reply,
            Err(error) => return Err(diagnostics.error(error, "SET_WINDOW")),
        };
        if let Err(error) = response_status(&window_reply, false) {
            return Err(diagnostics.error(error, "SET_WINDOW response"));
        }
        diagnostics.set_stage(ScanStage::Start);
        if let Err(error) = ready(usb, 0x31, &mut synchronized, cancel, deadline, diagnostics) {
            return Err(diagnostics.error(error, "START"));
        }
        let mut summary = ScanSummary {
            width: 0,
            height: 0,
            bands: 0,
            bytes: 0,
        };
        loop {
            diagnostics.set_stage(ScanStage::ReadMetadata);
            // START is confirmed here. Defer host cancellation only until the first
            // band metadata so READ_IMAGE can drain through the normal bounded path.
            let b = match ready_with_attempt(
                usb,
                0x28,
                &mut synchronized,
                (summary.bands != 0).then_some(cancel),
                deadline,
                diagnostics,
                &mut false,
            ) {
                Ok(reply) => reply,
                Err(error) => {
                    if summary.bands == 0 && cancel.load(Ordering::Relaxed) {
                        first_metadata_failed_after_deferred_cancel = true;
                        return Err(diagnostics.error(
                            error,
                            "READ metadata after host cancellation; reconnect scanner before another scan",
                        ));
                    }
                    return Err(diagnostics.error(error, "READ metadata"));
                }
            };
            let band = match BandInfo::parse(&b, request.mode) {
                Ok(band) => band,
                Err(error) => return Err(diagnostics.error(error, "READ metadata response")),
            };
            if summary.bands >= 4096
                || summary
                    .bytes
                    .checked_add(band.length)
                    .is_none_or(|bytes| bytes > 256 * 1024 * 1024)
                || (summary.width != 0 && summary.width != band.width as u32)
            {
                return Err(diagnostics.error(
                    invalid("Image exceeds job limits or changes width between bands"),
                    "band limits",
                ));
            }
            synchronized = false;
            diagnostics.set_stage(ScanStage::ReadImage);
            diagnostics.set_opcode(0x29);
            if let Err(error) = usb.write(&[0x1b, 0xa8, 0x29, 0]) {
                return Err(diagnostics.error(error, "READ image"));
            }
            let raw = match read_image_band(
                usb,
                band.length,
                cancel,
                deadline,
                &mut synchronized,
                diagnostics,
            ) {
                Ok(raw) => raw,
                Err(error) => return Err(error),
            };
            diagnostics.set_stage(ScanStage::Decode);
            let pixels = match band.decode(&raw, request.mode, caps.line_order) {
                Ok(pixels) => pixels,
                Err(error) => return Err(diagnostics.error(error, "image decode")),
            };
            let next_height = summary
                .height
                .checked_add(band.rows as u32)
                .ok_or_else(|| {
                    diagnostics.error(invalid("Image height overflow"), "image decode")
                })?;
            let next_bytes = summary.bytes.checked_add(pixels.len()).ok_or_else(|| {
                diagnostics.error(invalid("Image pixel byte count overflow"), "image decode")
            })?;
            let image = ImageBand {
                width: band.width as u32,
                rows: band.rows as u32,
                mode: request.mode,
                pixels,
                wire_data: raw,
            };
            // A panicking consumer is never called again. The band is fully drained,
            // so the error path can safely cancel and release our reservation.
            diagnostics.set_stage(ScanStage::Consumer);
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| sink(&image))) {
                Ok(Ok(())) => {}
                Ok(Err(error)) => return Err(diagnostics.error(error, "image consumer")),
                Err(_) => {
                    return Err(diagnostics.error(
                        io::Error::other("Image consumer panicked; job cancelled"),
                        "image consumer",
                    ));
                }
            }
            summary.width = band.width as u32;
            summary.height = next_height;
            summary.bands += 1;
            summary.bytes = next_bytes;
            diagnostics.record_band(summary.bands, summary.bytes);
            if let Err(error) = checkpoint(cancel, deadline) {
                return Err(diagnostics.error(error, "after image consumer"));
            }
            if band.final_band {
                return Ok(summary);
            }
        }
    })();
    let cleanup_attempted = synchronized;
    diagnostics.set_stage(ScanStage::Cleanup);
    let cleanup = finish(usb, result.is_err(), &mut synchronized, diagnostics);
    let cleanup = cleanup.map_err(|error| diagnostics.error(error, "cleanup"));
    let health = if cleanup.is_ok() && !first_metadata_failed_after_deferred_cancel {
        SessionHealth::Ready
    } else {
        SessionHealth::NeedsReconnect
    };
    let output = match (result, cleanup) {
        (Ok(summary), Ok(())) => Ok(summary),
        (Err(e), Ok(())) | (Ok(_), Err(e)) => Err(e),
        (Err(e), Err(cleanup)) => {
            let suffix = if cleanup_attempted {
                "cleanup secondary error"
            } else {
                "cleanup skipped"
            };
            Err(io::Error::new(
                e.kind(),
                format!("{e}; {suffix}: {cleanup}"),
            ))
        }
    };
    (output, health)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_cancellation_survives_diagnostics_without_matching_unrelated_interruptions() {
        let error = checkpoint(&AtomicBool::new(true), Instant::now()).unwrap_err();
        assert!(is_host_cancelled(&error));
        let diagnostics = ScanDiagnostics::new();
        let error = diagnostics.error(error, "synthetic checkpoint");
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(is_host_cancelled(&error));
        assert!(
            error
                .to_string()
                .contains("synthetic checkpoint: Scan cancelled")
        );
        let unrelated = io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled");
        assert!(!is_host_cancelled(
            &diagnostics.error(unrelated, "synthetic consumer")
        ));
    }

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
    fn wia_pixel_settings_reach_window_and_are_checked_against_live_inquiry() {
        use crate::wia::{BMP_FORMAT, FlatbedSettings};
        let settings = FlatbedSettings {
            x_resolution: 150,
            y_resolution: 150,
            x_position: 15,
            y_position: 30,
            x_extent: 150,
            y_extent: 300,
            data_type: 2,
            depth: 8,
            brightness: 0,
            contrast: 0,
            compression: 0,
            format: BMP_FORMAT,
        };
        let mapped = settings.to_request().unwrap();
        assert_eq!(
            mapped.command(&caps()).unwrap(),
            [
                0x1b, 0xa8, 0x24, 0x13, 0x30, 0, 0, 4, 0xb0, 0, 0, 9, 0x60, 2, 2, 0, 10, 0, 20, 3,
                0, 0, 2, 0x40, 0,
            ]
        );
        let mut usb = synthetic();
        let summary = run_job(&mut usb, mapped, &AtomicBool::new(false), &mut |_| Ok(())).unwrap();
        // Synthetic device delivers a different size; preserve measured data, never invent pixels.
        assert_eq!((summary.width, summary.height), (2, 1));
        assert_eq!(usb.writes.last(), Some(&0x17));
        for missing_mode in [false, true] {
            let mut usb = synthetic();
            let requested = if missing_mode {
                usb.replies.front_mut().unwrap()[39] = 1;
                mapped
            } else {
                FlatbedSettings {
                    x_extent: 2000,
                    ..settings
                }
                .to_request()
                .unwrap()
            };
            let result = run_job(&mut usb, requested, &AtomicBool::new(false), &mut |_| {
                panic!("rejected live capabilities must not deliver an image")
            });
            assert!(result.is_err());
            assert_eq!(
                usb.writes,
                [0x12],
                "capability rejection must precede RESERVE"
            );
        }
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
    struct InjectedReadFailure {
        inner: Synthetic,
        read_calls: usize,
        fail_call: usize,
    }
    impl Transport for InjectedReadFailure {
        fn write(&mut self, b: &[u8]) -> io::Result<()> {
            self.inner.write(b)
        }
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.read_calls += 1;
            if self.read_calls == self.fail_call {
                return Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "synthetic WinUSB error code=0xC0000001",
                ));
            }
            self.inner.read(b)
        }
    }
    struct BusyThenCancel<'a> {
        inner: Synthetic,
        cancel: &'a AtomicBool,
        read_calls: usize,
        cancel_on_read: usize,
    }
    impl Transport for BusyThenCancel<'_> {
        fn write(&mut self, b: &[u8]) -> io::Result<()> {
            self.inner.write(b)
        }
        fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
            self.read_calls += 1;
            let count = self.inner.read(b)?;
            if self.read_calls == self.cancel_on_read {
                self.cancel.store(true, Ordering::Relaxed);
            }
            Ok(count)
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
    fn shared_session_health_is_ready_after_successful_cleanup() {
        let mut usb = synthetic();
        let (result, health) = run_job_with_health(
            &mut usb,
            request(),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        );
        assert!(result.is_ok());
        assert_eq!(health, SessionHealth::Ready);
    }

    #[test]
    fn shared_session_health_is_ready_after_consumer_failure_and_cleanup() {
        let mut usb = synthetic();
        usb.replies.push_back(reply(0));
        let (result, health) =
            run_job_with_health(&mut usb, request(), &AtomicBool::new(false), &mut |_| {
                Err(io::Error::other("synthetic consumer failure"))
            });
        assert!(result.is_err());
        assert_eq!(health, SessionHealth::Ready);
        assert_eq!(usb.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29, 0x06, 0x17]);
    }

    #[test]
    fn shared_session_health_is_ready_after_cancellation_and_cleanup() {
        let mut usb = synthetic();
        usb.replies.push_back(reply(0));
        let cancel = AtomicBool::new(false);
        let (result, health) = run_job_with_health(&mut usb, request(), &cancel, &mut |_| {
            cancel.store(true, Ordering::Relaxed);
            Ok(())
        });
        let error = result.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(is_host_cancelled(&error));
        assert_eq!(health, SessionHealth::Ready);
        assert_eq!(usb.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29, 0x06, 0x17]);
    }

    #[test]
    fn shared_session_health_needs_reconnect_after_unknown_transfer_consumption() {
        let mut usb = InjectedReadFailure {
            inner: synthetic(),
            read_calls: 0,
            fail_call: 6,
        };
        let (result, health) = run_job_with_health(
            &mut usb,
            request(),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        );
        assert!(result.is_err());
        assert_eq!(health, SessionHealth::NeedsReconnect);
        assert_eq!(usb.inner.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29]);
    }

    #[test]
    fn shared_session_health_needs_reconnect_after_cleanup_failure() {
        let mut usb = synthetic();
        usb.replies.pop_back();
        usb.replies.push_back(reply(0));
        let (result, health) =
            run_job_with_health(&mut usb, request(), &AtomicBool::new(false), &mut |_| {
                Err(io::Error::other("synthetic consumer failure"))
            });
        assert!(result.is_err());
        assert_eq!(health, SessionHealth::NeedsReconnect);
        assert_eq!(usb.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29, 0x06, 0x17]);
    }

    #[test]
    fn shared_session_health_is_ready_for_pre_cancel_without_commands() {
        let mut usb = synthetic();
        let (result, health) =
            run_job_with_health(&mut usb, request(), &AtomicBool::new(true), &mut |_| Ok(()));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
        assert_eq!(health, SessionHealth::Ready);
        assert!(usb.writes.is_empty());
    }

    #[test]
    fn cancellation_around_reserve_preserves_only_an_unattempted_session() {
        // Synthetic cancellation at the INQUIRY/RESERVE boundary. Once RESERVE
        // has been sent, a Busy reply cannot prove that we own the device.
        for (cancel_on_read, expected_health, expected_writes) in [
            (1, SessionHealth::Ready, vec![0x12]),
            (2, SessionHealth::NeedsReconnect, vec![0x12, 0x16]),
        ] {
            let cancel = AtomicBool::new(false);
            let mut inner = synthetic();
            inner.replies[1][1] = 0x08;
            let mut usb = BusyThenCancel {
                inner,
                cancel: &cancel,
                read_calls: 0,
                cancel_on_read,
            };
            let (result, health) = run_job_with_health(&mut usb, request(), &cancel, &mut |_| {
                panic!("cancelled reservation must not deliver pixels")
            });
            assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
            assert_eq!(health, expected_health);
            assert_eq!(usb.inner.writes, expected_writes);
        }
    }

    #[test]
    fn shared_session_health_is_ready_for_pre_reservation_parameter_rejection() {
        let mut usb = synthetic();
        let invalid_request = ScanRequest {
            width_units: 10_200,
            ..request()
        };
        let (result, health) = run_job_with_health(
            &mut usb,
            invalid_request,
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        );
        assert!(result.is_err());
        assert_eq!(health, SessionHealth::Ready);
        assert_eq!(usb.writes, [0x12]);
    }

    #[test]
    fn shared_session_health_is_ready_for_preparation_rejection_after_inquiry() {
        let mut usb = synthetic();
        let mut profile = ScanProfile::default();
        let (result, health) = run_prepared_job_profiled_with_health(
            &mut usb,
            &AtomicBool::new(false),
            &mut |_| Ok(()),
            |_| Err(io::Error::other("synthetic parameter rejection")),
            &mut profile,
            ScanTuning::default(),
        );
        assert!(result.is_err());
        assert_eq!(health, SessionHealth::Ready);
        assert_eq!(usb.writes, [0x12]);
    }

    #[test]
    fn bitmap_consumer_only_finalizes_after_successful_scanner_release() {
        use crate::bitmap::BmpEncoder;
        use std::io::Cursor;

        for outcome in ["success", "cancel", "release-error"] {
            let mut usb = synthetic();
            if outcome == "cancel" {
                usb.replies.push_back(reply(0));
            } else if outcome == "release-error" {
                usb.replies.pop_back();
            }
            let cancel = AtomicBool::new(false);
            let mut stream = Cursor::new(Vec::new());
            let result = (|| {
                let mut encoder = BmpEncoder::new(&mut stream, 150, ColorMode::Gray)?;
                let summary = run_job(&mut usb, request(), &cancel, &mut |band| {
                    encoder.push(band)?;
                    if outcome == "cancel" {
                        cancel.store(true, Ordering::Relaxed);
                    }
                    Ok(())
                })?;
                assert_eq!(usb.writes.last(), Some(&0x17));
                encoder.finish(&summary)
            })();
            if outcome == "success" {
                result.unwrap();
                assert_eq!(&stream.get_ref()[..2], b"BM");
                assert_eq!(&stream.get_ref()[1078..], &[42, 190, 0, 0]);
            } else {
                assert!(result.is_err());
                assert_ne!(&stream.get_ref()[..2], b"BM");
                if outcome == "cancel" {
                    assert_eq!(result.unwrap_err().kind(), io::ErrorKind::Interrupted);
                    assert!(usb.writes.ends_with(&[0x06, 0x17]));
                }
            }
        }
    }

    #[test]
    fn bitmap_destination_error_aborts_releases_and_preserves_original_failure() {
        use std::io::{Cursor, Seek, SeekFrom, Write};
        struct FailedDestination(Cursor<Vec<u8>>);
        impl Write for FailedDestination {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::StorageFull,
                    "synthetic bitmap disk full",
                ))
            }
            fn flush(&mut self) -> io::Result<()> {
                panic!("Encoder must not require flush")
            }
        }
        impl Seek for FailedDestination {
            fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
                self.0.seek(position)
            }
        }
        let mut stream = FailedDestination(Cursor::new(Vec::new()));
        let mut encoder =
            crate::bitmap::BmpEncoder::new(&mut stream, 150, ColorMode::Gray).unwrap();
        let mut usb = synthetic();
        usb.replies.push_back(reply(0));
        let error = run_job(&mut usb, request(), &AtomicBool::new(false), &mut |band| {
            encoder.push(band)
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
        assert!(error.to_string().contains("synthetic bitmap disk full"));
        assert!(usb.writes.ends_with(&[0x06, 0x17]));
    }

    #[test]
    fn rejected_live_limit_keeps_requested_size_for_diagnostics() {
        let mut profile = ScanProfile::default();
        let error = record_read_buffer_limit(&mut profile, 262_144, 65_535).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(profile.read_buffer_bytes, 262_144);
        assert_eq!(profile.usb_read_limit, Some(65_535));
        assert!(profile.stages.is_empty());
        assert_eq!(profile.usb.read_calls, 0);
        assert!(record_read_buffer_limit(&mut profile, 32_768, 65_535).is_ok());
        assert_eq!(profile.read_buffer_bytes, 32_768);
    }

    #[test]
    fn read_buffer_options_reject_invalid_sizes_before_transport() {
        for bytes in [0, 1, 1023, 1025, 1024 * 1024 + 1024, usize::MAX] {
            let mut usb = synthetic();
            let mut profile = ScanProfile::default();
            let error = run_prepared_job_profiled(
                &mut usb,
                &AtomicBool::new(false),
                &mut |_| Ok(()),
                |_| Ok(request()),
                &mut profile,
                ScanTuning {
                    read_buffer_bytes: bytes,
                    ..ScanTuning::default()
                },
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(usb.writes.is_empty());
            assert_eq!(profile.usb.read_calls, 0);
        }
        assert!(validate_read_buffer_limit(256 * 1024, 64 * 1024).is_err());
        assert!(validate_read_buffer_limit(64 * 1024, 64 * 1024).is_ok());
        assert!(validate_read_buffer_limit(256 * 1024, 1024 * 1024).is_ok());
    }

    #[test]
    fn larger_read_buffers_preserve_bytes_short_reads_and_final_padding_request() {
        struct Stream {
            data: Vec<u8>,
            position: usize,
            requests: Vec<usize>,
            short: bool,
        }
        impl Transport for Stream {
            fn write(&mut self, _: &[u8]) -> io::Result<()> {
                panic!("No commands in image stream")
            }
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                self.requests.push(output.len());
                let available = self.data.len() - self.position;
                let count =
                    available
                        .min(output.len())
                        .min(if self.short { 513 } else { usize::MAX });
                output[..count].copy_from_slice(&self.data[self.position..self.position + count]);
                self.position += count;
                Ok(count)
            }
        }
        let expected: Vec<u8> = (0..524_305).map(|index| (index % 251) as u8).collect();
        for bytes in [1024, 65_536, 262_144, 1_048_576] {
            for short in [false, true] {
                let mut usb = Stream {
                    data: expected.clone(),
                    position: 0,
                    requests: vec![],
                    short,
                };
                let mut diagnostics = ScanDiagnostics::new();
                diagnostics.read_buffer_bytes = bytes;
                let mut synchronized = false;
                let output = read_image_band(
                    &mut usb,
                    expected.len(),
                    &AtomicBool::new(false),
                    Instant::now() + Duration::from_secs(5),
                    &mut synchronized,
                    &mut diagnostics,
                )
                .unwrap();
                assert_eq!(output, expected);
                assert!(synchronized);
                assert!(
                    usb.requests
                        .iter()
                        .all(|&size| size <= bytes && size % 1024 == 0)
                );
                if !short {
                    assert_eq!(usb.requests.len(), expected.len().div_ceil(bytes));
                }
            }
        }
    }

    #[test]
    fn larger_buffer_reaches_job_and_cancelled_stream_is_drained_without_delivery() {
        let mut usb = synthetic();
        let mut profile = ScanProfile::default();
        let mut delivered = vec![];
        run_prepared_job_profiled(
            &mut usb,
            &AtomicBool::new(false),
            &mut |band| {
                delivered.extend_from_slice(&band.pixels);
                Ok(())
            },
            |_| Ok(request()),
            &mut profile,
            ScanTuning {
                read_buffer_bytes: 262_144,
                ..ScanTuning::default()
            },
        )
        .unwrap();
        assert_eq!(delivered, [42, 190]);
        assert_eq!(profile.read_buffer_bytes, 262_144);
        assert_eq!(usb.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29, 0x17]);

        struct CancelAfterRead<'a> {
            cancel: &'a AtomicBool,
            calls: usize,
            remaining: usize,
        }
        impl Transport for CancelAfterRead<'_> {
            fn write(&mut self, _: &[u8]) -> io::Result<()> {
                panic!("No command inside image data")
            }
            fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
                self.calls += 1;
                let count = self.remaining.min(output.len());
                output[..count].fill(42);
                self.remaining -= count;
                self.cancel.store(true, Ordering::Relaxed);
                Ok(count)
            }
        }
        let cancel = AtomicBool::new(false);
        let mut usb = CancelAfterRead {
            cancel: &cancel,
            calls: 0,
            remaining: 524_305,
        };
        let mut diagnostics = ScanDiagnostics::new();
        diagnostics.read_buffer_bytes = 262_144;
        let mut synchronized = false;
        let error = read_image_band(
            &mut usb,
            524_305,
            &cancel,
            Instant::now() + Duration::from_secs(5),
            &mut synchronized,
            &mut diagnostics,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(synchronized);
        assert_eq!(usb.remaining, 0);
        assert_eq!(usb.calls, 3);
    }

    #[test]
    fn profile_preserves_complete_job_and_failed_transfer_evidence() {
        let mut usb = synthetic();
        let mut profile = ScanProfile::default();
        let mut pixels = Vec::new();
        run_prepared_job_profiled(
            &mut usb,
            &AtomicBool::new(false),
            &mut |band| {
                pixels.extend_from_slice(&band.pixels);
                Ok(())
            },
            |_| Ok(request()),
            &mut profile,
            Duration::from_millis(100),
        )
        .unwrap();
        assert_eq!(pixels, [42, 190]);
        assert_eq!(usb.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29, 0x17]);
        assert_eq!((profile.usb.read_calls, profile.usb.write_calls), (7, 7));
        assert!(profile.stages.contains_key("read-image"));
        assert!(profile.stages.contains_key("cleanup"));
        assert!(profile.stages.values().copied().sum::<Duration>() <= profile.total);
        let mut failed = InjectedReadFailure {
            inner: synthetic(),
            read_calls: 0,
            fail_call: 6,
        };
        let error = run_prepared_job_profiled(
            &mut failed,
            &AtomicBool::new(false),
            &mut |_| Ok(()),
            |_| Ok(request()),
            &mut profile,
            Duration::from_millis(100),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
        assert_eq!((profile.usb.read_calls, profile.usb.write_calls), (6, 6));
        assert!(profile.stages.contains_key("read-image"));
        assert_eq!(failed.inner.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29]);
    }

    #[test]
    fn profile_interval_validation_precedes_usb_and_only_changes_read_polling() {
        for interval in [
            Duration::ZERO,
            Duration::from_micros(999),
            Duration::from_millis(1001),
        ] {
            let mut usb = synthetic();
            let mut profile = ScanProfile::default();
            profile.usb.read_calls = 999;
            let error = run_prepared_job_profiled(
                &mut usb,
                &AtomicBool::new(false),
                &mut |_| Ok(()),
                |_| Ok(request()),
                &mut profile,
                interval,
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(usb.writes.is_empty());
            assert_eq!(profile.usb.read_calls, 0);
        }
        assert_eq!(
            busy_poll_interval(0x28, Duration::from_millis(500)),
            Duration::from_millis(500)
        );
        for opcode in [0x16, 0x31] {
            assert_eq!(
                busy_poll_interval(opcode, Duration::from_millis(500)),
                Duration::from_millis(100)
            );
        }
    }

    // Cancellation during START Busy remains immediate; only first metadata after confirmed START defers it.
    #[test]
    fn profile_cancels_if_start_is_still_busy_and_cleans_up_without_pixels() {
        let cancel = AtomicBool::new(false);
        let mut inner = synthetic();
        inner.replies[3] = reply(0x20);
        inner.replies[3][1] = 8;
        inner.replies[4] = reply(0);
        inner.replies[5] = reply(0);
        let mut usb = BusyThenCancel {
            inner,
            cancel: &cancel,
            read_calls: 0,
            cancel_on_read: 4,
        };
        let mut profile = ScanProfile::default();
        let error = run_prepared_job_profiled(
            &mut usb,
            &cancel,
            &mut |_| panic!("No image was ready"),
            |_| Ok(request()),
            &mut profile,
            Duration::from_millis(1),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(is_host_cancelled(&error));
        assert_eq!(profile.busy_replies, 1);
        assert!(!profile.busy_sleep.is_zero());
        assert!(profile.stages["start"] >= profile.busy_sleep);
        assert!(profile.stages.contains_key("cleanup"));
        assert_eq!(usb.inner.writes, [0x12, 0x16, 0x24, 0x31, 0x06, 0x17]);
    }

    #[test]
    fn host_cancel_after_metadata_busy_waits_for_first_band_then_drains_and_aborts() {
        let cancel = AtomicBool::new(false);
        let mut inner = synthetic();
        inner.replies[4] = reply(0x20);
        inner.replies[4][1] = 8;
        let raw = inner.replies[5].clone();
        let mut band = reply(0x81);
        band[4..8].copy_from_slice(&18u32.to_be_bytes());
        band[8..10].copy_from_slice(&1u16.to_be_bytes());
        band[10..12].copy_from_slice(&2u16.to_be_bytes());
        inner.replies[5] = band;
        inner.replies[6] = raw;
        inner.replies.push_back(reply(0));
        inner.replies.push_back(reply(0));
        let mut usb = BusyThenCancel {
            inner,
            cancel: &cancel,
            read_calls: 0,
            cancel_on_read: 5,
        };
        let mut profile = ScanProfile::default();
        let mut delivered = 0;
        let (result, health) = run_prepared_job_profiled_with_health(
            &mut usb,
            &cancel,
            &mut |_| {
                delivered += 1;
                Ok(())
            },
            |_| Ok(request()),
            &mut profile,
            Duration::from_millis(1),
        );
        let error = result.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(
            usb.inner.writes,
            [0x12, 0x16, 0x24, 0x31, 0x28, 0x28, 0x29, 0x06, 0x17]
        );
        assert!(is_host_cancelled(&error));
        assert_eq!(delivered, 0);
        assert_eq!(health, SessionHealth::Ready);
        assert_eq!(profile.busy_replies, 1);
        assert!(usb.inner.replies.is_empty());
    }

    #[test]
    fn host_cancelled_first_metadata_device_error_requires_reconnect_after_cleanup() {
        let cancel = AtomicBool::new(false);
        let mut inner = synthetic();
        inner.replies[4] = reply(0x20);
        inner.replies[4][1] = 8;
        inner.replies[5] = reply(0x20);
        inner.replies[5][1] = 4;
        inner.replies[6] = reply(0);
        inner.replies.push_back(reply(0));
        let mut usb = BusyThenCancel {
            inner,
            cancel: &cancel,
            read_calls: 0,
            cancel_on_read: 5,
        };
        let (result, health) = run_job_with_health(&mut usb, request(), &cancel, &mut |_| {
            panic!("No image metadata or band was available")
        });
        let error = result.unwrap_err();
        assert_eq!(health, SessionHealth::NeedsReconnect);
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(!is_host_cancelled(&error));
        assert!(error.to_string().contains("Scanner cancelled the job"));
        assert!(error.to_string().contains("reconnect"));
        assert_eq!(
            usb.inner.writes,
            [0x12, 0x16, 0x24, 0x31, 0x28, 0x28, 0x06, 0x17]
        );
        assert!(usb.inner.replies.is_empty());
    }

    #[test]
    fn deferred_first_metadata_wait_still_stops_at_deadline() {
        let mut usb = synthetic();
        let mut synchronized = true;
        let mut diagnostics = ScanDiagnostics::new();
        let mut attempted = false;
        let error = ready_with_attempt(
            &mut usb,
            0x28,
            &mut synchronized,
            None,
            Instant::now() - Duration::from_millis(1),
            &mut diagnostics,
            &mut attempted,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert!(error.to_string().contains("120 second deadline"));
        assert!(!attempted);
        assert!(usb.writes.is_empty());
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
    fn read_failure_reports_stage_progress_and_original_transport_text() {
        let mut inner = synthetic();
        inner.replies[5] = vec![0x11; 7];
        let mut usb = InjectedReadFailure {
            inner,
            read_calls: 0,
            fail_call: 7,
        };
        let error = run_job(
            &mut usb,
            request(),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotConnected);
        let text = error.to_string();
        assert!(text.contains("stage=read-image"), "{text}");
        assert!(text.contains("received=7/18"), "{text}");
        assert!(text.contains("since_progress_ms="), "{text}");
        assert!(
            text.contains("synthetic WinUSB error code=0xC0000001"),
            "{text}"
        );
        assert!(text.contains("completed_bands=0"), "{text}");
        assert!(text.contains("pixel_bytes=0"), "{text}");
        assert_eq!(usb.inner.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29]);
    }
    #[test]
    fn busy_then_cancel_reports_status_opcode_and_unknown_state() {
        let cancel = AtomicBool::new(false);
        let mut inner = synthetic();
        inner.replies[1][1] = 8;
        let mut usb = BusyThenCancel {
            inner,
            cancel: &cancel,
            read_calls: 0,
            cancel_on_read: 2,
        };
        let error = run_job(&mut usb, request(), &cancel, &mut |_| Ok(())).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        let text = error.to_string();
        assert!(text.contains("Scan cancelled"), "{text}");
        assert!(text.contains("opcode=0x16"), "{text}");
        assert!(text.contains("last_busy_status=0x08"), "{text}");
        assert!(text.contains("last_busy_state=unknown"), "{text}");
        assert!(text.contains("busy_replies=1"), "{text}");
        assert_eq!(usb.inner.writes, [0x12, 0x16]);
    }
    #[test]
    fn busy_history_is_reset_before_the_next_command_failure() {
        let cancel = AtomicBool::new(false);
        let mut inner = synthetic();
        let mut busy = reply(0);
        busy[1] = 8;
        inner.replies.insert(1, busy);
        inner.replies[3] = vec![];
        let mut usb = inner;
        let error = run_job(&mut usb, request(), &cancel, &mut |_| Ok(())).unwrap_err();
        let text = error.to_string();
        assert!(text.contains("stage=set-window"), "{text}");
        assert!(text.contains("opcode=0x24"), "{text}");
        assert!(text.contains("last_busy_status=unknown"), "{text}");
        assert!(text.contains("last_busy_state=unknown"), "{text}");
        assert!(text.contains("busy_replies=0"), "{text}");
        assert_eq!(usb.writes, [0x12, 0x16, 0x16, 0x24]);
    }
    #[test]
    fn check_state_uses_command_specific_offsets_before_cancellation() {
        for (opcode, offset) in [(0x16, 4usize), (0x28, 12usize)] {
            let cancel = AtomicBool::new(false);
            let mut busy = reply(0x20);
            busy[1] = 2;
            busy[offset..offset + 2].copy_from_slice(&0x0080u16.to_be_bytes());
            let mut usb = BusyThenCancel {
                inner: Synthetic {
                    replies: [busy].into(),
                    writes: vec![],
                },
                cancel: &cancel,
                read_calls: 0,
                cancel_on_read: 1,
            };
            let mut synchronized = false;
            let mut diagnostics = ScanDiagnostics::new();
            let error = ready(
                &mut usb,
                opcode,
                &mut synchronized,
                &cancel,
                Instant::now() + Duration::from_secs(1),
                &mut diagnostics,
            )
            .unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Interrupted);
            assert_eq!(diagnostics.opcode, Some(opcode));
            assert_eq!(diagnostics.busy_replies, 1);
            assert_eq!(diagnostics.last_busy_status, Some(0x02));
            assert_eq!(diagnostics.last_busy_state, Some(0x0080));
        }
    }
    #[test]
    fn consumer_failure_keeps_primary_stage_and_marks_cleanup_secondary() {
        let mut usb = synthetic();
        usb.replies[6][1] = 8;
        let error = run_job(&mut usb, request(), &AtomicBool::new(false), &mut |_| {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "consumer output code=0xC0000002",
            ))
        })
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
        let text = error.to_string();
        assert!(text.contains("stage=consumer"), "{text}");
        assert!(text.contains("consumer output code=0xC0000002"), "{text}");
        assert!(text.contains("completed_bands=0"), "{text}");
        assert!(text.contains("pixel_bytes=0"), "{text}");
        assert!(text.contains("cleanup secondary error"), "{text}");
        assert!(text.contains("stage=cleanup"), "{text}");
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
    fn zero_length_transfer_is_drained_before_abort_and_preserves_the_failure() {
        let mut usb = synthetic();
        usb.replies.insert(5, vec![]);
        usb.replies.push_back(reply(0));
        let mut delivered = 0;
        let error = run_job(&mut usb, request(), &AtomicBool::new(false), &mut |_| {
            delivered += 1;
            Ok(())
        })
        .unwrap_err();
        assert!(error.to_string().contains("no progress"));
        assert_eq!(delivered, 0);
        assert_eq!(usb.writes, [0x12, 0x16, 0x24, 0x31, 0x28, 0x29, 0x06, 0x17]);
        assert!(usb.replies.is_empty());
    }

    #[test]
    fn cancelled_and_expired_reads_drain_known_data_but_repeated_empty_reads_stop() {
        for cancelled in [true, false] {
            let mut usb = Synthetic {
                replies: [vec![1, 2], vec![3, 4]].into(),
                writes: vec![],
            };
            let mut synchronized = false;
            let mut diagnostics = ScanDiagnostics::new();
            let deadline = if cancelled {
                Instant::now() + Duration::from_secs(1)
            } else {
                Instant::now() - Duration::from_secs(1)
            };
            let error = read_image_band(
                &mut usb,
                4,
                &AtomicBool::new(cancelled),
                deadline,
                &mut synchronized,
                &mut diagnostics,
            )
            .unwrap_err();
            assert_eq!(
                error.kind(),
                if cancelled {
                    io::ErrorKind::Interrupted
                } else {
                    io::ErrorKind::TimedOut
                }
            );
            assert_eq!(is_host_cancelled(&error), cancelled);
            assert!(synchronized);
            assert!(usb.replies.is_empty());
        }
        let mut usb = Synthetic {
            replies: [vec![], vec![], vec![1, 2]].into(),
            writes: vec![],
        };
        let mut synchronized = false;
        let mut diagnostics = ScanDiagnostics::new();
        assert!(
            read_image_band(
                &mut usb,
                2,
                &AtomicBool::new(false),
                Instant::now() + Duration::from_secs(1),
                &mut synchronized,
                &mut diagnostics,
            )
            .is_err()
        );
        assert!(!synchronized);
        assert_eq!(usb.replies.len(), 1);
    }

    #[test]
    fn opaque_usb_failure_is_not_retried_and_keeps_cancellation_reason() {
        struct FailedRead(usize);
        impl Transport for FailedRead {
            fn write(&mut self, _: &[u8]) -> io::Result<()> {
                panic!("No recovery command allowed");
            }
            fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
                self.0 += 1;
                Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "synthetic disconnected transfer",
                ))
            }
        }
        let mut usb = FailedRead(0);
        let mut synchronized = false;
        let mut diagnostics = ScanDiagnostics::new();
        let error = read_image_band(
            &mut usb,
            18,
            &AtomicBool::new(true),
            Instant::now() + Duration::from_secs(1),
            &mut synchronized,
            &mut diagnostics,
        )
        .unwrap_err();
        assert_eq!(usb.0, 1);
        assert!(!synchronized);
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(error.to_string().contains("Scan cancelled"));
        assert!(error.to_string().contains("disconnected transfer"));
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
        assert!(error.to_string().contains("stage=prepare"));
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
        let text = error.to_string();
        assert!(text.contains("Scan cancelled"), "{text}");
        assert!(text.contains("stage=consumer"), "{text}");
        assert!(text.contains("completed_bands=1"), "{text}");
        assert!(text.contains("pixel_bytes=2"), "{text}");
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
        assert!(error.to_string().contains("stage=read-metadata"));
        assert_eq!(&usb.writes[7..], [0x06, 0x17]);
    }
    #[test]
    fn malformed_reply_diagnostic_reports_lengths_without_payload_bytes() {
        let mut usb = synthetic();
        let mut malformed = vec![0; 32];
        malformed[..4].copy_from_slice(&[0xa8, 0, 31, 0x10]);
        malformed[12..16].copy_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
        usb.replies[0] = malformed;
        let error = run_job(
            &mut usb,
            request(),
            &AtomicBool::new(false),
            &mut |_| Ok(()),
        )
        .unwrap_err();
        let text = error.to_string();
        assert!(text.contains("stage=inquiry"), "{text}");
        assert!(text.contains("received=32"), "{text}");
        assert!(text.contains("expected"), "{text}");
        assert!(!text.contains("de, ad, be, ef"), "{text}");
        assert!(usb.writes == [0x12]);
    }
    #[test]
    fn short_malformed_replies_never_panic_in_diagnostics() {
        for malformed in [vec![], vec![0xa8], vec![0xa8, 0]] {
            let mut usb = synthetic();
            usb.replies[0] = malformed;
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_job(
                    &mut usb,
                    request(),
                    &AtomicBool::new(false),
                    &mut |_| Ok(()),
                )
            }));
            assert!(outcome.is_ok());
            let error = outcome.unwrap().unwrap_err();
            assert!(error.to_string().contains("stage=inquiry"));
            assert_eq!(usb.writes, [0x12]);
        }
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

#[cfg(all(test, windows))]
mod hardware;
