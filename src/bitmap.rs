use crate::scan::{ColorMode, ImageBand, ScanSummary};
use std::io::{self, Seek, SeekFrom, Write};

const FILE_HEADER_BYTES: u64 = 14;
const INFO_HEADER_BYTES: u64 = 40;
const GRAY_PALETTE_BYTES: u64 = 256 * 4;
const MAX_INPUT_ROW_BYTES: usize = 65_536;
const MAX_PIXEL_BYTES: usize = 256 * 1024 * 1024;
const MAX_BANDS: u32 = 4096;

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

fn dpi_to_ppm(dpi: u32) -> io::Result<i32> {
    if dpi == 0 {
        return Err(invalid_input("BMP DPI must be non-zero"));
    }
    // Pixels per metre = dpi * 10000 / 254, rounded to the nearest integer.
    let ppm = u64::from(dpi)
        .checked_mul(10_000)
        .and_then(|value| value.checked_add(127))
        .ok_or_else(|| invalid_input("BMP DPI conversion overflow"))?
        / 254;
    i32::try_from(ppm).map_err(|_| invalid_input("BMP pixels-per-metre does not fit i32"))
}

fn write_bounded<W: Write + ?Sized>(writer: &mut W, mut bytes: &[u8]) -> io::Result<()> {
    while !bytes.is_empty() {
        match writer.write(bytes) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "BMP writer made no progress",
                ));
            }
            Ok(written) if written <= bytes.len() => bytes = &bytes[written..],
            Ok(written) => {
                return Err(invalid_data(format!(
                    "BMP writer returned more bytes ({written}) than provided"
                )));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct BandGeometry {
    row_bytes: usize,
    row_stride: usize,
    pixel_bytes: usize,
}

fn validate_band_geometry(band: &ImageBand, mode: ColorMode) -> io::Result<BandGeometry> {
    if band.mode != mode {
        return Err(invalid_data("Image band mode differs from BMP mode"));
    }
    if band.width == 0 || band.rows == 0 {
        return Err(invalid_data("Image band width and rows must be non-zero"));
    }
    if band.width > i32::MAX as u32 {
        return Err(invalid_data("BMP width does not fit i32"));
    }
    let channels = match mode {
        ColorMode::Gray => 1u64,
        ColorMode::Rgb => 3u64,
    };
    let row_bytes = u64::from(band.width)
        .checked_mul(channels)
        .ok_or_else(|| invalid_data("Image row size overflow"))?;
    if row_bytes == 0 || row_bytes > MAX_INPUT_ROW_BYTES as u64 {
        return Err(invalid_data("Image row exceeds the 65536-byte limit"));
    }
    let row_bytes = usize::try_from(row_bytes)
        .map_err(|_| invalid_data("Image row size does not fit usize"))?;
    let pixel_bytes = row_bytes
        .checked_mul(band.rows as usize)
        .ok_or_else(|| invalid_data("Image pixel size overflow"))?;
    if band.pixels.len() != pixel_bytes {
        return Err(invalid_data(format!(
            "Image band pixel length {} does not match expected {pixel_bytes}",
            band.pixels.len()
        )));
    }
    let row_stride = row_bytes
        .checked_add(3)
        .ok_or_else(|| invalid_data("BMP row stride overflow"))?
        & !3;
    Ok(BandGeometry {
        row_bytes,
        row_stride,
        pixel_bytes,
    })
}

fn validate_accumulation(
    current_height: u32,
    current_bands: u32,
    current_bytes: usize,
    width: u32,
    rows: u32,
    pixel_bytes: usize,
    row_stride: usize,
) -> io::Result<(u32, u32, usize)> {
    if width == 0 || width > i32::MAX as u32 || rows == 0 || row_stride == 0 {
        return Err(invalid_data("Invalid BMP image geometry"));
    }
    if row_stride > MAX_INPUT_ROW_BYTES + 3 {
        return Err(invalid_data("BMP row stride exceeds the bounded row limit"));
    }
    if current_bands >= MAX_BANDS {
        return Err(invalid_data("BMP image exceeds the 4096-band limit"));
    }
    let next_bands = current_bands
        .checked_add(1)
        .ok_or_else(|| invalid_data("BMP band count overflow"))?;
    let next_height = current_height
        .checked_add(rows)
        .ok_or_else(|| invalid_data("BMP image height overflow"))?;
    if next_height > i32::MAX as u32 {
        return Err(invalid_data("BMP image height does not fit i32"));
    }
    let next_bytes = current_bytes
        .checked_add(pixel_bytes)
        .ok_or_else(|| invalid_data("BMP pixel byte count overflow"))?;
    if next_bytes > MAX_PIXEL_BYTES {
        return Err(invalid_data("BMP image exceeds the 256 MiB pixel limit"));
    }
    Ok((next_height, next_bands, next_bytes))
}

struct Poison {
    kind: io::ErrorKind,
    message: String,
}

impl Poison {
    fn capture(error: &io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }

    fn error(&self) -> io::Error {
        io::Error::new(self.kind, self.message.clone())
    }
}

/// Incrementally writes a top-down uncompressed BMP from decoded scan bands.
///
/// The output is deliberately uncommitted until [`Self::finish`] writes the
/// final `BM` signature. Any error means the caller must discard the output,
/// because a write can fail after partially changing the stream.
/// Input is limited to 65536 bytes per row, 256 MiB of pixels and 4096 bands.
/// The encoder changes channel order and row padding only, preserving samples.
pub struct BmpEncoder<'a, W: Write + Seek> {
    writer: &'a mut W,
    mode: ColorMode,
    pixels_per_metre: i32,
    width: Option<u32>,
    height: u32,
    bands: u32,
    bytes: usize,
    row_stride: usize,
    pixel_offset: u64,
    header_written: bool,
    poisoned: Option<Poison>,
}

impl<'a, W: Write + Seek> BmpEncoder<'a, W> {
    /// Starts an encoder on an empty stream. The current stream contents are
    /// checked with `SeekFrom::End(0)` before any output is written.
    pub fn new(writer: &'a mut W, dpi: u32, mode: ColorMode) -> io::Result<Self> {
        let pixels_per_metre = dpi_to_ppm(dpi)?;
        let original_position = writer.stream_position()?;
        let end = writer.seek(SeekFrom::End(0))?;
        if end != 0 {
            let _ = writer.seek(SeekFrom::Start(original_position));
            return Err(invalid_input("BMP output stream must be empty"));
        }
        let pixel_offset = FILE_HEADER_BYTES
            + INFO_HEADER_BYTES
            + if mode == ColorMode::Gray {
                GRAY_PALETTE_BYTES
            } else {
                0
            };
        Ok(Self {
            writer,
            mode,
            pixels_per_metre,
            width: None,
            height: 0,
            bands: 0,
            bytes: 0,
            row_stride: 0,
            pixel_offset,
            header_written: false,
            poisoned: None,
        })
    }

    fn poison<T>(&mut self, error: io::Error) -> io::Result<T> {
        self.poisoned = Some(Poison::capture(&error));
        Err(error)
    }

    fn write_initial_header(&mut self, width: u32) -> io::Result<()> {
        self.writer.seek(SeekFrom::Start(0))?;
        let mut file_header = [0u8; 14];
        file_header[10..14].copy_from_slice(&(self.pixel_offset as u32).to_le_bytes());
        write_bounded(self.writer, &file_header)?;

        let mut info_header = [0u8; 40];
        info_header[0..4].copy_from_slice(&40u32.to_le_bytes());
        info_header[4..8].copy_from_slice(&(width as i32).to_le_bytes());
        // Height remains zero until finish, then becomes negative for top-down rows.
        info_header[8..12].copy_from_slice(&0i32.to_le_bytes());
        info_header[12..14].copy_from_slice(&1u16.to_le_bytes());
        info_header[14..16].copy_from_slice(
            &match self.mode {
                ColorMode::Gray => 8u16,
                ColorMode::Rgb => 24u16,
            }
            .to_le_bytes(),
        );
        info_header[16..20].copy_from_slice(&0u32.to_le_bytes());
        info_header[20..24].copy_from_slice(&0u32.to_le_bytes());
        info_header[24..28].copy_from_slice(&self.pixels_per_metre.to_le_bytes());
        info_header[28..32].copy_from_slice(&self.pixels_per_metre.to_le_bytes());
        info_header[32..36].copy_from_slice(
            &if self.mode == ColorMode::Gray {
                256u32
            } else {
                0u32
            }
            .to_le_bytes(),
        );
        info_header[36..40].copy_from_slice(&0u32.to_le_bytes());
        write_bounded(self.writer, &info_header)?;

        if self.mode == ColorMode::Gray {
            let mut palette = [0u8; 256 * 4];
            for (value, entry) in palette.chunks_exact_mut(4).enumerate() {
                entry[..3].fill(value as u8);
            }
            write_bounded(self.writer, &palette)?;
        }
        self.header_written = true;
        Ok(())
    }

    fn write_band(&mut self, band: &ImageBand, geometry: BandGeometry) -> io::Result<()> {
        let padding_len = geometry.row_stride - geometry.row_bytes;
        let padding = [0u8; 3];
        let mut converted = vec![0u8; geometry.row_bytes];
        for row in band.pixels.chunks_exact(geometry.row_bytes) {
            match self.mode {
                ColorMode::Gray => write_bounded(self.writer, row)?,
                ColorMode::Rgb => {
                    for (destination, source) in
                        converted.chunks_exact_mut(3).zip(row.chunks_exact(3))
                    {
                        destination.copy_from_slice(&[source[2], source[1], source[0]]);
                    }
                    write_bounded(self.writer, &converted)?;
                }
            }
            write_bounded(self.writer, &padding[..padding_len])?;
        }
        Ok(())
    }

    /// Appends one decoded band in scan order, bounded to one row at a time.
    pub fn push(&mut self, band: &ImageBand) -> io::Result<()> {
        if let Some(poison) = &self.poisoned {
            return Err(poison.error());
        }
        let geometry = match validate_band_geometry(band, self.mode) {
            Ok(geometry) => geometry,
            Err(error) => return self.poison(error),
        };
        if self.width.is_some_and(|width| width != band.width) {
            return self.poison(invalid_data("Image band width changed"));
        }
        let (next_height, next_bands, next_bytes) = match validate_accumulation(
            self.height,
            self.bands,
            self.bytes,
            band.width,
            band.rows,
            geometry.pixel_bytes,
            geometry.row_stride,
        ) {
            Ok(next) => next,
            Err(error) => return self.poison(error),
        };
        if self.width.is_none() {
            if let Err(error) = self.write_initial_header(band.width) {
                return self.poison(error);
            }
            self.width = Some(band.width);
            self.row_stride = geometry.row_stride;
        }
        if let Err(error) = self.write_band(band, geometry) {
            return self.poison(error);
        }
        self.height = next_height;
        self.bands = next_bands;
        self.bytes = next_bytes;
        Ok(())
    }

    fn write_at(&mut self, offset: u64, bytes: &[u8]) -> io::Result<()> {
        self.writer.seek(SeekFrom::Start(offset))?;
        write_bounded(self.writer, bytes)
    }

    /// Finalizes the BMP only when the supplied scan summary exactly matches
    /// all successfully delivered bands. The final signature write is last.
    /// Call this only after the scanner job, including cleanup, succeeds.
    /// Success leaves the stream position at byte 2; the caller owns any later
    /// positioning, flushing and publication. Every error invalidates the output,
    /// even if some or all of the signature was written before the error.
    pub fn finish(mut self, summary: &ScanSummary) -> io::Result<()> {
        if let Some(poison) = &self.poisoned {
            return Err(poison.error());
        }
        let width = self
            .width
            .ok_or_else(|| invalid_data("Cannot finish BMP without an image band"))?;
        if !self.header_written
            || summary.width != width
            || summary.height != self.height
            || summary.bands != self.bands
            || summary.bytes != self.bytes
        {
            return Err(invalid_data(
                "BMP scan summary does not match delivered image data",
            ));
        }
        let image_size = self
            .row_stride
            .checked_mul(self.height as usize)
            .ok_or_else(|| invalid_data("BMP image size overflow"))?;
        let file_size = self
            .pixel_offset
            .checked_add(image_size as u64)
            .ok_or_else(|| invalid_data("BMP file size overflow"))?;
        let file_size =
            u32::try_from(file_size).map_err(|_| invalid_data("BMP file size does not fit u32"))?;
        let height = i32::try_from(self.height)
            .map_err(|_| invalid_data("BMP image height does not fit i32"))?;
        let image_size = u32::try_from(image_size)
            .map_err(|_| invalid_data("BMP image size does not fit u32"))?;
        self.write_at(2, &file_size.to_le_bytes())?;
        self.write_at(22, &(-height).to_le_bytes())?;
        self.write_at(34, &image_size.to_le_bytes())?;
        // Do not seek after this operation. A successful BM signature is the
        // final commit point, while any error requires discarding the stream.
        self.writer.seek(SeekFrom::Start(0))?;
        write_bounded(self.writer, b"BM")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn band(width: u32, rows: u32, mode: ColorMode, pixels: &[u8]) -> ImageBand {
        ImageBand {
            width,
            rows,
            mode,
            pixels: pixels.to_vec(),
            wire_data: Vec::new(),
        }
    }

    fn summary(width: u32, height: u32, bands: u32, bytes: usize) -> ScanSummary {
        ScanSummary {
            width,
            height,
            bands,
            bytes,
        }
    }

    fn le_u16(bytes: &[u8], offset: usize) -> u16 {
        u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
    }

    fn le_u32(bytes: &[u8], offset: usize) -> u32 {
        u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    fn le_i32(bytes: &[u8], offset: usize) -> i32 {
        i32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
    }

    struct ShortWriter {
        inner: Cursor<Vec<u8>>,
        max_write: usize,
    }

    impl Write for ShortWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let length = bytes.len().min(self.max_write);
            self.inner.write(&bytes[..length])
        }

        fn flush(&mut self) -> io::Result<()> {
            panic!("BMP encoding must not require flush")
        }
    }

    impl Seek for ShortWriter {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }

    struct ZeroWriter {
        inner: Cursor<Vec<u8>>,
    }

    impl Write for ZeroWriter {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            Ok(0)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    impl Seek for ZeroWriter {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }

    struct ErrorWriter {
        inner: Cursor<Vec<u8>>,
        calls: usize,
        kind: io::ErrorKind,
        message: &'static str,
    }

    impl Write for ErrorWriter {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            Err(io::Error::new(self.kind, self.message))
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    impl Seek for ErrorWriter {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }

    struct OversizedWriter {
        inner: Cursor<Vec<u8>>,
        calls: usize,
    }

    impl Write for OversizedWriter {
        fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            Ok(usize::MAX)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    impl Seek for OversizedWriter {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            self.inner.seek(position)
        }
    }

    struct SeekFailWriter {
        inner: Cursor<Vec<u8>>,
    }

    impl Write for SeekFailWriter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.inner.write(bytes)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.inner.flush()
        }
    }

    impl Seek for SeekFailWriter {
        fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
            if matches!(position, SeekFrom::Start(2)) {
                return Err(io::Error::other("synthetic seek failure"));
            }
            self.inner.seek(position)
        }
    }

    #[test]
    fn writes_gray_palette_top_down_padding_dpi_and_summary() {
        let mut output = Cursor::new(Vec::new());
        let first = band(3, 1, ColorMode::Gray, &[1, 2, 3]);
        let second = band(3, 1, ColorMode::Gray, &[254, 0, 127]);
        let result = {
            let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
            encoder.push(&first).unwrap();
            encoder.push(&second).unwrap();
            encoder.finish(&summary(3, 2, 2, 6))
        };
        assert!(result.is_ok());

        let bytes = output.into_inner();
        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(le_u32(&bytes, 2), bytes.len() as u32);
        assert_eq!(le_u32(&bytes, 10), 14 + 40 + 256 * 4);
        assert_eq!(le_u32(&bytes, 14), 40);
        assert_eq!(le_i32(&bytes, 18), 3);
        assert_eq!(le_i32(&bytes, 22), -2);
        assert_eq!(le_u16(&bytes, 26), 1);
        assert_eq!(le_u16(&bytes, 28), 8);
        assert_eq!(le_u32(&bytes, 30), 0);
        assert_eq!(le_u32(&bytes, 34), 8);
        assert_eq!(le_i32(&bytes, 38), 11_811);
        assert_eq!(le_i32(&bytes, 42), 11_811);
        assert_eq!(le_u32(&bytes, 46), 256);
        assert_eq!(le_u32(&bytes, 50), 0);
        for value in 0..=255u8 {
            let offset = 54 + usize::from(value) * 4;
            assert_eq!(&bytes[offset..offset + 4], &[value, value, value, 0]);
        }
        assert_eq!(&bytes[1078..], &[1, 2, 3, 0, 254, 0, 127, 0]);
    }

    #[test]
    fn writes_rgb_as_bgr_with_top_down_rows_and_padding() {
        let mut output = Cursor::new(Vec::new());
        let pixels = [1, 2, 3, 10, 20, 30, 100, 110, 120, 200, 210, 220];
        let result = {
            let mut encoder = BmpEncoder::new(&mut output, 75, ColorMode::Rgb).unwrap();
            encoder.push(&band(2, 2, ColorMode::Rgb, &pixels)).unwrap();
            encoder.finish(&summary(2, 2, 1, pixels.len()))
        };
        assert!(result.is_ok());

        let bytes = output.into_inner();
        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(le_u32(&bytes, 10), 54);
        assert_eq!(le_i32(&bytes, 18), 2);
        assert_eq!(le_i32(&bytes, 22), -2);
        assert_eq!(le_u16(&bytes, 28), 24);
        assert_eq!(le_u32(&bytes, 34), 16);
        assert_eq!(
            &bytes[54..],
            &[
                3, 2, 1, 30, 20, 10, 0, 0, 120, 110, 100, 220, 210, 200, 0, 0
            ]
        );
    }

    #[test]
    fn first_push_leaves_uncommitted_header_until_finish() {
        let mut output = Cursor::new(Vec::new());
        {
            let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
            encoder.push(&band(1, 1, ColorMode::Gray, &[7])).unwrap();
        }
        let bytes = output.into_inner();
        assert!(bytes.len() > 54);
        assert_ne!(&bytes[..2], b"BM");
        assert_eq!(le_i32(&bytes, 22), 0);
        assert_eq!(le_u32(&bytes, 34), 0);
    }

    #[test]
    fn short_writes_are_completed_without_flush() {
        let mut output = ShortWriter {
            inner: Cursor::new(Vec::new()),
            max_write: 2,
        };
        let result = {
            let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
            encoder
                .push(&band(2, 1, ColorMode::Gray, &[9, 10]))
                .unwrap();
            encoder.finish(&summary(2, 1, 1, 2))
        };
        assert!(result.is_ok());
        assert_eq!(output.inner.position(), 2);
        let bytes = output.inner.into_inner();
        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(&bytes[1078..], &[9, 10, 0, 0]);
    }

    #[test]
    fn errors_after_partial_header_pixels_or_signature_do_not_report_success() {
        struct LimitedWriter {
            inner: Cursor<Vec<u8>>,
            remaining: usize,
            failures: usize,
        }
        impl Write for LimitedWriter {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if self.remaining == 0 {
                    self.failures += 1;
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "synthetic cancellation after partial output",
                    ));
                }
                let length = bytes.len().min(self.remaining);
                let written = self.inner.write(&bytes[..length])?;
                self.remaining -= written;
                Ok(written)
            }
            fn flush(&mut self) -> io::Result<()> {
                panic!("BMP encoding must not require flush")
            }
        }
        impl Seek for LimitedWriter {
            fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
                self.inner.seek(position)
            }
        }
        // Stop inside the file header, image row, or final two-byte signature.
        for budget in [5, 1079, 1082 + 12 + 1] {
            let mut output = LimitedWriter {
                inner: Cursor::new(Vec::new()),
                remaining: budget,
                failures: 0,
            };
            let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
            let pushed = encoder.push(&band(2, 1, ColorMode::Gray, &[9, 10]));
            let finished = encoder.finish(&summary(2, 1, 1, 2));
            let error = finished.unwrap_err();
            assert_eq!(error.kind(), io::ErrorKind::Interrupted);
            assert_eq!(
                error.to_string(),
                "synthetic cancellation after partial output"
            );
            assert_eq!(pushed.is_ok(), budget > 1082);
            assert_eq!(output.failures, 1, "poisoned output must not be retried");
            assert_ne!(&output.inner.get_ref()[..2], b"BM");
            if budget > 1082 {
                assert_eq!(&output.inner.get_ref()[..2], b"B\0");
            }
        }
    }

    #[test]
    fn zero_write_poison_prevents_finish_and_preserves_error() {
        let mut output = ZeroWriter {
            inner: Cursor::new(Vec::new()),
        };
        let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
        let error = encoder
            .push(&band(1, 1, ColorMode::Gray, &[1]))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WriteZero);
        let finish_error = encoder.finish(&summary(1, 1, 1, 1)).unwrap_err();
        assert_eq!(finish_error.kind(), io::ErrorKind::WriteZero);
        assert!(finish_error.to_string().contains("no progress"));
        assert_ne!(output.inner.get_ref().get(..2), Some(&b"BM"[..]));
    }

    #[test]
    fn interrupted_write_is_returned_once_and_poisoned() {
        let mut output = ErrorWriter {
            inner: Cursor::new(Vec::new()),
            calls: 0,
            kind: io::ErrorKind::Interrupted,
            message: "synthetic cancellation",
        };
        let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
        let error = encoder
            .push(&band(1, 1, ColorMode::Gray, &[1]))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(error.to_string(), "synthetic cancellation");
        let finish_error = encoder.finish(&summary(1, 1, 1, 1)).unwrap_err();
        assert_eq!(finish_error.kind(), io::ErrorKind::Interrupted);
        assert_eq!(finish_error.to_string(), "synthetic cancellation");
        assert_eq!(output.calls, 1);
    }

    #[test]
    fn oversized_write_result_is_rejected_once_and_poisoned() {
        let mut output = OversizedWriter {
            inner: Cursor::new(Vec::new()),
            calls: 0,
        };
        let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
        let error = encoder
            .push(&band(1, 1, ColorMode::Gray, &[1]))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(error.to_string().contains("more bytes"));
        let finish_error = encoder.finish(&summary(1, 1, 1, 1)).unwrap_err();
        assert_eq!(finish_error.kind(), io::ErrorKind::InvalidData);
        assert!(finish_error.to_string().contains("more bytes"));
        assert_eq!(output.calls, 1);
    }

    #[test]
    fn write_error_poison_preserves_original_kind_and_text() {
        let mut output = ErrorWriter {
            inner: Cursor::new(Vec::new()),
            calls: 0,
            kind: io::ErrorKind::StorageFull,
            message: "synthetic storage full",
        };
        let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
        let error = encoder
            .push(&band(1, 1, ColorMode::Gray, &[1]))
            .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::StorageFull);
        assert_eq!(error.to_string(), "synthetic storage full");
        let finish_error = encoder.finish(&summary(1, 1, 1, 1)).unwrap_err();
        assert_eq!(finish_error.kind(), io::ErrorKind::StorageFull);
        assert_eq!(finish_error.to_string(), "synthetic storage full");
    }

    #[test]
    fn finish_seek_failure_does_not_commit_signature() {
        let mut output = SeekFailWriter {
            inner: Cursor::new(Vec::new()),
        };
        let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
        encoder.push(&band(1, 1, ColorMode::Gray, &[1])).unwrap();
        let error = encoder.finish(&summary(1, 1, 1, 1)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("seek failure"));
        assert_ne!(output.inner.get_ref().get(..2), Some(&b"BM"[..]));
    }

    #[test]
    fn new_rejects_nonempty_stream_without_changing_content() {
        let original = vec![0x9a, 0xbc, 0xde];
        let mut output = Cursor::new(original.clone());
        output.set_position(1);
        let error = match BmpEncoder::new(&mut output, 300, ColorMode::Gray) {
            Ok(_) => panic!("nonempty stream was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(output.into_inner(), original);
    }

    #[test]
    fn new_rejects_zero_or_unrepresentable_dpi_without_writing() {
        for dpi in [0, u32::MAX] {
            let mut output = Cursor::new(Vec::new());
            let error = match BmpEncoder::new(&mut output, dpi, ColorMode::Gray) {
                Ok(_) => panic!("invalid dpi was accepted: {dpi}"),
                Err(error) => error,
            };
            assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
            assert!(output.into_inner().is_empty());
        }
    }

    #[test]
    fn empty_or_partial_summary_cannot_finish() {
        let mut output = Cursor::new(Vec::new());
        let encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
        let error = encoder.finish(&summary(0, 0, 0, 0)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert!(output.into_inner().is_empty());

        let mut output = Cursor::new(Vec::new());
        let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
        encoder.push(&band(2, 1, ColorMode::Gray, &[1, 2])).unwrap();
        let error = encoder.finish(&summary(2, 2, 2, 4)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        assert_ne!(output.into_inner().get(..2), Some(&b"BM"[..]));
    }

    #[test]
    fn malformed_bands_and_mode_changes_are_rejected() {
        for invalid_band in [
            band(0, 1, ColorMode::Gray, &[]),
            band(1, 0, ColorMode::Gray, &[]),
            band(1, 1, ColorMode::Gray, &[]),
            band(1, 1, ColorMode::Rgb, &[1]),
            band(65_537, 1, ColorMode::Gray, &[]),
        ] {
            let mut output = Cursor::new(Vec::new());
            let mut encoder = BmpEncoder::new(&mut output, 300, invalid_band.mode).unwrap();
            assert!(encoder.push(&invalid_band).is_err());
            assert!(encoder.finish(&summary(0, 0, 0, 0)).is_err());
        }

        let mut output = Cursor::new(Vec::new());
        let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
        encoder.push(&band(1, 1, ColorMode::Gray, &[1])).unwrap();
        assert!(encoder.push(&band(2, 1, ColorMode::Gray, &[2, 3])).is_err());

        let mut output = Cursor::new(Vec::new());
        let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
        encoder.push(&band(1, 1, ColorMode::Gray, &[1])).unwrap();
        assert!(
            encoder
                .push(&band(1, 1, ColorMode::Rgb, &[2, 3, 4]))
                .is_err()
        );
    }

    #[test]
    fn summary_dimensions_bands_and_bytes_must_match_delivered_pixels() {
        for summary in [
            summary(3, 1, 1, 3),
            summary(2, 2, 1, 2),
            summary(2, 1, 2, 2),
            summary(2, 1, 1, 3),
        ] {
            let mut output = Cursor::new(Vec::new());
            let mut encoder = BmpEncoder::new(&mut output, 300, ColorMode::Gray).unwrap();
            encoder.push(&band(2, 1, ColorMode::Gray, &[1, 2])).unwrap();
            assert!(encoder.finish(&summary).is_err());
        }
    }

    #[test]
    fn accumulation_limits_are_checked_without_allocating_a_whole_image() {
        assert!(validate_accumulation(0, 4096, 0, 1, 1, 1, 4).is_err());
        assert!(validate_accumulation(0, 0, 256 * 1024 * 1024, 1, 1, 1, 4).is_err());
        assert!(validate_accumulation(i32::MAX as u32 - 1, 0, 0, 1, 2, 2, 4).is_err());
    }
}
