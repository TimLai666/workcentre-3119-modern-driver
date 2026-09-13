use workcentre_3119::protocol::Capabilities;

// Synthetic protocol fixture, not a captured WorkCentre 3119 response.
fn response() -> Vec<u8> {
    let mut bytes = vec![0; 70];
    bytes[..4].copy_from_slice(&[0xa8, 0, 67, 0x10]);
    bytes[4..36].fill(b' ');
    bytes[4..26].copy_from_slice(b"XEROX WorkCentre 3119 ");
    bytes[36] = 1;
    bytes[37] = 0x22; // 150, 300 and 600 dpi, not SET_WINDOW codes.
    bytes[39] = 0x28; // Gray8 and RGB24.
    bytes[40..44].copy_from_slice(&10200u32.to_be_bytes());
    bytes[44..48].copy_from_slice(&14000u32.to_be_bytes());
    bytes[60..64].copy_from_slice(&14000u32.to_be_bytes());
    bytes
}

#[test]
fn decodes_reported_capabilities_without_assuming_defaults() {
    let parsed = Capabilities::parse(&response()).unwrap();
    assert_eq!(parsed.identity, "XEROX WorkCentre 3119");
    assert_eq!(parsed.resolutions(), vec![150, 300, 600]);
    assert_eq!(parsed.mode_mask, 0x28);
    assert_eq!(parsed.width_units, 10200);
    assert_eq!(parsed.flatbed_length_units, 14000);
    assert_eq!(parsed.line_order, 0);
}

#[test]
fn resolution_bits_include_extended_and_nonsequential_values() {
    let mut bytes = response();
    bytes[36] = 0x98; // 100, 1200, 2400
    bytes[37] = 0x11; // 75, 200
    bytes[55] = 0x0a; // 4800, 9600
    let parsed = Capabilities::parse(&bytes).unwrap();
    assert_eq!(
        parsed.resolutions(),
        vec![75, 100, 200, 1200, 2400, 4800, 9600]
    );
}

#[test]
fn rejects_every_truncation_and_bad_envelopes() {
    let bytes = response();
    for end in 0..bytes.len() {
        assert!(Capabilities::parse(&bytes[..end]).is_err(), "length {end}");
    }
    for (offset, value) in [(0, 0), (1, 2), (1, 4), (1, 8), (1, 255), (2, 66), (3, 0x20)] {
        let mut bytes = response();
        bytes[offset] = value;
        assert!(
            Capabilities::parse(&bytes).is_err(),
            "offset {offset}, value {value}"
        );
    }
    let mut bytes = response();
    bytes.push(0);
    assert!(Capabilities::parse(&bytes).is_err());
}

#[test]
fn preserves_unknown_flags_and_well_framed_extensions_for_diagnosis() {
    let mut bytes = response();
    bytes[37] |= 4;
    bytes[39] |= 0x80;
    bytes[49] = 2;
    bytes[50] = 0x40;
    bytes.push(0xff);
    bytes[2] += 1;
    let parsed = Capabilities::parse(&bytes).unwrap();
    assert_eq!(parsed.resolution_mask, 0x126);
    assert_eq!(parsed.mode_mask, 0xa8);
    assert_eq!(parsed.line_order, 2);
    assert_eq!(parsed.compression_mask, 0x40);
}

#[test]
fn rejects_empty_capabilities_and_unsafe_identity_text() {
    for range in [4..36, 36..38, 39..40, 40..44, 44..48, 60..64] {
        let mut bytes = response();
        bytes[range].fill(0);
        assert!(Capabilities::parse(&bytes).is_err());
    }
    for value in [0x1b, 0x0a, 0x7f, 0xff] {
        let mut bytes = response();
        bytes[10] = value;
        assert!(Capabilities::parse(&bytes).is_err());
    }
}

#[test]
fn accepts_nul_padding_and_keeps_unknown_only_resolution_explicit() {
    let mut bytes = response();
    bytes[9] = 0; // Vendor/model separator.
    bytes[26..36].fill(0);
    bytes[36] = 0;
    bytes[37] = 4; // An unknown bit must not become a guessed default DPI.
    let parsed = Capabilities::parse(&bytes).unwrap();
    assert_eq!(parsed.identity, "XEROX WorkCentre 3119");
    assert!(parsed.resolutions().is_empty());
    assert_eq!(parsed.resolution_mask, 4);
}

#[test]
fn arbitrary_wire_values_and_lengths_do_not_panic() {
    for offset in 0..70 {
        for value in 0..=255 {
            let mut bytes = response();
            bytes[offset] = value;
            let _ = Capabilities::parse(&bytes);
        }
    }
    for len in 0..=513 {
        let mut bytes = vec![0xa8; len];
        if len >= 3 {
            bytes[2] = len.wrapping_sub(3) as u8;
        }
        let _ = Capabilities::parse(&bytes);
    }
}
