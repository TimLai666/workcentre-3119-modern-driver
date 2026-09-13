//! Development-only repeated scanner stability check, not a user-facing scan application.

use std::{
    cell::Cell,
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::AtomicBool,
    time::Instant,
};

use workcentre_3119::{
    protocol::Capabilities,
    scan::{self, ColorMode, ImageBand, ScanRequest},
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
    let count = settings(args)?;
    let directory = Path::new(&args[0]);
    // Atomic directory creation rejects every existing target before USB access.
    fs::create_dir(directory)?;
    let mut diagnostics = new_file(&directory.join("diagnostics.log"))?;
    let result = (|| {
        writeln!(
            diagnostics,
            "stability_count={count}; pattern=rgb600,rgb600,rgb300,gray600"
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
            let scan_result = scan::scan_with_evidence(
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
            );
            let elapsed_ms = started.elapsed().as_millis();
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
scan_stability NEW_DIRECTORY [COUNT]\n\
Runs COUNT scans in one process using rgb600, rgb600, rgb300, gray600 repeatedly.\n\
COUNT defaults to 20 and must be an integer from 1 through 20. Example: scan_stability artifacts/stability 20\n\
Run with no arguments or --help to show this help. Requires Windows and a paired scanner MI_00.\n\
Each run discovers capabilities in its own scan session and scans the full reported flatbed.\n\
Only diagnostics.log is written during runs; no image or USB band bytes are saved. complete.txt is created only after every run succeeds.\n\
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
