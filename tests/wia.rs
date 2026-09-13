//! WIA scalar property validation, independent of COM registration or a scanner.
use std::{
    io::{self, Seek, SeekFrom, Write},
    sync::atomic::AtomicBool,
};
use workcentre_3119::{
    scan::ColorMode,
    wia::{BMP_FORMAT, FlatbedSettings, scan_bmp},
};

fn settings() -> FlatbedSettings {
    FlatbedSettings {
        x_resolution: 300,
        y_resolution: 300,
        x_position: 3,
        y_position: 6,
        x_extent: 300,
        y_extent: 600,
        data_type: 2,
        depth: 8,
        brightness: 0,
        contrast: 0,
        compression: 0,
        format: BMP_FORMAT,
    }
}

#[test]
fn scalar_properties_map_to_exact_protocol_units_and_modes() {
    let gray = settings().to_request().unwrap();
    assert_eq!(
        (
            gray.dpi,
            gray.mode,
            gray.x_units,
            gray.y_units,
            gray.width_units,
            gray.height_units
        ),
        (300, ColorMode::Gray, 12, 24, 1200, 2400)
    );
    let color = FlatbedSettings {
        data_type: 3,
        depth: 24,
        ..settings()
    }
    .to_request()
    .unwrap();
    assert_eq!(color.mode, ColorMode::Rgb);
    assert_eq!(
        BMP_FORMAT,
        [
            0xab, 0x3c, 0x6b, 0xb9, 0x28, 0x07, 0xd3, 0x11, 0x9d, 0x7b, 0x00, 0x00, 0xf8, 0x1e,
            0xf3, 0x2e
        ]
    );
}

#[test]
fn every_resolution_accepts_only_exact_position_steps() {
    for (dpi, pixel_step) in [(75, 3), (100, 1), (150, 3), (200, 2), (300, 3), (600, 6)] {
        let valid = FlatbedSettings {
            x_resolution: dpi,
            y_resolution: dpi,
            x_position: pixel_step,
            y_position: pixel_step,
            x_extent: 100,
            y_extent: 100,
            ..settings()
        };
        let request = valid.to_request().unwrap();
        assert_eq!(request.x_units, (pixel_step * 1200 / dpi) as u32);
        assert_eq!(request.width_units, (100 * 1200 / dpi) as u32);
        for coordinate in 0..pixel_step {
            let result = FlatbedSettings {
                x_position: coordinate,
                ..valid
            }
            .to_request();
            assert_eq!(result.is_ok(), coordinate == 0);
            let result = FlatbedSettings {
                y_position: coordinate,
                ..valid
            }
            .to_request();
            assert_eq!(result.is_ok(), coordinate == 0);
        }
    }
}

fn invalid_settings() -> Vec<FlatbedSettings> {
    vec![
        FlatbedSettings {
            x_resolution: 0,
            ..settings()
        },
        FlatbedSettings {
            x_resolution: -300,
            y_resolution: -300,
            ..settings()
        },
        FlatbedSettings {
            x_resolution: 1200,
            y_resolution: 1200,
            ..settings()
        },
        FlatbedSettings {
            y_resolution: 600,
            ..settings()
        },
        FlatbedSettings {
            data_type: 0,
            depth: 1,
            ..settings()
        },
        FlatbedSettings {
            data_type: 2,
            depth: 24,
            ..settings()
        },
        FlatbedSettings {
            data_type: 3,
            depth: 8,
            ..settings()
        },
        FlatbedSettings {
            data_type: 100,
            depth: 8,
            ..settings()
        },
        FlatbedSettings {
            brightness: 1,
            ..settings()
        },
        FlatbedSettings {
            contrast: -1,
            ..settings()
        },
        FlatbedSettings {
            compression: 1,
            ..settings()
        },
        FlatbedSettings {
            format: [0; 16],
            ..settings()
        },
        FlatbedSettings {
            x_position: -1,
            ..settings()
        },
        FlatbedSettings {
            y_position: -1,
            ..settings()
        },
        FlatbedSettings {
            x_extent: 0,
            ..settings()
        },
        FlatbedSettings {
            y_extent: -1,
            ..settings()
        },
        FlatbedSettings {
            x_position: 1,
            ..settings()
        },
        FlatbedSettings {
            y_position: 1,
            ..settings()
        },
        FlatbedSettings {
            x_extent: i32::MAX,
            ..settings()
        },
        FlatbedSettings {
            y_extent: i32::MAX,
            ..settings()
        },
        FlatbedSettings {
            x_position: i32::MAX,
            ..settings()
        },
        FlatbedSettings {
            x_position: 76800,
            ..settings()
        },
    ]
}

#[test]
fn unsupported_combinations_and_unrepresentable_geometry_are_rejected() {
    for invalid in invalid_settings() {
        let error = invalid.to_request().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput, "{invalid:?}");
    }
}

struct UntouchedOutput;
impl Write for UntouchedOutput {
    fn write(&mut self, _: &[u8]) -> io::Result<usize> {
        panic!("output touched before validation/cancellation")
    }
    fn flush(&mut self) -> io::Result<()> {
        panic!("output flushed before validation/cancellation")
    }
}
impl Seek for UntouchedOutput {
    fn seek(&mut self, _: SeekFrom) -> io::Result<u64> {
        panic!("output sought before validation/cancellation")
    }
}

#[test]
fn invalid_settings_and_pre_cancelled_jobs_do_not_touch_output() {
    for invalid in invalid_settings() {
        let error = scan_bmp(invalid, &AtomicBool::new(false), &mut UntouchedOutput).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }
    let error = scan_bmp(settings(), &AtomicBool::new(true), &mut UntouchedOutput).unwrap_err();
    assert_eq!(error.kind(), io::ErrorKind::Interrupted);
}
