//! Development-only capture of actual scanner bytes, not a user-facing scan application.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
};
use workcentre_3119::{
    protocol::Capabilities,
    scan::{self, ColorMode, ScanRequest},
};

fn new_file(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).open(path)
}

fn settings(args: &[String]) -> io::Result<(ColorMode, u32, bool)> {
    let invalid = || {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Expected NEW_DIRECTORY gray|rgb DPI [--cancel-after-band]",
        )
    };
    if !(3..=4).contains(&args.len()) || (args.len() == 4 && args[3] != "--cancel-after-band") {
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
    Ok((mode, dpi, args.len() == 4))
}

fn capture(args: &[String]) -> io::Result<()> {
    let (mode, dpi, cancel_after_band) = settings(args)?;
    let directory = Path::new(&args[0]);
    // Atomic directory creation fails for every existing target, before touching USB.
    fs::create_dir(directory)?;
    let mut evidence = new_file(&directory.join("evidence.txt"))?;
    let result = (|| {
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
        let summary = scan::scan_with_evidence(&cancel, prepare, |band| {
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
            if cancel_after_band {
                cancel.store(true, Ordering::Relaxed);
            }
            Ok(())
        })?;
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
            "Development capture: capture_scan NEW_DIRECTORY gray|rgb DPI [--cancel-after-band]\nCreates private diagnostic files. Requires paired scanner MI_00. No WIA integration."
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
            (ColorMode::Rgb, 150, true)
        );
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
