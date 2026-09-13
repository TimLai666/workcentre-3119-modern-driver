//! A bounded WIA-to-scan-core mapping for the currently verified flatbed modes.
//!
//! This module does not implement a WIA minidriver or COM registration. It
//! validates the scalar settings that the future WIA adapter may provide,
//! converts WIA pixel geometry to the scanner protocol's 1/1200-inch units,
//! and exposes a BMP streaming entry point for that adapter.

use crate::{
    bitmap::BmpEncoder,
    scan::{ColorMode, ScanRequest, ScanSummary, scan_to},
};
use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
};

/// The Windows `WiaImgFmt_BMP` GUID in its in-memory Windows byte order.
///
/// This is `{B96B3CAB-0728-11D3-9D7B-0000F81EF32E}`. The initial adapter
/// accepts BMP only; other WIA formats have not been verified for this core.
pub const BMP_FORMAT: [u8; 16] = [
    0xab, 0x3c, 0x6b, 0xb9, 0x28, 0x07, 0xd3, 0x11, 0x9d, 0x7b, 0x00, 0x00, 0xf8, 0x1e, 0xf3, 0x2e,
];

const WIA_DATA_GRAYSCALE: i32 = 2;
const WIA_DATA_COLOR: i32 = 3;
const DEVICE_UNITS_PER_INCH: u64 = 1200;
const PROTOCOL_MAX_OFFSET_INCHES: u32 = 255;

/// Scalar settings supplied by a future WIA flatbed item.
///
/// Positions and extents use WIA pixels. The current scan protocol supports
/// only equal X/Y resolutions, 8-bit grayscale or 24-bit RGB, uncompressed
/// BMP output, and neutral brightness/contrast. Live device capabilities are
/// checked later by [`crate::scan::scan_to`] in the same USB session.
#[derive(Clone, Copy, Debug)]
pub struct FlatbedSettings {
    pub x_resolution: i32,
    pub y_resolution: i32,
    pub x_position: i32,
    pub y_position: i32,
    pub x_extent: i32,
    pub y_extent: i32,
    pub data_type: i32,
    pub depth: i32,
    pub brightness: i32,
    pub contrast: i32,
    pub compression: i32,
    pub format: [u8; 16],
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn supported_resolution(dpi: i32) -> bool {
    matches!(dpi, 75 | 100 | 150 | 200 | 300 | 600)
}

fn pixels_to_units(pixels: i32, dpi: i32, name: &str) -> io::Result<u32> {
    if pixels < 0 {
        return Err(invalid(format!("WIA {name} must be non-negative")));
    }
    let pixels = u64::try_from(pixels).map_err(|_| invalid(format!("Invalid WIA {name}")))?;
    let dpi = u64::try_from(dpi).map_err(|_| invalid("WIA resolution must be positive"))?;
    let scaled = pixels
        .checked_mul(DEVICE_UNITS_PER_INCH)
        .ok_or_else(|| invalid(format!("WIA {name} conversion overflow")))?;
    if scaled % dpi != 0 {
        return Err(invalid(format!(
            "WIA {name} cannot be represented at {dpi} DPI"
        )));
    }
    u32::try_from(scaled / dpi)
        .map_err(|_| invalid(format!("WIA {name} exceeds protocol geometry")))
}

fn validate_offset(units: u32, name: &str) -> io::Result<()> {
    if !units.is_multiple_of(12) {
        return Err(invalid(format!(
            "WIA {name} does not align to the protocol's 1/100-inch offset"
        )));
    }
    if units / 1200 > PROTOCOL_MAX_OFFSET_INCHES {
        return Err(invalid(format!("WIA {name} exceeds protocol offset range")));
    }
    Ok(())
}

fn validate_end(start: u32, extent: u32, name: &str) -> io::Result<()> {
    let end = u64::from(start)
        .checked_add(u64::from(extent))
        .ok_or_else(|| invalid(format!("WIA {name} range overflow")))?;
    if end > u64::from(u32::MAX) {
        return Err(invalid(format!(
            "WIA {name} range exceeds protocol geometry"
        )));
    }
    Ok(())
}

impl FlatbedSettings {
    /// Validate these scalar properties and map them to a scan request.
    ///
    /// This operation does not open USB or inspect live capabilities. A later
    /// scan checks the request against the capabilities returned by the same
    /// session that performs the job.
    pub fn to_request(self) -> io::Result<ScanRequest> {
        if !supported_resolution(self.x_resolution) || self.x_resolution != self.y_resolution {
            return Err(invalid(
                "WIA X/Y resolution must match and be one of 75, 100, 150, 200, 300 or 600 DPI",
            ));
        }
        let mode = match (self.data_type, self.depth) {
            (WIA_DATA_GRAYSCALE, 8) => ColorMode::Gray,
            (WIA_DATA_COLOR, 24) => ColorMode::Rgb,
            _ => {
                return Err(invalid(
                    "WIA data type/depth must be grayscale 8 bpp or color 24 bpp",
                ));
            }
        };
        if self.brightness != 0 || self.contrast != 0 {
            return Err(invalid(
                "WIA brightness and contrast must remain at neutral value 0",
            ));
        }
        if self.compression != 0 {
            return Err(invalid(
                "WIA compression is unsupported; use uncompressed data",
            ));
        }
        if self.format != BMP_FORMAT {
            return Err(invalid("WIA format is unsupported; use BMP"));
        }
        if self.x_extent <= 0 || self.y_extent <= 0 {
            return Err(invalid("WIA extents must be positive"));
        }

        let x_units = pixels_to_units(self.x_position, self.x_resolution, "XPOS")?;
        let y_units = pixels_to_units(self.y_position, self.y_resolution, "YPOS")?;
        let width_units = pixels_to_units(self.x_extent, self.x_resolution, "XEXTENT")?;
        let height_units = pixels_to_units(self.y_extent, self.y_resolution, "YEXTENT")?;
        validate_offset(x_units, "XPOS")?;
        validate_offset(y_units, "YPOS")?;
        validate_end(x_units, width_units, "XPOS + XEXTENT")?;
        validate_end(y_units, height_units, "YPOS + YEXTENT")?;

        Ok(ScanRequest {
            dpi: self.x_resolution as u32,
            mode,
            x_units,
            y_units,
            width_units,
            height_units,
        })
    }
}

/// Validate WIA scalar settings, start a real USB scan as BMP, and return the
/// dimensions actually delivered by the scanner.
///
/// The settings and pre-cancellation checks happen before the output stream
/// or USB session is touched. The scanner's actual READ dimensions are passed
/// to [`BmpEncoder::finish`]; this function does not crop, pad, or resample
/// them. Callers must discard the output on any error and must still provide
/// WIA property updates and the minidriver/COM integration separately.
pub fn scan_bmp<W: io::Write + io::Seek>(
    settings: FlatbedSettings,
    cancel: &AtomicBool,
    output: &mut W,
) -> io::Result<ScanSummary> {
    let request = settings.to_request()?;
    if cancel.load(Ordering::Relaxed) {
        return Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"));
    }

    encode_bmp(request, output, |sink| scan_to(request, cancel, sink))
}

fn encode_bmp<W: io::Write + io::Seek>(
    request: ScanRequest,
    output: &mut W,
    run: impl FnOnce(
        &mut dyn FnMut(&crate::scan::ImageBand) -> io::Result<()>,
    ) -> io::Result<ScanSummary>,
) -> io::Result<ScanSummary> {
    let mut encoder = BmpEncoder::new(output, request.dpi, request.mode)?;
    let summary = run(&mut |band| encoder.push(band))?;
    encoder.finish(&summary)?;
    Ok(summary)
}

/// Internal path for a validated request and an already exclusively held device.
#[cfg(windows)]
pub(crate) fn scan_request_bmp_in_session<W: io::Write + io::Seek>(
    usb: &mut crate::usb::UsbSession,
    request: ScanRequest,
    cancel: &AtomicBool,
    output: &mut W,
) -> (io::Result<ScanSummary>, crate::scan::SessionHealth) {
    let mut health = crate::scan::SessionHealth::Ready;
    let result = encode_bmp(request, output, |sink| {
        let (result, current) = crate::scan::scan_in_session(usb, request, cancel, sink);
        health = current;
        result
    });
    (result, health)
}
