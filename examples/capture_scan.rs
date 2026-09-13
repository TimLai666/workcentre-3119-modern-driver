//! Development-only capture of actual scanner bytes, not a user-facing scan application.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};
use workcentre_3119::{
    protocol::Capabilities,
    scan::{self, ColorMode, ScanRequest},
};

fn new_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Cancellation {
    None,
    AfterBand,
    AfterMs(u64),
}

fn settings(args: &[String]) -> io::Result<(ColorMode, u32, Cancellation)> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected NEW_DIRECTORY gray|rgb DPI [--cancel-after-band | --cancel-after-ms N], with N in 1..=120000",
        )
    };
    if !(3..=5).contains(&args.len()) {
        return Err(invalid());
    }
    let mode = match args[1].as_str() {
        "gray" => ColorMode::Gray,
        "rgb" => ColorMode::Rgb,
        _ => return Err(invalid()),
    };
    let dpi = args[2].parse().map_err(|_| invalid())?;
    if ![75, 100, 150, 200, 300, 600].contains(&dpi) {
        return Err(invalid());
    }
    let cancellation = match args.get(3).map(String::as_str) {
        None => Cancellation::None,
        Some("--cancel-after-band") if args.len() == 4 => Cancellation::AfterBand,
        Some("--cancel-after-ms") if args.len() == 5 => {
            let milliseconds = args[4].parse::<u64>().map_err(|_| invalid())?;
            if !(1..=120_000).contains(&milliseconds) {
                return Err(invalid());
            }
            Cancellation::AfterMs(milliseconds)
        }
        _ => return Err(invalid()),
    };
    Ok((mode, dpi, cancellation))
}

fn run_with_timer<T>(
    cancel: &AtomicBool,
    milliseconds: u64,
    scan: impl FnOnce() -> io::Result<T>,
) -> io::Result<T> {
    let delay = Duration::from_millis(milliseconds);
    thread::scope(|scope| {
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let timer = scope.spawn(move || {
            if done_rx.recv_timeout(delay).is_err() {
                cancel.store(true, Ordering::Relaxed);
            }
        });
        let result = scan();
        let _ = done_tx.send(());
        timer
            .join()
            .map_err(|_| io::Error::other("Cancellation timer failed"))?;
        if result.is_ok() && cancel.load(Ordering::Relaxed) {
            Err(io::Error::new(
                io::ErrorKind::Interrupted,
                "Scan cancelled by timer",
            ))
        } else {
            result
        }
    })
}

fn capture(args: &[String]) -> io::Result<()> {
    let (mode, dpi, cancellation) = settings(args)?;
    let directory = Path::new(&args[0]);
    // Atomic directory creation fails for every existing target, before touching USB.
    fs::create_dir(directory)?;
    let mut evidence = new_file(&directory.join("evidence.txt"))?;
    let result = (|| {
        writeln!(evidence, "Requested cancellation: {cancellation:?}")?;
        evidence.sync_all()?;
        let mut preparation = new_file(&directory.join("inquiry.txt"))?;
        let prepare = |usb: &workcentre_3119::InquiryEvidence| {
            writeln!(
                preparation,
                "USB device: {:02x?}\nInterface: {:02x?}\nBulk IN: {:02x} max={}\nBulk OUT: {:02x} max={}\nINQUIRY: {:02x?}",
                usb.device_descriptor,
                usb.interface_descriptor,
                usb.bulk_in,
                usb.bulk_in_max_packet,
                usb.bulk_out,
                usb.bulk_out_max_packet,
                usb.reply
            )?;
            let caps = Capabilities::parse(&usb.reply)?;
            let request = ScanRequest {
                dpi,
                mode,
                x_units: 0,
                y_units: 0,
                width_units: caps.width_units,
                height_units: caps.flatbed_length_units.min(caps.length_units),
            };
            writeln!(
                preparation,
                "Request: {request:?}\nNo brightness, gamma or geometry correction. Dimensions come from READ metadata."
            )?;
            preparation.sync_all()?;
            Ok(request)
        };
        let mut raster = new_file(&directory.join("pixels.partial"))?;
        let cancel = AtomicBool::new(false);
        let mut bands = 0;
        let started = std::time::Instant::now();
        let scan = || {
            scan::scan_with_evidence(&cancel, prepare, |band| {
                bands += 1;
                let mut wire = new_file(&directory.join(format!("band-{bands:04}.bin")))?;
                wire.write_all(&band.wire_data)?;
                wire.sync_all()?;
                raster.write_all(&band.pixels)?;
                writeln!(
                    evidence,
                    "Band {bands}: width={} rows={} wire={} pixels={}",
                    band.width,
                    band.rows,
                    band.wire_data.len(),
                    band.pixels.len()
                )?;
                evidence.flush()?;
                eprintln!("Band {bands}: {} x {}", band.width, band.rows);
                if cancellation == Cancellation::AfterBand {
                    cancel.store(true, Ordering::Relaxed);
                }
                Ok(())
            })
        };
        let summary = match cancellation {
            Cancellation::AfterMs(milliseconds) => run_with_timer(&cancel, milliseconds, scan)?,
            Cancellation::None | Cancellation::AfterBand => scan()?,
        };
        raster.sync_all()?;
        drop(raster);
        let filename = if mode == ColorMode::Gray {
            "image.pgm"
        } else {
            "image.ppm"
        };
        let mut image = new_file(&directory.join(filename))?;
        writeln!(
            image,
            "{}\n{} {}\n255",
            if mode == ColorMode::Gray { "P5" } else { "P6" },
            summary.width,
            summary.height
        )?;
        let copied = io::copy(
            &mut File::open(directory.join("pixels.partial"))?,
            &mut image,
        )?;
        if copied != summary.bytes as u64 {
            return Err(io::Error::other("Raster output length changed"));
        }
        image.sync_all()?;
        writeln!(
            evidence,
            "Complete: {summary:?}; elapsed={:?}",
            started.elapsed()
        )?;
        evidence.sync_all()?;
        let mut marker = new_file(&directory.join("complete.txt"))?;
        writeln!(
            marker,
            "Successfully released scanner; image={filename}; {summary:?}"
        )?;
        marker.sync_all()?;
        eprintln!("Complete: {summary:?}");
        Ok(())
    })();
    if let Err(ref error) = result {
        let _ = writeln!(evidence, "INCOMPLETE: {error}");
        let _ = evidence.sync_all();
    }
    result
}

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.is_empty() || args == ["--help"] {
        println!(
            "Development capture. Usage:\n\
capture_scan NEW_DIRECTORY gray|rgb DPI\n\
capture_scan NEW_DIRECTORY gray|rgb DPI --cancel-after-band\n\
capture_scan NEW_DIRECTORY gray|rgb DPI --cancel-after-ms N\n\
By default no timed cancellation is requested; every scanner job still has a 120-second deadline plus bounded cleanup.\n\
NEW_DIRECTORY must be new. Modes: gray or rgb. DPI: 75, 100, 150, 200, 300, or 600.\n\
CANCEL is --cancel-after-band or --cancel-after-ms N, where N is 1..=120000; the flags are mutually exclusive.\n\
Example: capture_scan artifacts/sample gray 75\n\
Exit codes: 0 for a completed capture or help, 1 for invalid arguments or a failed/cancelled capture.\n\
Requires paired scanner MI_00. No WIA integration."
        );
        return std::process::ExitCode::SUCCESS;
    }
    match capture(&args) {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("Capture failed: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn invalid_capture_parameters_do_not_reach_usb() {
        for args in [
            vec![],
            vec!["unused", "gray", "0"],
            vec!["unused", "rgb", "1200"],
            vec!["unused", "unknown", "75"],
            vec!["unused", "gray", "75", "--bad"],
        ] {
            assert!(settings(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
        assert_eq!(
            settings(&["unused", "rgb", "150", "--cancel-after-band"].map(String::from)).unwrap(),
            (ColorMode::Rgb, 150, Cancellation::AfterBand)
        );
    }

    #[test]
    fn timed_cancellation_arguments_are_validated_before_usb_access() {
        assert_eq!(
            settings(&["unused", "gray", "75", "--cancel-after-ms", "1"].map(String::from))
                .unwrap(),
            (ColorMode::Gray, 75, Cancellation::AfterMs(1))
        );
        assert_eq!(
            settings(&["unused", "rgb", "600", "--cancel-after-ms", "120000"].map(String::from))
                .unwrap(),
            (ColorMode::Rgb, 600, Cancellation::AfterMs(120_000))
        );
        for args in [
            vec!["unused", "gray", "75", "--cancel-after-ms"],
            vec!["unused", "gray", "75", "--cancel-after-ms", "0"],
            vec!["unused", "gray", "75", "--cancel-after-ms", "120001"],
            vec!["unused", "gray", "75", "--cancel-after-ms", "not-a-number"],
            vec![
                "unused",
                "gray",
                "75",
                "--cancel-after-band",
                "--cancel-after-ms",
                "1",
            ],
        ] {
            assert!(settings(&args.into_iter().map(String::from).collect::<Vec<_>>()).is_err());
        }
    }

    #[test]
    fn timer_stops_when_scan_finishes() {
        let cancel = AtomicBool::new(false);
        let result = run_with_timer(&cancel, 1_000, || Ok::<_, io::Error>(7));

        assert_eq!(result.unwrap(), 7);
        assert!(!cancel.load(Ordering::Relaxed));
    }

    #[test]
    fn existing_directory_is_rejected_before_hardware_access() {
        let path = std::env::temp_dir().join(format!(
            "wc3119-test-{}-{:?}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&path).unwrap();
        let result = capture(&[
            path.to_str().unwrap().to_owned(),
            "gray".into(),
            "75".into(),
        ]);
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
        fs::remove_dir(path).unwrap();
    }
}
