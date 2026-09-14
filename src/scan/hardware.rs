//! Opt-in scan-protocol cancellation evidence.
//!
//! The ignored test in this module is deliberately lower than the WIA/STI
//! layers. It keeps one opened `UsbSession`, arms cancellation at either the
//! first valid READ metadata Busy reply or the first delivered image band,
//! and then asks the core to rescan only when cleanup reports `Ready`.
//!
//! Exact opt-in invocation on the Windows host:
//!
//! ```text
//! $env:WC3119_TEST_CANCEL_PHASE = "metadata_busy" # or "first_band"
//! $env:WC3119_TEST_STI_PATH = "<fresh MI_00 WinUSB interface path>"
//! $env:WC3119_TEST_OUTPUT_DIR = "<fresh nonexistent directory>"
//! cargo test --offline --lib scan::hardware::actual_cancel_phase_then_same_session_rescan -- --ignored --exact --nocapture
//! ```
//!
//! The test writes only control status, timing, band dimensions and byte
//! counts. It never writes image pixels, raw USB payloads or device identity.

use super::{
    ColorMode, ImageBand, ScanProfile, ScanRequest, ScanSummary, ScanTuning, SessionHealth,
    Transport,
};
use crate::protocol::Capabilities;
use crate::usb::UsbSession;
use std::{
    collections::VecDeque,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Instant,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CancelPhase {
    MetadataBusy,
    FirstBand,
}

impl CancelPhase {
    fn parse(value: &str) -> io::Result<Self> {
        match value {
            "metadata_busy" => Ok(Self::MetadataBusy),
            "first_band" => Ok(Self::FirstBand),
            _ => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "WC3119_TEST_CANCEL_PHASE must be metadata_busy or first_band",
            )),
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::MetadataBusy => "metadata_busy",
            Self::FirstBand => "first_band",
        }
    }
}

enum LogTarget {
    File(File),
    Memory(Vec<u8>),
}

struct CompletionFile {
    pending_path: PathBuf,
    final_path: PathBuf,
    file: Option<File>,
    committed: bool,
}

impl CompletionFile {
    fn create_new(final_path: &Path) -> io::Result<Self> {
        let pending_path = final_path.with_file_name("complete.txt.pending");
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending_path)?;
        Ok(Self {
            pending_path,
            final_path: final_path.to_owned(),
            file: Some(file),
            committed: false,
        })
    }

    fn write_line(&mut self, line: impl AsRef<str>) -> io::Result<()> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("completion marker is closed"))?;
        writeln!(file, "{}", line.as_ref())
    }

    fn sync_all(&self) -> io::Result<()> {
        self.file
            .as_ref()
            .ok_or_else(|| io::Error::other("completion marker is closed"))?
            .sync_all()
    }

    fn commit(mut self) -> io::Result<()> {
        self.file.take();
        // The output directory was atomically created by this test and the
        // destination is inside it. A same-directory hard link publishes the
        // marker without replacing an existing destination.
        fs::hard_link(&self.pending_path, &self.final_path)?;
        self.committed = true;
        let _ = fs::remove_file(&self.pending_path);
        Ok(())
    }
}

impl Drop for CompletionFile {
    fn drop(&mut self) {
        if !self.committed {
            self.file.take();
            let _ = fs::remove_file(&self.pending_path);
        }
    }
}

#[derive(Clone)]
struct EventLog {
    target: Arc<Mutex<LogTarget>>,
    started: Instant,
}

impl EventLog {
    fn create_new(path: &Path) -> io::Result<Self> {
        let target = OpenOptions::new().write(true).create_new(true).open(path)?;
        let log = Self {
            target: Arc::new(Mutex::new(LogTarget::File(target))),
            started: Instant::now(),
        };
        log.record("harness=started")?;
        Ok(log)
    }

    #[cfg(test)]
    fn memory() -> Self {
        Self {
            target: Arc::new(Mutex::new(LogTarget::Memory(Vec::new()))),
            started: Instant::now(),
        }
    }

    fn elapsed_ms(&self) -> u128 {
        self.started.elapsed().as_millis()
    }

    fn record(&self, event: impl AsRef<str>) -> io::Result<()> {
        let line = format!("elapsed_ms={} {}", self.elapsed_ms(), event.as_ref());
        let mut target = self
            .target
            .lock()
            .map_err(|_| io::Error::other("diagnostic log mutex poisoned"))?;
        match &mut *target {
            LogTarget::File(file) => {
                writeln!(file, "{line}")?;
                file.flush()?;
            }
            LogTarget::Memory(bytes) => {
                writeln!(bytes, "{line}")?;
                bytes.flush()?;
            }
        }
        Ok(())
    }

    fn sync_all(&self) -> io::Result<()> {
        let target = self
            .target
            .lock()
            .map_err(|_| io::Error::other("diagnostic log mutex poisoned"))?;
        if let LogTarget::File(file) = &*target {
            file.sync_all()?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn memory_text(&self) -> String {
        let target = self.target.lock().unwrap();
        match &*target {
            LogTarget::Memory(bytes) => String::from_utf8_lossy(bytes).into_owned(),
            LogTarget::File(_) => String::new(),
        }
    }
}

#[derive(Default)]
struct TriggerState {
    claimed: AtomicBool,
    phase: Mutex<Option<&'static str>>,
    at_ms: Mutex<Option<u128>>,
}

impl TriggerState {
    fn claim(&self, phase: CancelPhase, cancel: &AtomicBool, log: &EventLog) -> io::Result<bool> {
        if self
            .claimed
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return Ok(false);
        }
        // The cancellation flag is set at the observation point, before the
        // core's next checkpoint or any later diagnostic write.
        cancel.store(true, Ordering::SeqCst);
        let at_ms = log.elapsed_ms();
        *self
            .phase
            .lock()
            .map_err(|_| io::Error::other("trigger phase mutex poisoned"))? = Some(phase.name());
        *self
            .at_ms
            .lock()
            .map_err(|_| io::Error::other("trigger timing mutex poisoned"))? = Some(at_ms);
        log.record(format!(
            "cancel_trigger phase={} at_ms={at_ms}",
            phase.name()
        ))?;
        Ok(true)
    }

    fn claimed(&self) -> bool {
        self.claimed.load(Ordering::SeqCst)
    }

    fn details(&self) -> io::Result<Option<(&'static str, u128)>> {
        let phase = *self
            .phase
            .lock()
            .map_err(|_| io::Error::other("trigger phase mutex poisoned"))?;
        let at_ms = *self
            .at_ms
            .lock()
            .map_err(|_| io::Error::other("trigger timing mutex poisoned"))?;
        Ok(phase.zip(at_ms))
    }
}

#[derive(Clone, Copy, Debug, Default)]
struct BandStats {
    bands: u32,
    width: u32,
    rows: u64,
    pixel_bytes: usize,
    wire_bytes: usize,
}

impl BandStats {
    fn observe(&mut self, band: &ImageBand) -> io::Result<()> {
        self.bands = self
            .bands
            .checked_add(1)
            .ok_or_else(|| io::Error::other("band count overflow in hardware evidence"))?;
        self.width = band.width;
        self.rows = self
            .rows
            .checked_add(u64::from(band.rows))
            .ok_or_else(|| io::Error::other("row count overflow in hardware evidence"))?;
        self.pixel_bytes = self
            .pixel_bytes
            .checked_add(band.pixels.len())
            .ok_or_else(|| io::Error::other("pixel byte count overflow in hardware evidence"))?;
        self.wire_bytes = self
            .wire_bytes
            .checked_add(band.wire_data.len())
            .ok_or_else(|| io::Error::other("wire byte count overflow in hardware evidence"))?;
        Ok(())
    }
}

struct ObservedTransport<'a, T> {
    inner: &'a mut T,
    cancel: &'a AtomicBool,
    phase: Option<CancelPhase>,
    trigger: Arc<TriggerState>,
    log: EventLog,
    last_opcode: Option<u8>,
    command_started: Option<Instant>,
}

impl<'a, T> ObservedTransport<'a, T> {
    fn new(
        inner: &'a mut T,
        cancel: &'a AtomicBool,
        phase: Option<CancelPhase>,
        trigger: Arc<TriggerState>,
        log: EventLog,
    ) -> Self {
        Self {
            inner,
            cancel,
            phase,
            trigger,
            log,
            last_opcode: None,
            command_started: None,
        }
    }

    fn opcode_text(opcode: Option<u8>) -> String {
        opcode
            .map(|opcode| format!("0x{opcode:02x}"))
            .unwrap_or_else(|| "unknown".to_owned())
    }
}

impl<T: Transport> Transport for ObservedTransport<'_, T> {
    fn write(&mut self, data: &[u8]) -> io::Result<()> {
        let opcode = data.get(2).copied();
        self.last_opcode = opcode;
        self.command_started = Some(Instant::now());
        self.log.record(format!(
            "command_start opcode={} bytes={}",
            Self::opcode_text(opcode),
            data.len()
        ))?;
        match self.inner.write(data) {
            Ok(()) => {
                self.log.record(format!(
                    "command_complete opcode={} result=ok",
                    Self::opcode_text(opcode)
                ))?;
                Ok(())
            }
            Err(error) => {
                self.log.record(format!(
                    "command_complete opcode={} result=error kind={:?}",
                    Self::opcode_text(opcode),
                    error.kind()
                ))?;
                Err(error)
            }
        }
    }

    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        let call_started = Instant::now();
        let opcode = self.last_opcode;
        let n = match self.inner.read(data) {
            Ok(n) => n,
            Err(error) => {
                self.log.record(format!(
                    "read_complete opcode={} result=error kind={:?}",
                    Self::opcode_text(opcode),
                    error.kind()
                ))?;
                return Err(error);
            }
        };

        if opcode == Some(0x29) {
            self.log.record(format!(
                "image_read bytes={} elapsed_call_ms={}",
                n,
                call_started.elapsed().as_millis()
            ))?;
            return Ok(n);
        }

        let response = data.get(..n).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "observed USB read count exceeds caller buffer",
            )
        })?;
        let status = super::response_status(response, opcode == Some(0x28));
        let command_elapsed_ms = self
            .command_started
            .map(|started| started.elapsed().as_millis());
        let is_first_metadata_busy = self.phase == Some(CancelPhase::MetadataBusy)
            && !self.trigger.claimed()
            && opcode == Some(0x28)
            && response
                .get(3)
                .is_some_and(|message| matches!(*message, 0x20 | 0x80 | 0x81))
            && matches!(&status, Ok(super::ReplyStatus::Busy));
        if is_first_metadata_busy {
            self.trigger
                .claim(CancelPhase::MetadataBusy, self.cancel, &self.log)?;
        }
        self.log.record(format!(
            "control_reply opcode={} status_byte={} message={} bytes={} elapsed_call_ms={} command_elapsed_ms={}",
            Self::opcode_text(opcode),
            response
                .get(1)
                .map(|status| format!("0x{status:02x}"))
                .unwrap_or_else(|| "unknown".to_owned()),
            response
                .get(3)
                .map(|message| format!("0x{message:02x}"))
                .unwrap_or_else(|| "unknown".to_owned()),
            n,
            call_started.elapsed().as_millis(),
            command_elapsed_ms
                .map_or_else(|| "unknown".to_owned(), |elapsed| elapsed.to_string())
        ))?;
        self.command_started = None;
        Ok(n)
    }
}

fn full_bed_rgb75(raw: &[u8]) -> io::Result<ScanRequest> {
    let caps = Capabilities::parse(raw)?;
    let request = ScanRequest {
        dpi: 75,
        mode: ColorMode::Rgb,
        x_units: 0,
        y_units: 0,
        width_units: caps.width_units,
        height_units: caps.flatbed_length_units.min(caps.length_units),
    };
    // Reuse the production command validator before RESERVE. This checks the
    // live mode, resolution, compression and geometry capabilities without
    // recording the identity parsed by INQUIRY.
    request.command(&caps)?;
    Ok(request)
}

fn log_profile(log: &EventLog, job: &str, profile: &ScanProfile) -> io::Result<()> {
    log.record(format!(
        "job={job} profile total_ms={} read_buffer={} read_limit={} busy_replies={} busy_sleep_ms={} read_calls={} write_calls={} read_ms={} write_ms={}",
        profile.total.as_millis(),
        profile.read_buffer_bytes,
        profile
            .usb_read_limit
            .map_or_else(|| "unknown".to_owned(), |limit| limit.to_string()),
        profile.busy_replies,
        profile.busy_sleep.as_millis(),
        profile.usb.read_calls,
        profile.usb.write_calls,
        profile.usb.read_time.as_millis(),
        profile.usb.write_time.as_millis(),
    ))
}

fn log_outcome(
    log: &EventLog,
    job: &str,
    result: &io::Result<ScanSummary>,
    health: SessionHealth,
    stats: BandStats,
) -> io::Result<()> {
    match result {
        Ok(summary) => log.record(format!(
            "job={job} result=success health={health:?} summary_width={} summary_height={} summary_bands={} summary_bytes={} observed_bands={} observed_rows={} observed_pixels={} observed_wire={}",
            summary.width,
            summary.height,
            summary.bands,
            summary.bytes,
            stats.bands,
            stats.rows,
            stats.pixel_bytes,
            stats.wire_bytes,
        )),
        Err(error) => log.record(format!(
            "job={job} result=error health={health:?} kind={:?} error={error} observed_bands={} observed_rows={} observed_pixels={} observed_wire={}",
            error.kind(),
            stats.bands,
            stats.rows,
            stats.pixel_bytes,
            stats.wire_bytes,
        )),
    }
}

fn test_inputs() -> io::Result<(CancelPhase, Vec<u16>, PathBuf)> {
    let phase = CancelPhase::parse(&std::env::var("WC3119_TEST_CANCEL_PHASE").map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "set WC3119_TEST_CANCEL_PHASE to metadata_busy or first_band",
        )
    })?)?;
    let path = std::env::var_os("WC3119_TEST_STI_PATH").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "set WC3119_TEST_STI_PATH to a fresh MI_00 interface path",
        )
    })?;
    let mut path_utf16: Vec<u16> = path.encode_wide().collect();
    if path_utf16.is_empty() || path_utf16.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WC3119_TEST_STI_PATH must be a nonempty path without embedded NUL",
        ));
    }
    path_utf16.push(0);
    let output = std::env::var_os("WC3119_TEST_OUTPUT_DIR").ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "set WC3119_TEST_OUTPUT_DIR to a fresh nonexistent directory",
        )
    })?;
    if output.as_os_str().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "WC3119_TEST_OUTPUT_DIR must be nonempty",
        ));
    }
    Ok((phase, path_utf16, PathBuf::from(output)))
}

fn run_hardware_phase(phase: CancelPhase, path: &[u16], output: &Path) -> io::Result<()> {
    fs::create_dir(output).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!("create fresh WC3119_TEST_OUTPUT_DIR: {error}"),
        )
    })?;
    let log = EventLog::create_new(&output.join("diagnostics.log"))?;
    log.record(format!("cancel_phase={} mode=rgb dpi=75", phase.name()))?;

    let mut usb = UsbSession::open_matching_path(path)?;
    let tuning = ScanTuning::default();
    let mut preflight_profile = ScanProfile::default();
    let read_limit = match super::preflight_read_buffer(
        &usb,
        tuning.read_buffer_bytes,
        &mut preflight_profile,
    ) {
        Ok(limit) => limit,
        Err(error) => {
            log.record(format!(
                "preflight result=error kind={:?} error={error}",
                error.kind()
            ))?;
            log.sync_all()?;
            return Err(error);
        }
    };
    log.record(format!(
        "preflight result=ok read_buffer={} read_limit={read_limit}",
        tuning.read_buffer_bytes
    ))?;

    let cancel = AtomicBool::new(false);
    let trigger = Arc::new(TriggerState::default());
    let mut first_stats = BandStats::default();
    let mut first_profile = ScanProfile::default();
    let first = {
        let trigger_for_sink = Arc::clone(&trigger);
        let log_for_sink = log.clone();
        let mut transport = ObservedTransport::new(
            &mut usb,
            &cancel,
            Some(phase),
            Arc::clone(&trigger),
            log.clone(),
        );
        let mut sink = |band: &ImageBand| -> io::Result<()> {
            first_stats.observe(band)?;
            if phase == CancelPhase::FirstBand && first_stats.bands == 1 {
                trigger_for_sink.claim(CancelPhase::FirstBand, &cancel, &log_for_sink)?;
            }
            log_for_sink.record(format!(
                "job=cancel band={} width={} rows={} pixel_bytes={} wire_bytes={}",
                first_stats.bands,
                band.width,
                band.rows,
                band.pixels.len(),
                band.wire_data.len(),
            ))?;
            Ok(())
        };
        super::run_prepared_job_profiled_with_health(
            &mut transport,
            &cancel,
            &mut sink,
            full_bed_rgb75,
            &mut first_profile,
            tuning,
        )
    };
    first_profile.usb_read_limit = Some(read_limit);
    log_profile(&log, "cancel", &first_profile)?;
    log_outcome(&log, "cancel", &first.0, first.1, first_stats)?;
    log.sync_all()?;

    if !trigger.claimed() {
        return Err(io::Error::other(format!(
            "cancel phase {} was not observed",
            phase.name()
        )));
    }
    match &first.0 {
        Err(error) if super::is_host_cancelled(error) => {}
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("cancel phase returned an unexpected error: {error}"),
            ));
        }
        Ok(summary) => {
            return Err(io::Error::other(format!(
                "cancel phase completed a scan unexpectedly: {summary:?}"
            )));
        }
    }
    if first.1 != SessionHealth::Ready {
        return Err(io::Error::other(format!(
            "cancel cleanup did not leave the held session Ready: {:?}",
            first.1
        )));
    }
    let trigger_details = trigger
        .details()?
        .ok_or_else(|| io::Error::other("cancel trigger details were not recorded"))?;
    log.record(format!(
        "cancel_trigger_verified phase={} at_ms={}",
        trigger_details.0, trigger_details.1
    ))?;

    let rescan_cancel = AtomicBool::new(false);
    let rescan_trigger = Arc::new(TriggerState::default());
    let mut rescan_stats = BandStats::default();
    let mut rescan_profile = ScanProfile::default();
    let rescan = {
        let log_for_sink = log.clone();
        let mut transport = ObservedTransport::new(
            &mut usb,
            &rescan_cancel,
            None,
            Arc::clone(&rescan_trigger),
            log.clone(),
        );
        let mut sink = |band: &ImageBand| -> io::Result<()> {
            rescan_stats.observe(band)?;
            log_for_sink.record(format!(
                "job=rescan band={} width={} rows={} pixel_bytes={} wire_bytes={}",
                rescan_stats.bands,
                band.width,
                band.rows,
                band.pixels.len(),
                band.wire_data.len(),
            ))?;
            Ok(())
        };
        super::run_prepared_job_profiled_with_health(
            &mut transport,
            &rescan_cancel,
            &mut sink,
            full_bed_rgb75,
            &mut rescan_profile,
            tuning,
        )
    };
    rescan_profile.usb_read_limit = Some(read_limit);
    log_profile(&log, "rescan", &rescan_profile)?;
    log_outcome(&log, "rescan", &rescan.0, rescan.1, rescan_stats)?;
    log.sync_all()?;
    let rescan_summary = match &rescan.0 {
        Ok(summary) if rescan.1 == SessionHealth::Ready => summary,
        Ok(summary) => {
            return Err(io::Error::other(format!(
                "rescan succeeded but cleanup health was {:?}: {summary:?}",
                rescan.1
            )));
        }
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("same-session rescan failed: {error}"),
            ));
        }
    };

    log.record(format!(
        "rescan_verified width={} height={} bands={} bytes={} observed_bands={} observed_rows={} observed_pixels={} observed_wire={}",
        rescan_summary.width,
        rescan_summary.height,
        rescan_summary.bands,
        rescan_summary.bytes,
        rescan_stats.bands,
        rescan_stats.rows,
        rescan_stats.pixel_bytes,
        rescan_stats.wire_bytes,
    ))?;
    log.sync_all()?;

    let mut complete = CompletionFile::create_new(&output.join("complete.txt"))?;
    complete.write_line(format!("cancel_phase={}", phase.name()))?;
    complete.write_line(format!("rescan_width={}", rescan_summary.width))?;
    complete.write_line(format!("rescan_height={}", rescan_summary.height))?;
    complete.write_line(format!("rescan_bands={}", rescan_summary.bands))?;
    complete.write_line(format!("rescan_bytes={}", rescan_summary.bytes))?;
    complete.sync_all()?;
    // Any diagnostic failure still drops the pending marker. The final name is
    // published only after all other fallible evidence writes complete.
    log.record("harness=complete_pending")?;
    log.sync_all()?;
    complete.commit()
}

#[test]
#[ignore = "Hardware: set WC3119_TEST_CANCEL_PHASE, fresh WC3119_TEST_STI_PATH and new WC3119_TEST_OUTPUT_DIR; cancels at an exact core phase then rescans on the same held UsbSession"]
fn actual_cancel_phase_then_same_session_rescan() -> io::Result<()> {
    let (phase, path, output) = test_inputs()?;
    run_hardware_phase(phase, &path, &output)
}

#[cfg(test)]
fn reply(status: u8, message: u8) -> Vec<u8> {
    let mut bytes = vec![0; 32];
    bytes[..4].copy_from_slice(&[0xa8, status, 29, message]);
    bytes
}

#[cfg(test)]
struct Synthetic {
    replies: VecDeque<Vec<u8>>,
}

#[cfg(test)]
impl Transport for Synthetic {
    fn write(&mut self, _: &[u8]) -> io::Result<()> {
        Ok(())
    }

    fn read(&mut self, data: &mut [u8]) -> io::Result<usize> {
        let reply = self
            .replies
            .pop_front()
            .ok_or_else(|| io::Error::other("synthetic reply exhausted"))?;
        data[..reply.len()].copy_from_slice(&reply);
        Ok(reply.len())
    }
}

#[test]
fn first_metadata_busy_triggers_exactly_once() {
    let log = EventLog::memory();
    let trigger = Arc::new(TriggerState::default());
    let cancel = AtomicBool::new(false);
    let mut inner = Synthetic {
        replies: VecDeque::from([
            reply(8, 0x20), // Busy from an unrelated command.
            reply(8, 0x20), // First valid READ metadata Busy.
            reply(8, 0x20), // A later Busy must not retrigger.
        ]),
    };
    let mut observed = ObservedTransport::new(
        &mut inner,
        &cancel,
        Some(CancelPhase::MetadataBusy),
        Arc::clone(&trigger),
        log.clone(),
    );
    let mut buffer = vec![0; 32];
    observed.write(&[0x1b, 0xa8, 0x16, 0]).unwrap();
    observed.read(&mut buffer).unwrap();
    assert!(!trigger.claimed());
    assert!(!cancel.load(Ordering::SeqCst));
    observed.write(&[0x1b, 0xa8, 0x28, 0]).unwrap();
    observed.read(&mut buffer).unwrap();
    assert!(trigger.claimed());
    assert!(cancel.load(Ordering::SeqCst));
    observed.read(&mut buffer).unwrap();
    assert_eq!(log.memory_text().matches("cancel_trigger ").count(), 1);
}

#[test]
fn metadata_trigger_requires_read_metadata_message() {
    let log = EventLog::memory();
    let trigger = Arc::new(TriggerState::default());
    let cancel = AtomicBool::new(false);
    let mut inner = Synthetic {
        replies: VecDeque::from([reply(8, 0x10), reply(8, 0x80)]),
    };
    let mut observed = ObservedTransport::new(
        &mut inner,
        &cancel,
        Some(CancelPhase::MetadataBusy),
        Arc::clone(&trigger),
        log,
    );
    let mut buffer = vec![0; 32];
    observed.write(&[0x1b, 0xa8, 0x28, 0]).unwrap();
    observed.read(&mut buffer).unwrap();
    assert!(!trigger.claimed());
    assert!(!cancel.load(Ordering::SeqCst));
    observed.read(&mut buffer).unwrap();
    assert!(trigger.claimed());
    assert!(cancel.load(Ordering::SeqCst));
}

#[test]
fn metadata_trigger_accepts_each_core_read_message() {
    for message in [0x20, 0x80, 0x81] {
        let log = EventLog::memory();
        let trigger = Arc::new(TriggerState::default());
        let cancel = AtomicBool::new(false);
        let mut inner = Synthetic {
            replies: VecDeque::from([reply(8, message)]),
        };
        let mut observed = ObservedTransport::new(
            &mut inner,
            &cancel,
            Some(CancelPhase::MetadataBusy),
            Arc::clone(&trigger),
            log,
        );
        let mut buffer = vec![0; 32];
        observed.write(&[0x1b, 0xa8, 0x28, 0]).unwrap();
        observed.read(&mut buffer).unwrap();
        assert!(trigger.claimed(), "message=0x{message:02x}");
        assert!(cancel.load(Ordering::SeqCst), "message=0x{message:02x}");
    }
}

#[test]
fn malformed_or_unrelated_control_replies_never_trigger_or_claim_success() {
    let mut malformed_header = reply(8, 0x20);
    malformed_header[0] = 0;
    let mut malformed_length = reply(8, 0x20);
    malformed_length[2] = 0;
    for response in [reply(0, 0x10), malformed_header, malformed_length] {
        let log = EventLog::memory();
        let trigger = Arc::new(TriggerState::default());
        let cancel = AtomicBool::new(false);
        let mut inner = Synthetic {
            replies: VecDeque::from([response]),
        };
        let mut observed = ObservedTransport::new(
            &mut inner,
            &cancel,
            Some(CancelPhase::MetadataBusy),
            Arc::clone(&trigger),
            log.clone(),
        );
        let mut synchronized = true;
        assert!(super::exchange(&mut observed, &[0x1b, 0xa8, 0x28, 0], &mut synchronized).is_err());
        assert!(!synchronized);
        assert!(!trigger.claimed());
        assert!(!cancel.load(Ordering::SeqCst));
        assert!(!log.memory_text().contains("status=good"));
    }
}

#[test]
fn first_band_trigger_is_one_shot() {
    let log = EventLog::memory();
    let trigger = TriggerState::default();
    let cancel = AtomicBool::new(false);
    assert!(
        trigger
            .claim(CancelPhase::FirstBand, &cancel, &log)
            .unwrap()
    );
    assert!(
        !trigger
            .claim(CancelPhase::FirstBand, &cancel, &log)
            .unwrap()
    );
    assert!(cancel.load(Ordering::SeqCst));
    assert_eq!(log.memory_text().matches("cancel_trigger ").count(), 1);
}

#[test]
fn completion_marker_is_absent_until_commit_and_never_replaces_existing_file() {
    let root = std::env::temp_dir().join(format!(
        "wc3119-scan-hardware-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&root).unwrap();
    let marker = root.join("complete.txt");
    let mut pending = CompletionFile::create_new(&marker).unwrap();
    assert!(!marker.exists());
    pending.write_line("new").unwrap();
    pending.sync_all().unwrap();
    assert!(!marker.exists());
    pending.commit().unwrap();
    assert_eq!(fs::read_to_string(&marker).unwrap(), "new\n");

    let existing = root.join("existing");
    fs::create_dir(&existing).unwrap();
    let existing_marker = existing.join("complete.txt");
    fs::write(&existing_marker, "old\n").unwrap();
    let mut pending = CompletionFile::create_new(&existing_marker).unwrap();
    pending.write_line("replacement").unwrap();
    pending.sync_all().unwrap();
    assert!(pending.commit().is_err());
    assert_eq!(fs::read_to_string(&existing_marker).unwrap(), "old\n");
    assert!(!existing.join("complete.txt.pending").exists());
    fs::remove_file(marker).unwrap();
    fs::remove_file(existing_marker).unwrap();
    fs::remove_dir(existing).unwrap();
    fs::remove_dir(root).unwrap();
}
