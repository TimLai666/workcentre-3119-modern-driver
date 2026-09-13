//! Independent decoding of documented xerox_mfp wire fields.
//! Field provenance: SANE 1.4.0 xerox_mfp.c, INQUIRY and inq_dpi_bits.
//! Tests use synthetic frames; reported capabilities still need hardware validation.

use std::io::{self, ErrorKind};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Capabilities {
    pub identity: String,
    pub resolution_mask: u32,
    pub mode_mask: u8,
    /// Protocol geometry units are 1/1200 inch, not output pixel counts.
    pub width_units: u32,
    pub length_units: u32,
    pub flatbed_length_units: u32,
    /// Preserve the wire value; interpretation is deferred until image decoding.
    pub line_order: u8,
    pub compression_mask: u8,
}

impl Capabilities {
    pub fn parse(bytes: &[u8]) -> io::Result<Self> {
        let invalid = |message| io::Error::new(ErrorKind::InvalidData, message);
        if bytes.len() < 4 || bytes[0] != 0xa8 || usize::from(bytes[2]) + 3 != bytes.len() {
            return Err(invalid("Malformed INQUIRY response header or length"));
        }
        if bytes[1] != 0 {
            return Err(invalid("Device returned a non-success INQUIRY status"));
        }
        if bytes.len() < 70 || bytes[3] != 0x10 {
            return Err(invalid(
                "INQUIRY response is not a complete product-info frame",
            ));
        }
        if bytes[4..36]
            .iter()
            .any(|&b| b != 0 && !(0x20..=0x7e).contains(&b))
        {
            return Err(invalid("INQUIRY identity contains invalid text"));
        }
        let identity: String = bytes[4..36]
            .iter()
            .map(|&b| if b == 0 { ' ' } else { char::from(b) })
            .collect();
        let integer = |offset| {
            u32::from_be_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("checked frame length"),
            )
        };
        let result = Self {
            identity: identity.trim().to_owned(),
            resolution_mask: (u32::from(bytes[55]) << 16)
                | (u32::from(bytes[36]) << 8)
                | u32::from(bytes[37]),
            mode_mask: bytes[39],
            width_units: integer(40),
            length_units: integer(44),
            flatbed_length_units: integer(60),
            line_order: bytes[49],
            compression_mask: bytes[50],
        };
        if result.identity.is_empty()
            || result.resolution_mask == 0
            || result.mode_mask == 0
            || result.width_units == 0
            || result.length_units == 0
            || result.flatbed_length_units == 0
        {
            return Err(invalid("INQUIRY response contains empty capabilities"));
        }
        Ok(result)
    }

    /// Recognized, reported resolutions only. These are not verified optical resolutions.
    pub fn resolutions(&self) -> Vec<u32> {
        [
            (75, 0),
            (100, 12),
            (150, 1),
            (200, 4),
            (300, 5),
            (600, 8),
            (1200, 11),
            (2400, 15),
            (4800, 17),
            (9600, 19),
        ]
        .into_iter()
        .filter_map(|(dpi, bit)| (self.resolution_mask & (1 << bit) != 0).then_some(dpi))
        .collect()
    }
}
