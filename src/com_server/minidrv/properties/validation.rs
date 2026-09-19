//! Pure dependency resolution for WIA flatbed property validation.
//!
//! This module receives the value before the service write, the value after
//! the service write, and the numeric property IDs explicitly written by the
//! application.  It never calls WIA helpers, touches a service context, or
//! accesses hardware.  The native adapter publishes the returned settings
//! and validity descriptions after this calculation succeeds.

use super::{E_INVALIDARG, catalog::PropertyCatalog};
use crate::wia::FlatbedSettings;

/// Resolve the post-write flatbed settings without performing native writes.
///
/// `old` is the value observed before the WIA service applied the current
/// request. `current` is the value observed after that write. `written` holds
/// numeric `PROPID`s from the original application request.  Unknown IDs are
/// intentionally ignored here so the WIA service's catch-all validator can
/// handle properties outside this dependency graph.
/// Translate a written WIA_IPS_CUR_INTENT into the data type it implies.
/// `Ok(None)` means the intent carries no image-type request (size/quality
/// hints only). Contradictory or unsupported image types are rejected.
pub(super) fn data_type_for_intent(intent: i32) -> Result<Option<i32>, i32> {
    use super::catalog::{
        WIA_INTENT_IMAGE_TYPE_COLOR, WIA_INTENT_IMAGE_TYPE_GRAYSCALE, WIA_INTENT_MAXIMIZE_QUALITY,
        WIA_INTENT_MINIMIZE_SIZE,
    };
    let known = WIA_INTENT_IMAGE_TYPE_COLOR
        | WIA_INTENT_IMAGE_TYPE_GRAYSCALE
        | WIA_INTENT_MINIMIZE_SIZE
        | WIA_INTENT_MAXIMIZE_QUALITY;
    if intent & !known != 0 {
        return Err(E_INVALIDARG);
    }
    match (
        intent & WIA_INTENT_IMAGE_TYPE_COLOR != 0,
        intent & WIA_INTENT_IMAGE_TYPE_GRAYSCALE != 0,
    ) {
        (true, true) => Err(E_INVALIDARG),
        (true, false) => Ok(Some(3)),
        (false, true) => Ok(Some(2)),
        (false, false) => Ok(None),
    }
}

pub(super) fn resolve(
    catalog: &PropertyCatalog,
    old: FlatbedSettings,
    current: FlatbedSettings,
    written: &[u32],
) -> Result<FlatbedSettings, i32> {
    // The old value is the service state against which the request was made.
    // Rejecting an already-invalid baseline avoids treating an arbitrary
    // service state as a dependent value that this resolver may repair.
    catalog.with_settings(old)?;

    let mut resolved = current;
    resolve_mode_and_depth(&mut resolved, written)?;

    let (x_resolution, y_resolution) = resolve_resolutions(&resolved, written)?;
    resolved.x_resolution = x_resolution;
    resolved.y_resolution = y_resolution;

    let (max_width, max_height) = catalog.max_extent(x_resolution);
    if max_width <= 0 || max_height <= 0 {
        return Err(E_INVALIDARG);
    }

    let (x_position, x_extent) = resolve_axis(AxisInput {
        old_dpi: old.x_resolution,
        target_dpi: x_resolution,
        position: AxisValue {
            old: old.x_position,
            current: current.x_position,
            explicit: is_written(written, super::WIA_IPS_XPOS),
        },
        extent: AxisValue {
            old: old.x_extent,
            current: current.x_extent,
            explicit: is_written(written, super::WIA_IPS_XEXTENT),
        },
        max_extent: max_width,
    })?;
    let (y_position, y_extent) = resolve_axis(AxisInput {
        old_dpi: old.y_resolution,
        target_dpi: y_resolution,
        position: AxisValue {
            old: old.y_position,
            current: current.y_position,
            explicit: is_written(written, super::WIA_IPS_YPOS),
        },
        extent: AxisValue {
            old: old.y_extent,
            current: current.y_extent,
            explicit: is_written(written, super::WIA_IPS_YEXTENT),
        },
        max_extent: max_height,
    })?;
    resolved.x_position = x_position;
    resolved.x_extent = x_extent;
    resolved.y_position = y_position;
    resolved.y_extent = y_extent;

    // This is the final capability and core validation gate.  It also checks
    // non-dependent fields such as format, compression and tone controls.
    catalog.with_settings(resolved)?;
    Ok(resolved)
}

fn is_written(written: &[u32], propid: u32) -> bool {
    written.contains(&propid)
}

fn resolve_mode_and_depth(settings: &mut FlatbedSettings, written: &[u32]) -> Result<(), i32> {
    let datatype_written = is_written(written, super::WIA_IPA_DATATYPE);
    let depth_written = is_written(written, super::WIA_IPA_DEPTH);

    match (datatype_written, depth_written) {
        (true, true) => {
            // Both application values are authoritative.  An explicit
            // contradictory pair is rejected instead of silently replacing
            // one value with a driver preference.
            if depth_for(settings.data_type) != Some(settings.depth) {
                return Err(E_INVALIDARG);
            }
        }
        (true, false) => {
            settings.depth = depth_for(settings.data_type).ok_or(E_INVALIDARG)?;
        }
        (false, true) => {
            // Each advertised depth belongs to exactly one data type, so a
            // depth-only request selects that data type (WinRT clients write
            // WIA_IPA_DEPTH when switching colour modes).
            settings.data_type = match settings.depth {
                8 => 2,
                24 => 3,
                _ => return Err(E_INVALIDARG),
            };
        }
        (false, false) => {
            // If a service-side dependent update left the pair inconsistent,
            // the WIA priority order gives DATATYPE control over DEPTH.
            if depth_for(settings.data_type) != Some(settings.depth) {
                settings.depth = depth_for(settings.data_type).ok_or(E_INVALIDARG)?;
            }
        }
    }
    Ok(())
}

fn resolve_resolutions(settings: &FlatbedSettings, written: &[u32]) -> Result<(i32, i32), i32> {
    let x_written = is_written(written, super::WIA_IPS_XRES);
    let y_written = is_written(written, super::WIA_IPS_YRES);
    if x_written && y_written && settings.x_resolution != settings.y_resolution {
        // The core accepts one physical resolution for both axes.  Two
        // explicit conflicting inputs must remain an error.
        return Err(E_INVALIDARG);
    }

    // XRES precedes YRES in Microsoft's documented WIA conflict order, so an
    // explicit X (or the current X when neither is written) drives both axes.
    // Both lists advertise every resolution, so a y-only write to a listed
    // value must also be honoured: the other axis follows it.
    if x_written || !y_written {
        Ok((settings.x_resolution, settings.x_resolution))
    } else {
        Ok((settings.y_resolution, settings.y_resolution))
    }
}

#[derive(Clone, Copy)]
struct AxisValue {
    old: i32,
    current: i32,
    explicit: bool,
}

#[derive(Clone, Copy)]
struct AxisInput {
    old_dpi: i32,
    target_dpi: i32,
    position: AxisValue,
    extent: AxisValue,
    max_extent: i32,
}

fn resolve_axis(input: AxisInput) -> Result<(i32, i32), i32> {
    if input.max_extent <= 0 {
        return Err(E_INVALIDARG);
    }
    let resolution_changed = input.old_dpi != input.target_dpi;

    let mut position = if input.position.explicit {
        input.position.current
    } else if resolution_changed && input.position.current == input.position.old {
        quantize_scaled_position(input.position.old, input.old_dpi, input.target_dpi)?
    } else {
        // With no resolution change, an unwritten position is already a
        // service value.  Preserve it and let the final catalog gate reject
        // an invalid pre-existing value rather than changing it on a no-op.
        input.position.current
    };
    let mut extent = if input.extent.explicit {
        input.extent.current
    } else if resolution_changed && input.extent.current == input.extent.old {
        scale_extent(input.extent.old, input.old_dpi, input.target_dpi)?
    } else {
        input.extent.current
    };

    if input.position.explicit {
        if position < 0 || position >= input.max_extent {
            return Err(E_INVALIDARG);
        }
        // Windows clients derive positions from an inch-based region and
        // round to whole pixels, so explicit values regularly miss the
        // 1/100-inch hardware step (Windows Scan wrote XPOS=7 at 600 dpi).
        // Snap to the nearest step; the shift is below one step and far
        // smaller than the client's own rounding. An explicit extent must
        // still fit, so a snap upwards falls back to the step below.
        let step = super::catalog::offset_step(input.target_dpi);
        let requested = position;
        position = quantize_position(requested, input.target_dpi)?;
        let limit = if input.extent.explicit && extent > 0 {
            input.max_extent.checked_sub(extent).ok_or(E_INVALIDARG)?
        } else {
            input.max_extent - 1
        };
        if position > limit {
            // Rounding down never moves further than rounding up did; the
            // extent check below still rejects a request that cannot fit.
            position = floor_to_step(requested, step);
        }
    } else if position < 0 {
        return Err(E_INVALIDARG);
    }
    if input.extent.explicit {
        if extent <= 0 || extent > input.max_extent {
            return Err(E_INVALIDARG);
        }
    } else if extent < 0 {
        return Err(E_INVALIDARG);
    } else if extent == 0 {
        // A non-explicit zero extent has no usable scan rectangle.  The
        // smallest positive WIA extent is a safe dependent repair.
        extent = 1;
    }

    if !input.position.explicit && !input.extent.explicit && position >= input.max_extent {
        position = floor_to_step(
            input.max_extent - 1,
            super::catalog::offset_step(input.target_dpi),
        );
    }

    if !input.position.explicit && input.extent.explicit {
        // Keep the explicit extent intact and move only the dependent start
        // position down to the largest representable hardware step.
        let limit = input.max_extent.checked_sub(extent).ok_or(E_INVALIDARG)?;
        position = position.min(floor_to_step(
            limit,
            super::catalog::offset_step(input.target_dpi),
        ));
    }

    let available = input.max_extent.checked_sub(position).ok_or(E_INVALIDARG)?;
    if available <= 0 {
        return Err(E_INVALIDARG);
    }
    if extent > available {
        if input.extent.explicit {
            return Err(E_INVALIDARG);
        }
        extent = available;
    }
    if extent <= 0 {
        return Err(E_INVALIDARG);
    }

    // Explicit positions are snapped above; dependent positions are quantized
    // when scaled. The final catalog/core gate re-checks the 1/100-inch
    // alignment and the extents.
    Ok((position, extent))
}

fn depth_for(data_type: i32) -> Option<i32> {
    match data_type {
        2 => Some(8),
        3 => Some(24),
        _ => None,
    }
}

fn scale_extent(value: i32, old_dpi: i32, target_dpi: i32) -> Result<i32, i32> {
    if value < 0 || old_dpi <= 0 || target_dpi <= 0 {
        return Err(E_INVALIDARG);
    }
    let old_dpi = u64::try_from(old_dpi).map_err(|_| E_INVALIDARG)?;
    let target_dpi = u64::try_from(target_dpi).map_err(|_| E_INVALIDARG)?;
    let numerator = u64::try_from(value)
        .map_err(|_| E_INVALIDARG)?
        .checked_mul(target_dpi)
        .ok_or(E_INVALIDARG)?;
    let rounded = numerator.checked_add(old_dpi / 2).ok_or(E_INVALIDARG)? / old_dpi;
    i32::try_from(rounded).map_err(|_| E_INVALIDARG)
}

fn quantize_scaled_position(value: i32, old_dpi: i32, target_dpi: i32) -> Result<i32, i32> {
    let scaled = scale_extent(value, old_dpi, target_dpi)?;
    quantize_position(scaled, target_dpi)
}

fn quantize_position(value: i32, dpi: i32) -> Result<i32, i32> {
    if value < 0 || dpi <= 0 {
        return Err(E_INVALIDARG);
    }
    let step = super::catalog::offset_step(dpi);
    if step <= 0 {
        return Err(E_INVALIDARG);
    }
    let quotient = value / step;
    let remainder = value % step;
    let rounded = if remainder.saturating_mul(2) >= step {
        quotient.checked_add(1).ok_or(E_INVALIDARG)?
    } else {
        quotient
    };
    rounded.checked_mul(step).ok_or(E_INVALIDARG)
}

fn floor_to_step(value: i32, step: i32) -> i32 {
    if value <= 0 { 0 } else { value / step * step }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intent_maps_to_a_single_data_type_or_nothing() {
        assert_eq!(data_type_for_intent(0), Ok(None));
        assert_eq!(data_type_for_intent(0x1), Ok(Some(3)));
        assert_eq!(data_type_for_intent(0x2 | 0x10000), Ok(Some(2)));
        assert_eq!(data_type_for_intent(0x20000), Ok(None));
        assert_eq!(data_type_for_intent(0x3), Err(E_INVALIDARG));
        assert_eq!(
            data_type_for_intent(0x4),
            Err(E_INVALIDARG),
            "text is not offered"
        );
        assert_eq!(data_type_for_intent(0x8), Err(E_INVALIDARG));
    }
    use crate::{protocol::Capabilities, wia::BMP_FORMAT};

    const WIA_IPA_DATATYPE: u32 = 4103;
    const WIA_IPA_DEPTH: u32 = 4104;
    const WIA_IPS_XRES: u32 = 6147;
    const WIA_IPS_YRES: u32 = 6148;
    const WIA_IPS_XPOS: u32 = 6149;
    const WIA_IPS_YPOS: u32 = 6150;
    const WIA_IPS_XEXTENT: u32 = 6151;
    const WIA_IPS_YEXTENT: u32 = 6152;

    fn catalog() -> PropertyCatalog {
        let capabilities = Capabilities {
            identity: "synthetic".to_owned(),
            resolution_mask: (1 << 0) | (1 << 5) | (1 << 8),
            mode_mask: (1 << 3) | (1 << 5),
            width_units: 10_200,
            length_units: 14_040,
            flatbed_length_units: 14_040,
            line_order: 0,
            compression_mask: 1,
        };
        PropertyCatalog::flatbed(&capabilities, "Flatbed", "Root\\Flatbed").unwrap()
    }

    fn settings() -> FlatbedSettings {
        FlatbedSettings {
            x_resolution: 300,
            y_resolution: 300,
            x_position: 3,
            y_position: 0,
            x_extent: 600,
            y_extent: 900,
            data_type: 2,
            depth: 8,
            brightness: 0,
            contrast: 0,
            compression: 0,
            format: BMP_FORMAT,
        }
    }

    #[test]
    fn dpi_only_change_scales_unwritten_rectangle_and_quantizes_position() {
        let old = settings();
        let current = FlatbedSettings {
            x_resolution: 75,
            ..old
        };
        let resolved = resolve(&catalog(), old, current, &[WIA_IPS_XRES]).unwrap();

        assert_eq!(resolved.x_resolution, 75);
        assert_eq!(resolved.y_resolution, 75);
        assert_eq!(resolved.x_position, 0);
        assert_eq!(resolved.y_position, 0);
        assert_eq!(resolved.x_extent, 150);
        assert_eq!(resolved.y_extent, 225);
    }

    #[test]
    fn no_op_preserves_all_current_settings() {
        let old = settings();
        let resolved = resolve(&catalog(), old, old, &[]).unwrap();

        assert_eq!(resolved.x_resolution, old.x_resolution);
        assert_eq!(resolved.y_resolution, old.y_resolution);
        assert_eq!(resolved.x_position, old.x_position);
        assert_eq!(resolved.y_position, old.y_position);
        assert_eq!(resolved.x_extent, old.x_extent);
        assert_eq!(resolved.y_extent, old.y_extent);
        assert_eq!(resolved.data_type, old.data_type);
        assert_eq!(resolved.depth, old.depth);
        assert_eq!(resolved.brightness, old.brightness);
        assert_eq!(resolved.contrast, old.contrast);
        assert_eq!(resolved.compression, old.compression);
        assert_eq!(resolved.format, old.format);
    }

    #[test]
    fn explicit_position_snaps_to_the_hardware_step() {
        // Windows clients compute XPOS/YPOS from an inch-based region and
        // round to whole pixels, so they land off the 1/100-inch step (the
        // Windows Scan app wrote XPOS=7 at 600 dpi, step 6, 2026-09-19).
        // Snapping moves the origin by less than one step instead of failing
        // the whole scan.
        let old = settings();
        let current = FlatbedSettings {
            x_resolution: 75,
            x_position: 1,
            ..old
        };
        let resolved = resolve(&catalog(), old, current, &[WIA_IPS_XRES, WIA_IPS_XPOS]).unwrap();
        assert_eq!(resolved.x_position, 0);

        let current = FlatbedSettings {
            x_resolution: 600,
            y_resolution: 600,
            x_position: 7,
            y_position: 0,
            x_extent: 3729,
            y_extent: 4015,
            data_type: 3,
            depth: 24,
            ..old
        };
        let written = [
            WIA_IPA_DATATYPE,
            WIA_IPA_DEPTH,
            WIA_IPS_XPOS,
            WIA_IPS_YPOS,
            WIA_IPS_XEXTENT,
            WIA_IPS_YEXTENT,
        ];
        let old600 = FlatbedSettings {
            x_resolution: 600,
            y_resolution: 600,
            x_position: 0,
            ..old
        };
        let resolved = resolve(&catalog(), old600, current, &written).unwrap();
        assert_eq!((resolved.x_position, resolved.x_extent), (6, 3729));
        assert_eq!((resolved.y_position, resolved.y_extent), (0, 4015));
    }

    #[test]
    fn small_region_after_a_full_bed_scan_is_accepted() {
        // WinRT region 0.0117 x 0.0233 in, 1.5 x 1.0 in at 75 dpi, written
        // right after a full-bed scan (2026-09-19 combination run).
        let old = FlatbedSettings {
            x_resolution: 75,
            y_resolution: 75,
            x_position: 0,
            y_position: 0,
            x_extent: 637,
            y_extent: 877,
            data_type: 3,
            depth: 24,
            ..settings()
        };
        let current = FlatbedSettings {
            y_position: 1,
            x_extent: 112,
            y_extent: 75,
            ..old
        };
        let written = [
            WIA_IPA_DATATYPE,
            WIA_IPA_DEPTH,
            WIA_IPS_XPOS,
            WIA_IPS_YPOS,
            WIA_IPS_XEXTENT,
            WIA_IPS_YEXTENT,
        ];
        let resolved = resolve(&catalog(), old, current, &written).unwrap();
        assert_eq!((resolved.y_position, resolved.y_extent), (0, 75));
        assert_eq!((resolved.x_position, resolved.x_extent), (0, 112));
    }

    #[test]
    fn snapped_explicit_position_keeps_the_explicit_extent_inside_the_bed() {
        // 10 200 units = 8.5 in = 637 px at 75 dpi. Position 635 with extent
        // 2 would snap up to 636 (step 3) and overrun the bed, so it snaps
        // down to 633 instead.
        let old = FlatbedSettings {
            x_resolution: 75,
            y_resolution: 75,
            x_extent: 150,
            y_extent: 225,
            ..settings()
        };
        let current = FlatbedSettings {
            x_position: 635,
            x_extent: 2,
            ..old
        };
        let resolved = resolve(&catalog(), old, current, &[WIA_IPS_XPOS, WIA_IPS_XEXTENT]).unwrap();
        assert_eq!((resolved.x_position, resolved.x_extent), (633, 2));

        // An explicit extent that does not fit after snapping is still an error.
        let current = FlatbedSettings {
            x_position: 635,
            x_extent: 10,
            ..old
        };
        assert!(matches!(
            resolve(&catalog(), old, current, &[WIA_IPS_XPOS, WIA_IPS_XEXTENT]),
            Err(E_INVALIDARG)
        ));
    }

    #[test]
    fn datatype_only_change_updates_depth() {
        let old = settings();
        let current = FlatbedSettings {
            data_type: 3,
            ..old
        };

        let resolved = resolve(&catalog(), old, current, &[WIA_IPA_DATATYPE]).unwrap();
        assert_eq!(resolved.data_type, 3);
        assert_eq!(resolved.depth, 24);
    }

    #[test]
    fn depth_only_write_selects_the_matching_datatype_and_rejects_unknown_depths() {
        // Each advertised depth belongs to one data type, so WinRT's depth-only
        // colour switch is honoured instead of rejected.
        let old = settings();
        let current = FlatbedSettings { depth: 24, ..old };
        let resolved = resolve(&catalog(), old, current, &[WIA_IPA_DEPTH]).unwrap();
        assert_eq!((resolved.data_type, resolved.depth), (3, 24));

        let current = FlatbedSettings { depth: 16, ..old };
        assert!(matches!(
            resolve(&catalog(), old, current, &[WIA_IPA_DEPTH]),
            Err(E_INVALIDARG)
        ));
    }

    #[test]
    fn y_resolution_only_write_moves_both_axes_like_an_x_write() {
        // Every advertised WIA_IPS_YRES value must be writable on its own; the
        // hardware has one resolution, so the X axis follows.
        let old = settings();
        let current = FlatbedSettings {
            y_resolution: 75,
            ..old
        };

        let resolved = resolve(&catalog(), old, current, &[WIA_IPS_YRES]).unwrap();
        assert_eq!((resolved.x_resolution, resolved.y_resolution), (75, 75));
        assert_eq!((resolved.x_extent, resolved.y_extent), (150, 225));
    }

    #[test]
    fn conflicting_explicit_x_and_y_resolutions_are_rejected() {
        let old = settings();
        let current = FlatbedSettings {
            x_resolution: 75,
            y_resolution: 300,
            ..old
        };

        assert!(matches!(
            resolve(&catalog(), old, current, &[WIA_IPS_XRES, WIA_IPS_YRES]),
            Err(E_INVALIDARG)
        ));
    }

    #[test]
    fn simultaneous_explicit_rectangle_is_preserved() {
        let old = settings();
        let current = FlatbedSettings {
            x_position: 300,
            y_position: 600,
            x_extent: 900,
            y_extent: 1200,
            ..old
        };
        let written = [WIA_IPS_XPOS, WIA_IPS_YPOS, WIA_IPS_XEXTENT, WIA_IPS_YEXTENT];

        let resolved = resolve(&catalog(), old, current, &written).unwrap();
        assert_eq!(resolved.x_resolution, current.x_resolution);
        assert_eq!(resolved.y_resolution, current.y_resolution);
        assert_eq!(resolved.x_position, current.x_position);
        assert_eq!(resolved.y_position, current.y_position);
        assert_eq!(resolved.x_extent, current.x_extent);
        assert_eq!(resolved.y_extent, current.y_extent);
        assert_eq!(resolved.data_type, current.data_type);
        assert_eq!(resolved.depth, current.depth);
    }

    #[test]
    fn explicit_position_shrinks_only_the_unwritten_extent() {
        let old = settings();
        let current = FlatbedSettings {
            x_position: 2400,
            ..old
        };

        let resolved = resolve(&catalog(), old, current, &[WIA_IPS_XPOS]).unwrap();
        assert_eq!(resolved.x_position, 2400);
        assert_eq!(resolved.x_extent, 150);
        assert_eq!(resolved.y_position, old.y_position);
        assert_eq!(resolved.y_extent, old.y_extent);
    }

    #[test]
    fn explicit_position_and_extent_out_of_bounds_are_rejected() {
        let old = settings();
        let current = FlatbedSettings {
            x_position: 2400,
            x_extent: 600,
            ..old
        };
        let written = [WIA_IPS_XPOS, WIA_IPS_XEXTENT];

        assert!(matches!(
            resolve(&catalog(), old, current, &written),
            Err(E_INVALIDARG)
        ));
    }

    #[test]
    fn future_datatype_is_rejected_by_live_catalog() {
        let old = settings();
        let current = FlatbedSettings {
            data_type: 7,
            ..old
        };

        assert!(matches!(
            resolve(&catalog(), old, current, &[WIA_IPA_DATATYPE]),
            Err(E_INVALIDARG)
        ));
    }

    #[test]
    fn both_explicit_resolution_values_must_match_core() {
        let old = settings();
        let current = FlatbedSettings {
            x_resolution: 75,
            y_resolution: 300,
            ..old
        };

        assert!(matches!(
            resolve(&catalog(), old, current, &[WIA_IPS_XRES, WIA_IPS_YRES]),
            Err(E_INVALIDARG)
        ));
    }
}
