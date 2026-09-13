//! Development-only repeated scanner stability check, not a user-facing scan application.

use std::{
    cell::Cell,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};

use workcentre_3119::{
    protocol::Capabilities,
    scan::{self, ColorMode, ImageBand, ScanRequest, ScanTuning},
};

const DEFAULT_COUNT: u32 = 20;
const MAX_COUNT: u32 = 20;

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn new_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn settings(args: &[String]) -> io::Result<u32> {
    if !(1..=2).contains(&args.len()) {
        return Err(invalid(
            "Expected scan_stability NEW_DIRECTORY [COUNT], with COUNT in 1..=20",
        ));
    }
    match args.get(1) {
        None => Ok(DEFAULT_COUNT),
        Some(value) => {
            let count = value.parse::<u32>().map_err(|_| {
                invalid("Expected scan_stability NEW_DIRECTORY [COUNT], with COUNT in 1..=20")
            })?;
            if !(1..=MAX_COUNT).contains(&count) {
                return Err(invalid(
                    "Expected scan_stability NEW_DIRECTORY [COUNT], with COUNT in 1..=20",
                ));
            }
            Ok(count)
        }
    }
}

fn capture_settings(args: &[String]) -> io::Result<(u32, ScanTuning)> {
    let first_option = args
        .iter()
        .position(|arg| arg.starts_with("--"))
        .unwrap_or(args.len());
    let count = settings(&args[..first_option])?;
    let mut tuning = ScanTuning::default();
    let mut seen_poll = false;
    let mut seen_buffer = false;
    for pair in args[first_option..].chunks(2) {
        if pair.len() != 2 {
            return Err(invalid("Each transfer option requires an integer value"));
        }
        let value = pair[1]
            .parse::<usize>()
            .map_err(|_| invalid("Transfer options require positive integers"))?;
        match pair[0].as_str() {
            "--read-poll-ms" if !seen_poll && (1..=1000).contains(&value) => {
                seen_poll = true;
                tuning.read_poll_interval = Duration::from_millis(value as u64);
            }
            "--read-buffer-kib" if !seen_buffer && (1..=1024).contains(&value) => {
                seen_buffer = true;
                tuning.read_buffer_bytes = value * 1024;
            }
            _ => {
                return Err(invalid(
                    "Unknown, repeated or out-of-range transfer option: --read-poll-ms 1..=1000; --read-buffer-kib 1..=1024",
                ));
            }
        }
    }
    Ok((count, tuning))
}

fn record_scan_profile<T>(
    result: io::Result<T>,
    output: &mut impl Write,
    run: u32,
    profile: &impl std::fmt::Debug,
) -> io::Result<T> {
    let recorded = writeln!(output, "run={run} profile={profile:?}").and_then(|_| output.flush());
    match (result, recorded) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(error), Ok(())) | (Ok(_), Err(error)) => Err(error),
        (Err(error), Err(log_error)) => Err(io::Error::new(
            error.kind(),
            format!("{error}; profile logging failed: {log_error}"),
        )),
    }
}

fn run_settings(index: usize) -> (ColorMode, u32) {
    match index % 4 {
        0 => (ColorMode::Rgb, 600),
        1 => (ColorMode::Rgb, 600),
        2 => (ColorMode::Rgb, 300),
        _ => (ColorMode::Gray, 600),
    }
}

fn mode_name(mode: ColorMode) -> &'static str {
    match mode {
        ColorMode::Gray => "gray",
        ColorMode::Rgb => "rgb",
    }
}

fn verify_band(expected_mode: ColorMode, line_order: u8, band: &ImageBand) -> io::Result<()> {
    if band.mode != expected_mode {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Scan band mode differs from requested mode",
        ));
    }
    if expected_mode == ColorMode::Rgb && line_order > 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Unsupported RGB channel order",
        ));
    }
    let width = usize::try_from(band.width)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Scan band width overflow"))?;
    let rows = usize::try_from(band.rows)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Scan band row overflow"))?;
    let channels = match expected_mode {
        ColorMode::Gray => 1,
        ColorMode::Rgb => 3,
    };
    let pixel_bytes = width
        .checked_mul(rows)
        .and_then(|value| value.checked_mul(channels))
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "Scan band size overflow"))?;
    if width == 0
        || rows == 0
        || band.pixels.len() != pixel_bytes
        || band.wire_data.len() < pixel_bytes
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Scan band byte counts do not match metadata",
        ));
    }
    match (expected_mode, line_order) {
        (ColorMode::Gray, _) | (ColorMode::Rgb, 0) => {
            if band.pixels != band.wire_data[..pixel_bytes] {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Decoded pixels differ from effective wire bytes",
                ));
            }
        }
        (ColorMode::Rgb, 1) => {
            for row in 0..rows {
                for x in 0..width {
                    for channel in 0..3 {
                        let wire_index = row * width * 3 + channel * width + x;
                        let pixel_index = (row * width + x) * 3 + channel;
                        if band.pixels[pixel_index] != band.wire_data[wire_index] {
                            return Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "Decoded RGB pixels differ from effective planar wire bytes",
                            ));
                        }
                    }
                }
            }
        }
        (ColorMode::Rgb, _) => unreachable!("RGB channel order was checked above"),
    }
    Ok(())
}

fn capture(args: &[String]) -> io::Result<()> {
    let (count, tuning) = capture_settings(args)?;
    let directory = Path::new(&args[0]);
    // Atomic directory creation rejects every existing target before USB access.
    fs::create_dir(directory)?;
    let mut diagnostics = new_file(&directory.join("diagnostics.log"))?;
    let result = (|| {
        writeln!(
            diagnostics,
            "stability_count={count}; pattern=rgb600,rgb600,rgb300,gray600; read_poll_ms={}; read_buffer_bytes={}",
            tuning.read_poll_interval.as_millis(),
            tuning.read_buffer_bytes
        )?;
        diagnostics.sync_all()?;

        for ordinal in 0..count {
            let run_index = ordinal + 1;
            let (mode, dpi) = run_settings(ordinal as usize);
            println!(
                "Run {run_index}/{count} starting: {} {dpi} dpi",
                mode_name(mode)
            );
            let started = Instant::now();
            let line_order = Cell::new(None);
            let mut bands = 0u32;
            let mut pixel_bytes = 0usize;
            let cancel = AtomicBool::new(false);
            let mut profile = scan::ScanProfile::default();
            let scan_result = scan::scan_with_profile(
                &cancel,
                |usb| {
                    let caps = Capabilities::parse(&usb.reply)?;
                    line_order.set(Some(caps.line_order));
                    Ok(ScanRequest {
                        dpi,
                        mode,
                        x_units: 0,
                        y_units: 0,
                        width_units: caps.width_units,
                        height_units: caps.flatbed_length_units.min(caps.length_units),
                    })
                },
                |band| {
                    let order = line_order.get().ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Missing capabilities for scan band validation",
                        )
                    })?;
                    verify_band(mode, order, band)?;
                    bands = bands
                        .checked_add(1)
                        .ok_or_else(|| io::Error::other("Scan band count overflow"))?;
                    let next_pixel_bytes = pixel_bytes
                        .checked_add(band.pixels.len())
                        .ok_or_else(|| io::Error::other("Scan pixel byte count overflow"))?;
                    pixel_bytes = next_pixel_bytes;
                    writeln!(
                        diagnostics,
                        "run={run_index} mode={} dpi={dpi} band={bands} elapsed_ms={} width={} rows={} pixel_bytes={}",
                        mode_name(mode),
                        started.elapsed().as_millis(),
                        band.width,
                        band.rows,
                        band.pixels.len()
                    )?;
                    diagnostics.flush()
                },
                &mut profile,
                tuning,
            );
            let elapsed_ms = started.elapsed().as_millis();
            let scan_result =
                record_scan_profile(scan_result, &mut diagnostics, run_index, &profile);
            let run_result = match scan_result {
                Ok(summary) => {
                    if summary.bands != bands || summary.bytes != pixel_bytes {
                        Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "Scan summary differs from delivered bands",
                        ))
                    } else {
                        (|| {
                            writeln!(
                                diagnostics,
                                "run={run_index} mode={} dpi={dpi} success=true bands={} width={} height={} pixel_bytes={} job_elapsed_ms={elapsed_ms}",
                                mode_name(mode),
                                summary.bands,
                                summary.width,
                                summary.height,
                                summary.bytes
                            )?;
                            diagnostics.sync_all()?;
                            Ok(summary)
                        })()
                    }
                }
                Err(error) => Err(error),
            };
            match run_result {
                Ok(summary) => {
                    println!(
                        "Run {run_index}/{count} complete: {} {dpi} dpi, {} bands, {}x{}, {} bytes, {elapsed_ms} ms",
                        mode_name(mode),
                        summary.bands,
                        summary.width,
                        summary.height,
                        summary.bytes
                    );
                }
                Err(error) => {
                    println!("Run {run_index}/{count} failed after {elapsed_ms} ms: {error}");
                    let _ = writeln!(
                        diagnostics,
                        "run={run_index} mode={} dpi={dpi} success=false partial_bands={bands} partial_pixel_bytes={pixel_bytes} error_kind={:?} error={error} job_elapsed_ms={elapsed_ms}",
                        mode_name(mode),
                        error.kind()
                    );
                    let _ = diagnostics.flush();
                    return Err(error);
                }
            }
        }

        let mut complete = new_file(&directory.join("complete.txt"))?;
        writeln!(
            complete,
            "Completed {count} scans: pattern=rgb600,rgb600,rgb300,gray600"
        )?;
        complete.sync_all()?;
        Ok(())
    })();
    if let Err(ref error) = result {
        let _ = writeln!(
            diagnostics,
            "stability_complete=false error_kind={:?} error={error}",
            error.kind()
        );
        let _ = diagnostics.sync_all();
    }
    result
}

fn print_help() {
    println!(
        "Development-only scanner stability check. Usage:\n\
scan_stability NEW_DIRECTORY [COUNT] [--read-poll-ms N] [--read-buffer-kib N]\n\
Runs COUNT scans in one process using rgb600, rgb600, rgb300, gray600 repeatedly.\n\
COUNT defaults to 20 and must be an integer from 1 through 20. Example: scan_stability artifacts/stability 20\n\
--read-poll-ms N is an experimental READ Busy interval in 1..=1000 milliseconds (default 100). Example: scan_stability artifacts/poll500 1 --read-poll-ms 500\n\
--read-buffer-kib N sets each image read buffer in 1..=1024 KiB (default 64), bounded by the current WinUSB pipe limit. Example: scan_stability artifacts/buffer256 1 --read-buffer-kib 256\n\
Options follow COUNT in either order and cannot repeat. Larger buffers or poll intervals can delay cancellation checks until the active call or sleep completes.\n\
Only the selected READ Busy interval and image read buffer size change. Other commands, the 120-second job deadline, image settings and transfer policy remain unchanged.\n\
Run with no arguments or --help to show this help. Requires Windows and a paired scanner MI_00.\n\
Each run discovers capabilities in its own scan session and scans the full reported flatbed.\n\
Only diagnostics.log is written during runs, including a timing profile on success or failure; no image or USB band bytes are saved. complete.txt is created only after every run succeeds.\n\
USB call durations and Busy sleeps overlap the stage totals; do not add them together. USB call time includes device waiting and is not a pure bus-speed measurement.\n\
The new directory and output files use create-new semantics. Existing directories, invalid arguments, scan errors, write errors, and pixel mismatches stop immediately with exit code 1.\n\
Exit codes: 0 for all requested scans completed or help; 1 for invalid arguments or a failed stability run."
    );
}

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        print_help();
        return std::process::ExitCode::SUCCESS;
    }
    match capture(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Stability check failed: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use workcentre_3119::scan::{ColorMode, ImageBand};

    #[test]
    fn parameters_accept_default_and_bounded_counts_only() {
        assert_eq!(settings(&["unused".into()]).unwrap(), 20);
        assert_eq!(settings(&["unused".into(), "1".into()]).unwrap(), 1);
        assert_eq!(settings(&["unused".into(), "20".into()]).unwrap(), 20);
        for args in [
            vec![],
            vec!["unused", "0"],
            vec!["unused", "21"],
            vec!["unused", "not-a-number"],
            vec!["unused", "1", "extra"],
        ] {
            assert!(settings(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
    }

    #[test]
    fn polling_override_is_explicit_bounded_and_keeps_default_count() {
        assert_eq!(
            capture_settings(&["unused".into()]).unwrap(),
            (20, ScanTuning::from(std::time::Duration::from_millis(100)))
        );
        assert_eq!(
            capture_settings(&[
                "unused".into(),
                "2".into(),
                "--read-poll-ms".into(),
                "500".into()
            ])
            .unwrap(),
            (2, ScanTuning::from(std::time::Duration::from_millis(500)))
        );
        assert_eq!(
            capture_settings(&["unused".into(), "--read-poll-ms".into(), "1".into()]).unwrap(),
            (20, ScanTuning::from(std::time::Duration::from_millis(1)))
        );
        for value in ["0", "1001", "-1", "1.5", "bad"] {
            assert!(
                capture_settings(&[
                    "unused".into(),
                    "1".into(),
                    "--read-poll-ms".into(),
                    value.into()
                ])
                .is_err()
            );
        }
        for args in [
            vec!["unused", "--read-poll-ms"],
            vec!["unused", "1", "--other", "500"],
            vec![
                "unused",
                "1",
                "--read-poll-ms",
                "500",
                "--read-poll-ms",
                "100",
            ],
        ] {
            assert!(
                capture_settings(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err()
            );
        }
    }

    #[test]
    fn buffer_override_is_bounded_and_can_be_combined_with_polling() {
        assert_eq!(
            capture_settings(&["unused".into()])
                .unwrap()
                .1
                .read_buffer_bytes,
            65_536
        );
        for order in [
            [
                "unused",
                "1",
                "--read-poll-ms",
                "100",
                "--read-buffer-kib",
                "256",
            ],
            [
                "unused",
                "1",
                "--read-buffer-kib",
                "256",
                "--read-poll-ms",
                "100",
            ],
        ] {
            let (count, tuning) = capture_settings(&order.map(String::from)).unwrap();
            assert_eq!(count, 1);
            assert_eq!(tuning.read_buffer_bytes, 262_144);
            assert_eq!(tuning.read_poll_interval, Duration::from_millis(100));
        }
        for value in ["0", "1025", "-1", "1.5", "18446744073709551615"] {
            assert!(
                capture_settings(&["unused".into(), "--read-buffer-kib".into(), value.into()])
                    .is_err()
            );
        }
        for args in [
            vec!["unused", "--read-buffer-kib"],
            vec![
                "unused",
                "--read-buffer-kib",
                "64",
                "--read-buffer-kib",
                "256",
            ],
            vec!["unused", "--read-buffer-kib", "64", "extra"],
        ] {
            assert!(
                capture_settings(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err()
            );
        }
    }

    #[test]
    fn profile_logging_preserves_scan_failure_and_rejects_incomplete_log() {
        struct FailedOutput;
        impl Write for FailedOutput {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "synthetic log failure",
                ))
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let error = record_scan_profile::<()>(
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "synthetic cancellation",
            )),
            &mut FailedOutput,
            1,
            &"synthetic profile",
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(error.to_string().contains("synthetic cancellation"));
        assert!(error.to_string().contains("synthetic log failure"));
        assert_eq!(
            record_scan_profile(Ok(()), &mut FailedOutput, 1, &"synthetic profile")
                .unwrap_err()
                .kind(),
            io::ErrorKind::PermissionDenied
        );
        let mut output = Vec::new();
        assert_eq!(
            record_scan_profile(Ok(42), &mut output, 7, &"synthetic profile").unwrap(),
            42
        );
        assert!(
            String::from_utf8(output)
                .unwrap()
                .contains("run=7 profile=")
        );
    }

    #[test]
    fn existing_directory_is_rejected_before_hardware_access() {
        let path = std::env::temp_dir().join(format!(
            "wc3119-stability-test-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        let result = capture(&[path.to_str().unwrap().to_owned()]);
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::AlreadyExists
        );
        assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
        fs::remove_dir(path).unwrap();
    }

    #[test]
    fn wrong_channel_order_is_rejected() {
        let band = ImageBand {
            width: 1,
            rows: 1,
            mode: ColorMode::Rgb,
            pixels: vec![1, 2, 3],
            wire_data: vec![1, 2, 3],
        };
        assert!(verify_band(ColorMode::Rgb, 2, &band).is_err());

        let mut wrong_values = band;
        wrong_values.pixels = vec![1, 3, 2];
        assert!(verify_band(ColorMode::Rgb, 0, &wrong_values).is_err());
    }

    #[test]
    fn synthetic_gray_and_rgb_values_match_effective_wire_bytes() {
        let gray = ImageBand {
            width: 2,
            rows: 1,
            mode: ColorMode::Gray,
            pixels: vec![17, 34],
            wire_data: vec![17, 34, 0, 0],
        };
        verify_band(ColorMode::Gray, 0, &gray).unwrap();

        let rgb_interleaved = ImageBand {
            width: 2,
            rows: 1,
            mode: ColorMode::Rgb,
            pixels: vec![10, 20, 30, 40, 50, 60],
            wire_data: vec![10, 20, 30, 40, 50, 60, 0],
        };
        verify_band(ColorMode::Rgb, 0, &rgb_interleaved).unwrap();

        let rgb_planar = ImageBand {
            width: 2,
            rows: 2,
            mode: ColorMode::Rgb,
            pixels: vec![10, 20, 30, 40, 50, 60, 70, 80, 90, 100, 110, 120],
            wire_data: vec![10, 40, 20, 50, 30, 60, 70, 100, 80, 110, 90, 120],
        };
        verify_band(ColorMode::Rgb, 1, &rgb_planar).unwrap();

        let mut wrong_planar_values = rgb_planar;
        wrong_planar_values.pixels[9..12].copy_from_slice(&[70, 90, 80]);
        assert!(verify_band(ColorMode::Rgb, 1, &wrong_planar_values).is_err());
    }

    #[test]
    fn run_pattern_has_expected_order_and_counts_for_twenty_jobs() {
        let runs: Vec<_> = (0..20).map(run_settings).collect();
        assert_eq!(runs[0], (ColorMode::Rgb, 600));
        assert_eq!(runs[1], (ColorMode::Rgb, 600));
        assert_eq!(runs[2], (ColorMode::Rgb, 300));
        assert_eq!(runs[3], (ColorMode::Gray, 600));
        assert_eq!(
            runs.iter()
                .filter(|&&run| run == (ColorMode::Rgb, 600))
                .count(),
            10
        );
        assert_eq!(
            runs.iter()
                .filter(|&&run| run == (ColorMode::Rgb, 300))
                .count(),
            5
        );
        assert_eq!(
            runs.iter()
                .filter(|&&run| run == (ColorMode::Gray, 600))
                .count(),
            5
        );
    }
}
