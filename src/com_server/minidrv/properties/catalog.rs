//! Pure WIA property catalog derived from a validated protocol capability frame.
//!
//! The catalog owns only driver-side values.  The WIA service context remains
//! opaque and is handled by `native.rs` when the catalog is published.

use crate::{com_server::Guid, protocol::Capabilities, wia::BMP_FORMAT};

const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;

const VT_I4: u16 = 3;
const VT_BSTR: u16 = 8;
const VT_CLSID: u16 = 72;

const WIA_PROP_READ: u32 = 0x01;
const WIA_PROP_WRITE: u32 = 0x02;
const WIA_PROP_RW: u32 = WIA_PROP_READ | WIA_PROP_WRITE;
const WIA_PROP_NONE: u32 = 0x08;
const WIA_PROP_RANGE: u32 = 0x10;
const WIA_PROP_LIST: u32 = 0x20;
const WIA_PROP_FLAG: u32 = 0x40;
const WIA_PROP_CACHEABLE: u32 = 0x1_0000;

const WIA_ITEM_READ: i32 = 0x01;

const WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES: u32 = 3086;
const WIA_DPS_DOCUMENT_HANDLING_STATUS: u32 = 3087;
// Read by the WIA common UI (Windows Fax and Scan) and the WinRT scan runtime
// even though Microsoft marks the DPS forms obsolete; both came back VT_EMPTY
// in the 2026-09-19 traces before the UIs gave up. Same numeric IDs as the
// WIA_IPS_* item forms in wiadef.h.
const WIA_DPS_DOCUMENT_HANDLING_SELECT: u32 = 3088;
const WIA_SHOW_PREVIEW_CONTROL_ID: u32 = 3103;
const WIA_IPS_SEGMENTATION: u32 = 6164;
// Windows Fax and Scan writes WIA_IPS_ROTATION (PORTRAIT) before every scan
// and aborts with E_INVALIDARG when the item does not have it (2026-09-19).
const WIA_IPS_ROTATION: u32 = 6157;
const WIA_ROTATION_PORTRAIT: i32 = 0;
const DOCUMENT_HANDLING_FLATBED: i32 = 0x0002;
const WIA_DONT_SHOW_PREVIEW_CONTROL: i32 = 1;
const WIA_DONT_USE_SEGMENTATION_FILTER: i32 = 1;
const WIA_IPA_ITEM_NAME: u32 = 4098;
const WIA_IPA_FULL_ITEM_NAME: u32 = 4099;
const WIA_IPA_ACCESS_RIGHTS: u32 = 4102;
const WIA_IPA_DATATYPE: u32 = 4103;
const WIA_IPA_DEPTH: u32 = 4104;
const WIA_IPA_PREFERRED_FORMAT: u32 = 4105;
const WIA_IPA_FORMAT: u32 = 4106;
const WIA_IPA_COMPRESSION: u32 = 4107;
const WIA_IPA_TYMED: u32 = 4108;
const WIA_IPA_CHANNELS_PER_PIXEL: u32 = 4109;
const WIA_IPA_BITS_PER_CHANNEL: u32 = 4110;
const WIA_IPA_PLANAR: u32 = 4111;
const WIA_IPA_PIXELS_PER_LINE: u32 = 4112;
const WIA_IPA_NUMBER_OF_LINES: u32 = 4114;
const WIA_IPA_ITEM_SIZE: u32 = 4116;
const WIA_IPA_COLOR_PROFILE: u32 = 4117;
const WIA_IPA_BUFFER_SIZE: u32 = 4118;
const WIA_IPA_PROP_STREAM_COMPAT_ID: u32 = 4122;
const WIA_IPA_ITEM_CATEGORY: u32 = 4125;
const WIA_IPS_CUR_INTENT: u32 = 6146;
const WIA_IPS_XRES: u32 = 6147;
const WIA_IPS_YRES: u32 = 6148;
const WIA_IPS_XPOS: u32 = 6149;
const WIA_IPS_YPOS: u32 = 6150;
const WIA_IPS_XEXTENT: u32 = 6151;
const WIA_IPS_YEXTENT: u32 = 6152;
const WIA_IPS_BRIGHTNESS: u32 = 6154;
const WIA_IPS_CONTRAST: u32 = 6155;
const WIA_IPS_MAX_HORIZONTAL_SIZE: u32 = 6165;
const WIA_IPS_MAX_VERTICAL_SIZE: u32 = 6166;
const WIA_IPS_MIN_HORIZONTAL_SIZE: u32 = 6167;
const WIA_IPS_MIN_VERTICAL_SIZE: u32 = 6168;
const WIA_IPS_OPTICAL_XRES: u32 = 3090;
const WIA_IPS_OPTICAL_YRES: u32 = 3091;
const WIA_IPS_PREVIEW: u32 = 3100;

const WIA_DATA_GRAYSCALE: i32 = 2;
const WIA_DATA_COLOR: i32 = 3;
// SDK wiadef.h WIA_IPS_CUR_INTENT flags. The two size/quality hints are
// accepted without changing settings; TEXT (1 bpp) is not offered.
pub(super) const WIA_INTENT_IMAGE_TYPE_COLOR: i32 = 0x0000_0001;
pub(super) const WIA_INTENT_IMAGE_TYPE_GRAYSCALE: i32 = 0x0000_0002;
pub(super) const WIA_INTENT_MINIMIZE_SIZE: i32 = 0x0001_0000;
pub(super) const WIA_INTENT_MAXIMIZE_QUALITY: i32 = 0x0002_0000;
const WIA_COMPRESSION_NONE: i32 = 0;
const TYMED_FILE: i32 = 2;
const WIA_PACKED_PIXEL: i32 = 0;
const WIA_FINAL_SCAN: i32 = 0;
const WIA_CATEGORY_ROOT: Guid = Guid {
    data1: 0xf193_526f,
    data2: 0x59b8,
    data3: 0x4a26,
    data4: [0x98, 0x88, 0xe1, 0x6e, 0x4f, 0x97, 0xce, 0x10],
};
const WIA_CATEGORY_FLATBED: Guid = Guid {
    data1: 0xfb60_7b1f,
    data2: 0x43f3,
    data3: 0x488b,
    data4: [0x85, 0x5b, 0xfb, 0x70, 0x3e, 0xc3, 0x42, 0xa6],
};

/// A scalar initial value written by `wiasWriteMultiple`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PropertyValue {
    Long(i32),
    Guid(Guid),
    String(String),
}

/// The effective WIA validity description for one property.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum PropertyAttribute {
    None {
        access: u32,
        vt: u16,
    },
    RangeLong {
        access: u32,
        min: i32,
        nominal: i32,
        max: i32,
        step: i32,
    },
    ListLong {
        access: u32,
        values: Vec<i32>,
        nominal: i32,
    },
    ListGuid {
        access: u32,
        values: Vec<Guid>,
        nominal: Guid,
    },
    FlagLong {
        access: u32,
        nominal: i32,
        valid_bits: i32,
    },
}

impl PropertyAttribute {
    pub(super) fn access(&self) -> u32 {
        match self {
            Self::None { access, .. }
            | Self::RangeLong { access, .. }
            | Self::ListLong { access, .. }
            | Self::ListGuid { access, .. }
            | Self::FlagLong { access, .. } => *access,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemKind {
    Root,
    Flatbed,
}

/// A complete, aligned name/value/attribute set for one WIA item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PropertyCatalog {
    kind: ItemKind,
    ids: Vec<u32>,
    names: Vec<String>,
    initial_values: Vec<PropertyValue>,
    attributes: Vec<PropertyAttribute>,
    resolutions: Vec<i32>,
    data_types: Vec<i32>,
    default_resolution: i32,
    width_units: u32,
    height_units: u32,
}

impl PropertyCatalog {
    /// Build root category, access rights and document handling properties.
    ///
    /// `status` must come from the caller's current device-status policy.  The
    /// protocol INQUIRY frame has no WIA cover/status field, so this builder
    /// does not invent one.
    pub(super) fn root(caps: &Capabilities, status: i32) -> Result<Self, i32> {
        validate_capabilities(caps)?;
        let mut catalog = Self {
            kind: ItemKind::Root,
            ids: Vec::new(),
            names: Vec::new(),
            initial_values: Vec::new(),
            attributes: Vec::new(),
            resolutions: Vec::new(),
            data_types: Vec::new(),
            default_resolution: 0,
            width_units: 0,
            height_units: 0,
        };
        catalog.push(
            WIA_IPA_ITEM_CATEGORY,
            "Item Category",
            PropertyValue::Guid(WIA_CATEGORY_ROOT),
            PropertyAttribute::None {
                access: WIA_PROP_READ | WIA_PROP_NONE | WIA_PROP_CACHEABLE,
                vt: VT_CLSID,
            },
        );
        catalog.push(
            WIA_IPA_ACCESS_RIGHTS,
            "Access Rights",
            PropertyValue::Long(WIA_ITEM_READ),
            PropertyAttribute::FlagLong {
                access: WIA_PROP_READ | WIA_PROP_FLAG,
                nominal: WIA_ITEM_READ,
                valid_bits: WIA_ITEM_READ,
            },
        );
        catalog.push(
            WIA_DPS_DOCUMENT_HANDLING_CAPABILITIES,
            "Document Handling Capabilities",
            PropertyValue::Long(0x02), // FLAT
            PropertyAttribute::None {
                access: WIA_PROP_READ | WIA_PROP_NONE | WIA_PROP_CACHEABLE,
                vt: VT_I4,
            },
        );
        catalog.push(
            WIA_DPS_DOCUMENT_HANDLING_STATUS,
            "Document Handling Status",
            PropertyValue::Long(status),
            PropertyAttribute::None {
                access: WIA_PROP_READ | WIA_PROP_NONE,
                vt: VT_I4,
            },
        );
        // Flatbed only: the legacy selector is fixed and read-only.
        catalog.push(
            WIA_DPS_DOCUMENT_HANDLING_SELECT,
            "Document Handling Select",
            PropertyValue::Long(DOCUMENT_HANDLING_FLATBED),
            PropertyAttribute::FlagLong {
                access: WIA_PROP_READ | WIA_PROP_FLAG,
                nominal: DOCUMENT_HANDLING_FLATBED,
                valid_bits: DOCUMENT_HANDLING_FLATBED,
            },
        );
        // No preview scan mode is offered (WIA_IPS_PREVIEW lists FINAL only).
        catalog.push(
            WIA_SHOW_PREVIEW_CONTROL_ID,
            "Show preview control",
            PropertyValue::Long(WIA_DONT_SHOW_PREVIEW_CONTROL),
            PropertyAttribute::None {
                access: WIA_PROP_READ | WIA_PROP_NONE,
                vt: VT_I4,
            },
        );
        Ok(catalog)
    }

    /// Build the required flatbed item properties and their initial effective
    /// ranges.  Every resolution and geometry limit is derived from `caps`.
    pub(super) fn flatbed(
        caps: &Capabilities,
        item_name: &str,
        full_item_name: &str,
    ) -> Result<Self, i32> {
        validate_capabilities(caps)?;
        validate_name(item_name)?;
        validate_name(full_item_name)?;

        let resolutions: Vec<i32> = caps
            .resolutions()
            .into_iter()
            .filter(|dpi| matches!(*dpi, 75 | 100 | 150 | 200 | 300 | 600))
            .map(|dpi| dpi as i32)
            .collect();
        if resolutions.is_empty() {
            return Err(E_INVALIDARG);
        }

        // The scan core currently accepts only the two mode command codes
        // below.  The bit positions are protocol capability bits, not WIA
        // datatype values.
        let mut data_types = Vec::new();
        if caps.mode_mask & (1 << 3) != 0 {
            data_types.push(WIA_DATA_GRAYSCALE);
        }
        if caps.mode_mask & (1 << 5) != 0 && caps.line_order <= 1 {
            data_types.push(WIA_DATA_COLOR);
        }
        if data_types.is_empty() || caps.compression_mask & 1 == 0 {
            return Err(E_INVALIDARG);
        }

        let default_resolution = resolutions[0];
        let default_data_type = data_types[0];
        let default_depth = depth_for(default_data_type);
        let default_channels = channels_for(default_data_type);
        let max_horizontal_size = size_thousandths(caps.width_units)?;
        let max_vertical_units = caps.flatbed_length_units.min(caps.length_units);
        let max_vertical_size = size_thousandths(max_vertical_units)?;
        let min_size = min_size_thousandths(default_resolution)?;
        let mut catalog = Self {
            kind: ItemKind::Flatbed,
            ids: Vec::new(),
            names: Vec::new(),
            initial_values: Vec::new(),
            attributes: Vec::new(),
            resolutions,
            data_types,
            default_resolution,
            width_units: caps.width_units,
            height_units: max_vertical_units,
        };
        let (max_width, max_height) = catalog.max_extent(catalog.default_resolution);
        if max_width <= 0 || max_height <= 0 {
            return Err(E_INVALIDARG);
        }
        let initial_size = bmp_size(max_width, max_height, default_depth)?;

        let rw_list = |values: Vec<i32>, nominal| PropertyAttribute::ListLong {
            access: WIA_PROP_RW | WIA_PROP_LIST,
            values,
            nominal,
        };
        let ro_none = |vt| PropertyAttribute::None {
            // Read-only values can still change with the selected resolution
            // and datatype. Do not let the service cache them across changes.
            access: WIA_PROP_READ | WIA_PROP_NONE,
            vt,
        };
        let rw_range = |min, nominal, max| PropertyAttribute::RangeLong {
            access: WIA_PROP_RW | WIA_PROP_RANGE,
            min,
            nominal,
            max,
            step: 1,
        };
        let add_long = |catalog: &mut Self, id, name, value, attribute: PropertyAttribute| {
            catalog.push(id, name, PropertyValue::Long(value), attribute);
        };

        add_long(
            &mut catalog,
            WIA_IPA_ACCESS_RIGHTS,
            "Access Rights",
            WIA_ITEM_READ,
            PropertyAttribute::FlagLong {
                access: WIA_PROP_READ | WIA_PROP_FLAG,
                nominal: WIA_ITEM_READ,
                valid_bits: WIA_ITEM_READ,
            },
        );
        add_long(
            &mut catalog,
            WIA_IPA_BITS_PER_CHANNEL,
            "Bits Per Channel",
            8,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPA_BUFFER_SIZE,
            "Buffer Size",
            64 * 1024,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPA_CHANNELS_PER_PIXEL,
            "Channels Per Pixel",
            default_channels,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPA_COLOR_PROFILE,
            "Color Profiles",
            0,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPA_COMPRESSION,
            "Compression",
            WIA_COMPRESSION_NONE,
            rw_list(vec![WIA_COMPRESSION_NONE], WIA_COMPRESSION_NONE),
        );
        let data_type_values = catalog.data_types.clone();
        // Every depth reachable through the advertised data types stays in the
        // valid list; WinRT Windows.Devices.Scanners decides colour support from
        // it (2026-09-19: a single-entry list made it report grayscale only).
        // Y resolution still tracks the X resolution list.
        let depth_values = catalog.all_depths();
        add_long(
            &mut catalog,
            WIA_IPA_DATATYPE,
            "Data Type",
            default_data_type,
            rw_list(data_type_values, default_data_type),
        );
        add_long(
            &mut catalog,
            WIA_IPA_DEPTH,
            "Bits Per Pixel",
            default_depth,
            rw_list(depth_values, default_depth),
        );
        catalog.push(
            WIA_IPA_FORMAT,
            "Format",
            PropertyValue::Guid(guid_from_bytes(BMP_FORMAT)),
            PropertyAttribute::ListGuid {
                access: WIA_PROP_RW | WIA_PROP_LIST,
                values: vec![guid_from_bytes(BMP_FORMAT)],
                nominal: guid_from_bytes(BMP_FORMAT),
            },
        );
        catalog.push(
            WIA_IPA_FULL_ITEM_NAME,
            "Full Item Name",
            PropertyValue::String(full_item_name.to_owned()),
            ro_none(VT_BSTR),
        );
        // WIA_IPA_ICM_PROFILE_NAME is owned by the WIA service from the INF
        // ICMProfiles entry. Never overwrite its installed value with a
        // fabricated profile or an empty string.
        catalog.push(
            WIA_IPA_ITEM_CATEGORY,
            "Item Category",
            PropertyValue::Guid(WIA_CATEGORY_FLATBED),
            ro_none(VT_CLSID),
        );
        catalog.push(
            WIA_IPA_ITEM_NAME,
            "Item Name",
            PropertyValue::String(item_name.to_owned()),
            ro_none(VT_BSTR),
        );
        add_long(
            &mut catalog,
            WIA_IPA_ITEM_SIZE,
            "Item Size",
            initial_size,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPS_MAX_HORIZONTAL_SIZE,
            "Maximum Horizontal Scan Size",
            max_horizontal_size,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPS_MAX_VERTICAL_SIZE,
            "Maximum Vertical Scan Size",
            max_vertical_size,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPS_MIN_HORIZONTAL_SIZE,
            "Minimum Horizontal Scan Size",
            min_size,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPS_MIN_VERTICAL_SIZE,
            "Minimum Vertical Scan Size",
            min_size,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPA_NUMBER_OF_LINES,
            "Number of Lines",
            max_height,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPA_PIXELS_PER_LINE,
            "Pixels Per Line",
            max_width,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPA_PLANAR,
            "Planar",
            WIA_PACKED_PIXEL,
            rw_list(vec![WIA_PACKED_PIXEL], WIA_PACKED_PIXEL),
        );
        catalog.push(
            WIA_IPA_PREFERRED_FORMAT,
            "Preferred Format",
            PropertyValue::Guid(guid_from_bytes(BMP_FORMAT)),
            ro_none(VT_CLSID),
        );
        catalog.push(
            WIA_IPA_PROP_STREAM_COMPAT_ID,
            "Stream Compatibility ID",
            PropertyValue::Guid(guid_from_bytes(BMP_FORMAT)),
            PropertyAttribute::ListGuid {
                access: WIA_PROP_READ | WIA_PROP_LIST | WIA_PROP_CACHEABLE,
                values: vec![guid_from_bytes(BMP_FORMAT)],
                nominal: guid_from_bytes(BMP_FORMAT),
            },
        );
        add_long(
            &mut catalog,
            WIA_IPA_TYMED,
            "Media Type",
            TYMED_FILE,
            rw_list(vec![TYMED_FILE], TYMED_FILE),
        );
        add_long(
            &mut catalog,
            WIA_IPS_BRIGHTNESS,
            "Brightness",
            0,
            rw_range(-1000, 0, 1000),
        );
        add_long(
            &mut catalog,
            WIA_IPS_CONTRAST,
            "Contrast",
            0,
            rw_range(-1000, 0, 1000),
        );
        // WinRT Windows.Devices.Scanners derives colour support from these
        // valid flags (2026-09-19: with 0 it reported grayscale only although
        // WIA_IPA_DATATYPE already listed colour).
        let mut intent_bits = WIA_INTENT_MINIMIZE_SIZE | WIA_INTENT_MAXIMIZE_QUALITY;
        if catalog.data_types.contains(&WIA_DATA_COLOR) {
            intent_bits |= WIA_INTENT_IMAGE_TYPE_COLOR;
        }
        if catalog.data_types.contains(&WIA_DATA_GRAYSCALE) {
            intent_bits |= WIA_INTENT_IMAGE_TYPE_GRAYSCALE;
        }
        add_long(
            &mut catalog,
            WIA_IPS_CUR_INTENT,
            "Current Intent",
            0,
            PropertyAttribute::FlagLong {
                access: WIA_PROP_RW | WIA_PROP_FLAG,
                nominal: 0,
                valid_bits: intent_bits,
            },
        );
        // WorkCentre 3119 model optics, from Xerox W31BR-01.PDF. These are
        // informational physical specs, not selectable resolutions. Actual
        // X/Y scan settings remain the intersection of INQUIRY and the core.
        // https://www.office.xerox.com/latest/W31BR-01.PDF
        add_long(
            &mut catalog,
            WIA_IPS_OPTICAL_XRES,
            "Horizontal Optical Resolution",
            600,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPS_OPTICAL_YRES,
            "Vertical Optical Resolution",
            2400,
            ro_none(VT_I4),
        );
        add_long(
            &mut catalog,
            WIA_IPS_PREVIEW,
            "Preview",
            WIA_FINAL_SCAN,
            rw_list(vec![WIA_FINAL_SCAN], WIA_FINAL_SCAN),
        );
        add_long(
            &mut catalog,
            WIA_SHOW_PREVIEW_CONTROL_ID,
            "Show preview control",
            WIA_DONT_SHOW_PREVIEW_CONTROL,
            ro_none(VT_I4),
        );
        // The driver never rotates; only the unrotated value is accepted.
        add_long(
            &mut catalog,
            WIA_IPS_ROTATION,
            "Rotation",
            WIA_ROTATION_PORTRAIT,
            rw_list(vec![WIA_ROTATION_PORTRAIT], WIA_ROTATION_PORTRAIT),
        );
        // Single fixed region; no segmentation filter and no child items.
        add_long(
            &mut catalog,
            WIA_IPS_SEGMENTATION,
            "Segmentation",
            WIA_DONT_USE_SEGMENTATION_FILTER,
            ro_none(VT_I4),
        );
        let position = PropertyAttribute::RangeLong {
            access: WIA_PROP_RW | WIA_PROP_RANGE,
            min: 0,
            nominal: 0,
            // The initial selection covers the entire available image.
            max: 0,
            step: offset_step(default_resolution),
        };
        add_long(
            &mut catalog,
            WIA_IPS_XEXTENT,
            "Horizontal Extent",
            max_width,
            rw_range(1, max_width, max_width),
        );
        add_long(
            &mut catalog,
            WIA_IPS_XPOS,
            "Horizontal Start Position",
            0,
            position.clone(),
        );
        let x_resolutions = catalog.resolutions.clone();
        let y_resolutions = vec![default_resolution];
        add_long(
            &mut catalog,
            WIA_IPS_XRES,
            "Horizontal Resolution",
            default_resolution,
            rw_list(x_resolutions, default_resolution),
        );
        add_long(
            &mut catalog,
            WIA_IPS_YEXTENT,
            "Vertical Extent",
            max_height,
            rw_range(1, max_height, max_height),
        );
        add_long(
            &mut catalog,
            WIA_IPS_YPOS,
            "Vertical Start Position",
            0,
            position,
        );
        add_long(
            &mut catalog,
            WIA_IPS_YRES,
            "Vertical Resolution",
            default_resolution,
            rw_list(y_resolutions, default_resolution),
        );
        catalog.assert_aligned()?;
        Ok(catalog)
    }

    /// Build the values and effective ranges for an already resolved setting
    /// snapshot. The caller resolves old/new dependent selections first; this
    /// method never silently rounds a requested position or changes the mode.
    /// Failure leaves the source catalog unchanged and performs no SDK writes.
    pub(super) fn with_settings(&self, settings: crate::wia::FlatbedSettings) -> Result<Self, i32> {
        if self.kind != ItemKind::Flatbed
            || !self.resolutions.contains(&settings.x_resolution)
            || !self.data_types.contains(&settings.data_type)
        {
            return Err(E_INVALIDARG);
        }
        settings.to_request().map_err(|_| E_INVALIDARG)?;
        self.assert_aligned()?;
        let (max_width, max_height) = self.max_extent(settings.x_resolution);
        if settings
            .x_position
            .checked_add(settings.x_extent)
            .is_none_or(|end| end > max_width)
            || settings
                .y_position
                .checked_add(settings.y_extent)
                .is_none_or(|end| end > max_height)
        {
            return Err(E_INVALIDARG);
        }
        let size = bmp_size(settings.x_extent, settings.y_extent, settings.depth)?;
        let min_size = min_size_thousandths(settings.x_resolution)?;
        let step = offset_step(settings.x_resolution);
        let mut selected = self.clone();
        for (id, value) in [
            (WIA_IPS_XRES, settings.x_resolution),
            (WIA_IPS_YRES, settings.y_resolution),
            (WIA_IPS_XPOS, settings.x_position),
            (WIA_IPS_YPOS, settings.y_position),
            (WIA_IPS_XEXTENT, settings.x_extent),
            (WIA_IPS_YEXTENT, settings.y_extent),
            (WIA_IPA_DATATYPE, settings.data_type),
            (WIA_IPA_DEPTH, settings.depth),
            (WIA_IPA_CHANNELS_PER_PIXEL, channels_for(settings.data_type)),
            (WIA_IPA_PIXELS_PER_LINE, settings.x_extent),
            (WIA_IPA_NUMBER_OF_LINES, settings.y_extent),
            (WIA_IPA_ITEM_SIZE, size),
            (WIA_IPS_MIN_HORIZONTAL_SIZE, min_size),
            (WIA_IPS_MIN_VERTICAL_SIZE, min_size),
        ] {
            let index = selected.index(id)?;
            selected.initial_values[index] = PropertyValue::Long(value);
        }
        {
            let index = selected.index(WIA_IPS_YRES)?;
            selected.attributes[index] = PropertyAttribute::ListLong {
                access: WIA_PROP_RW | WIA_PROP_LIST,
                values: vec![settings.y_resolution],
                nominal: settings.y_resolution,
            };
            let index = selected.index(WIA_IPA_DEPTH)?;
            selected.attributes[index] = PropertyAttribute::ListLong {
                access: WIA_PROP_RW | WIA_PROP_LIST,
                values: self.all_depths(),
                nominal: settings.depth,
            };
        }
        for (id, min, nominal, max, increment) in [
            (
                WIA_IPS_XPOS,
                0,
                0,
                (max_width - settings.x_extent) / step * step,
                step,
            ),
            (
                WIA_IPS_YPOS,
                0,
                0,
                (max_height - settings.y_extent) / step * step,
                step,
            ),
            (
                WIA_IPS_XEXTENT,
                1,
                max_width - settings.x_position,
                max_width - settings.x_position,
                1,
            ),
            (
                WIA_IPS_YEXTENT,
                1,
                max_height - settings.y_position,
                max_height - settings.y_position,
                1,
            ),
        ] {
            let index = selected.index(id)?;
            selected.attributes[index] = PropertyAttribute::RangeLong {
                access: WIA_PROP_RW | WIA_PROP_RANGE,
                min,
                nominal,
                max,
                step: increment,
            };
        }
        Ok(selected)
    }

    fn index(&self, id: u32) -> Result<usize, i32> {
        self.ids
            .iter()
            .position(|value| *value == id)
            .ok_or(E_UNEXPECTED)
    }

    fn push(&mut self, id: u32, name: &str, value: PropertyValue, attribute: PropertyAttribute) {
        self.ids.push(id);
        self.names.push(name.to_owned());
        self.initial_values.push(value);
        self.attributes.push(attribute);
    }

    fn assert_aligned(&self) -> Result<(), i32> {
        if self.ids.len() == self.names.len()
            && self.ids.len() == self.initial_values.len()
            && self.ids.len() == self.attributes.len()
        {
            Ok(())
        } else {
            Err(E_UNEXPECTED)
        }
    }

    pub(super) fn property_ids(&self) -> &[u32] {
        &self.ids
    }

    pub(super) fn names(&self) -> &[String] {
        &self.names
    }

    pub(super) fn initial_values(&self) -> &[PropertyValue] {
        &self.initial_values
    }

    pub(super) fn attributes(&self) -> &[PropertyAttribute] {
        &self.attributes
    }

    #[cfg(test)]
    pub(super) fn resolutions(&self) -> &[i32] {
        &self.resolutions
    }

    #[cfg(test)]
    pub(super) fn data_types(&self) -> &[i32] {
        &self.data_types
    }

    /// Depths reachable through the advertised data types, in list order.
    pub(super) fn all_depths(&self) -> Vec<i32> {
        self.data_types
            .iter()
            .map(|data_type| depth_for(*data_type))
            .filter(|depth| *depth > 0)
            .collect()
    }

    #[cfg(test)]
    pub(super) fn default_resolution(&self) -> i32 {
        self.default_resolution
    }

    pub(super) fn max_extent(&self, dpi: i32) -> (i32, i32) {
        if self.kind != ItemKind::Flatbed || !self.resolutions.contains(&dpi) {
            return (0, 0);
        }
        (
            pixels_from_units(self.width_units, dpi).unwrap_or(0),
            pixels_from_units(self.height_units, dpi).unwrap_or(0),
        )
    }
}

fn validate_capabilities(caps: &Capabilities) -> Result<(), i32> {
    if caps.identity.is_empty()
        || caps.width_units == 0
        || caps.length_units == 0
        || caps.flatbed_length_units == 0
        || caps.resolution_mask == 0
        || caps.mode_mask == 0
    {
        Err(E_INVALIDARG)
    } else {
        Ok(())
    }
}

fn validate_name(value: &str) -> Result<(), i32> {
    if value.is_empty() || value.encode_utf16().any(|unit| unit == 0) || value.len() > 16 * 1024 {
        Err(E_INVALIDARG)
    } else {
        Ok(())
    }
}

fn pixels_from_units(units: u32, dpi: i32) -> Result<i32, i32> {
    let dpi = u64::try_from(dpi).map_err(|_| E_INVALIDARG)?;
    let pixels = u64::from(units).checked_mul(dpi).ok_or(E_INVALIDARG)? / 1200;
    i32::try_from(pixels).map_err(|_| E_INVALIDARG)
}

fn size_thousandths(units: u32) -> Result<i32, i32> {
    let value = u64::from(units).checked_mul(1000).ok_or(E_INVALIDARG)? / 1200;
    i32::try_from(value).map_err(|_| E_INVALIDARG)
}

fn min_size_thousandths(max_dpi: i32) -> Result<i32, i32> {
    let dpi = u64::try_from(max_dpi).map_err(|_| E_INVALIDARG)?;
    let value = 1000u64.checked_add(dpi - 1).ok_or(E_INVALIDARG)? / dpi;
    i32::try_from(value).map_err(|_| E_INVALIDARG)
}

pub(super) fn offset_step(dpi: i32) -> i32 {
    // One offset unit is 1/100 inch; find the smallest pixel increment that
    // is an exact multiple of it. All selectable DPI values divide 1200.
    let mut a = dpi;
    let mut b = 100;
    while b != 0 {
        (a, b) = (b, a % b);
    }
    dpi / a
}

fn bmp_size(width: i32, height: i32, depth: i32) -> Result<i32, i32> {
    let row_bits = u64::try_from(width)
        .map_err(|_| E_INVALIDARG)?
        .checked_mul(depth as u64)
        .ok_or(E_INVALIDARG)?;
    let stride = row_bits.div_ceil(32).checked_mul(4).ok_or(E_INVALIDARG)?;
    let palette = if depth == 8 { 1024 } else { 0 };
    let size = stride
        .checked_mul(height as u64)
        .and_then(|value| value.checked_add(14 + 40 + palette))
        .ok_or(E_INVALIDARG)?;
    i32::try_from(size).map_err(|_| E_INVALIDARG)
}

fn depth_for(data_type: i32) -> i32 {
    match data_type {
        WIA_DATA_GRAYSCALE => 8,
        WIA_DATA_COLOR => 24,
        _ => 0,
    }
}

fn channels_for(data_type: i32) -> i32 {
    match data_type {
        WIA_DATA_GRAYSCALE => 1,
        WIA_DATA_COLOR => 3,
        _ => 0,
    }
}

fn guid_from_bytes(bytes: [u8; 16]) -> Guid {
    Guid {
        data1: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
        data2: u16::from_le_bytes(bytes[4..6].try_into().unwrap()),
        data3: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
        data4: bytes[8..16].try_into().unwrap(),
    }
}

const E_UNEXPECTED: i32 = 0x8000_ffffu32 as i32;

#[cfg(test)]
mod tests {
    use super::*;

    fn caps() -> Capabilities {
        Capabilities {
            identity: "synthetic".into(),
            resolution_mask: (1 << 0) | (1 << 5) | (1 << 8),
            mode_mask: (1 << 3) | (1 << 5),
            width_units: 10_200,
            length_units: 14_040,
            flatbed_length_units: 14_040,
            line_order: 0,
            compression_mask: 1,
        }
    }

    fn property(catalog: &PropertyCatalog, id: u32) -> (&PropertyValue, &PropertyAttribute) {
        let index = catalog.ids.iter().position(|value| *value == id).unwrap();
        (&catalog.initial_values[index], &catalog.attributes[index])
    }

    fn rgb_crop() -> crate::wia::FlatbedSettings {
        crate::wia::FlatbedSettings {
            x_resolution: 300,
            y_resolution: 300,
            x_position: 3,
            y_position: 6,
            x_extent: 601,
            y_extent: 801,
            data_type: 3,
            depth: 24,
            brightness: 0,
            contrast: 0,
            compression: 0,
            format: BMP_FORMAT,
        }
    }

    #[test]
    fn selected_settings_update_all_dependent_values_without_changing_the_original() {
        let original = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        let saved = original.clone();
        let selected = original.with_settings(rgb_crop()).unwrap();
        assert_eq!(original, saved);
        for (id, expected) in [
            (WIA_IPS_XRES, 300),
            (WIA_IPS_YRES, 300),
            (WIA_IPA_DATATYPE, 3),
            (WIA_IPA_DEPTH, 24),
            (WIA_IPA_CHANNELS_PER_PIXEL, 3),
            (WIA_IPA_BITS_PER_CHANNEL, 8),
            (WIA_IPA_PIXELS_PER_LINE, 601),
            (WIA_IPA_NUMBER_OF_LINES, 801),
            (WIA_IPA_ITEM_SIZE, 1_445_058),
            (WIA_IPS_MIN_HORIZONTAL_SIZE, 4),
            (WIA_IPS_MIN_VERTICAL_SIZE, 4),
        ] {
            assert_eq!(
                property(&selected, id).0,
                &PropertyValue::Long(expected),
                "property {id}"
            );
        }
        // The depth list keeps every reachable depth; only the nominal follows
        // the selection (WinRT reads colour support from this list).
        assert!(matches!(property(&selected, WIA_IPA_DEPTH).1,
            PropertyAttribute::ListLong { values, nominal: 24, .. } if values == &[8, 24]));
        assert!(matches!(property(&selected, WIA_IPS_YRES).1,
            PropertyAttribute::ListLong { values, .. } if values == &[300]));
        for (id, expected_max, expected_step) in [
            (WIA_IPS_XPOS, 1947, 3),
            (WIA_IPS_YPOS, 2709, 3),
            (WIA_IPS_XEXTENT, 2547, 1),
            (WIA_IPS_YEXTENT, 3504, 1),
        ] {
            assert!(
                matches!(property(&selected, id).1,
                PropertyAttribute::RangeLong { max, step, .. }
                if *max == expected_max && *step == expected_step),
                "range {id}"
            );
        }
        for id in [
            WIA_IPA_ITEM_NAME,
            WIA_IPA_FULL_ITEM_NAME,
            WIA_IPA_ACCESS_RIGHTS,
            WIA_IPS_OPTICAL_YRES,
        ] {
            assert_eq!(property(&selected, id), property(&original, id));
        }
    }

    #[test]
    fn settings_reject_unsupported_or_unrepresentable_requests_without_partial_updates() {
        let original = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        let saved = original.clone();
        let good = rgb_crop();
        for invalid in [
            crate::wia::FlatbedSettings {
                x_resolution: 200,
                y_resolution: 200,
                ..good
            },
            crate::wia::FlatbedSettings {
                y_resolution: 600,
                ..good
            },
            crate::wia::FlatbedSettings { depth: 8, ..good },
            crate::wia::FlatbedSettings {
                x_position: 1,
                ..good
            },
            crate::wia::FlatbedSettings {
                x_extent: 2550,
                ..good
            },
            crate::wia::FlatbedSettings {
                y_extent: 0,
                ..good
            },
            crate::wia::FlatbedSettings {
                y_position: i32::MAX,
                ..good
            },
            crate::wia::FlatbedSettings {
                brightness: 1001, // outside the WIA -1000..=1000 range
                ..good
            },
            crate::wia::FlatbedSettings {
                format: [0; 16],
                ..good
            },
        ] {
            assert_eq!(original.with_settings(invalid).unwrap_err(), E_INVALIDARG);
            assert_eq!(original, saved);
        }
        let mut gray_only = caps();
        gray_only.mode_mask = 1 << 3;
        let gray = PropertyCatalog::flatbed(&gray_only, "Flatbed", "Root\\Flatbed").unwrap();
        assert_eq!(gray.with_settings(good).unwrap_err(), E_INVALIDARG);
        assert_eq!(
            PropertyCatalog::root(&caps(), 0)
                .unwrap()
                .with_settings(good)
                .unwrap_err(),
            E_INVALIDARG
        );
    }

    #[test]
    fn selected_edge_rectangle_keeps_only_representable_positions() {
        let original = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        let selected = original
            .with_settings(crate::wia::FlatbedSettings {
                x_resolution: 600,
                y_resolution: 600,
                x_position: 5094,
                y_position: 7014,
                x_extent: 6,
                y_extent: 6,
                data_type: 2,
                depth: 8,
                ..rgb_crop()
            })
            .unwrap();
        assert_eq!(
            property(&selected, WIA_IPA_ITEM_SIZE).0,
            &PropertyValue::Long(1126)
        );
        assert!(matches!(
            property(&selected, WIA_IPS_XPOS).1,
            PropertyAttribute::RangeLong {
                max: 5094,
                step: 6,
                ..
            }
        ));
        assert!(matches!(
            property(&selected, WIA_IPS_XEXTENT).1,
            PropertyAttribute::RangeLong {
                min: 1,
                max: 6,
                step: 1,
                ..
            }
        ));
    }

    #[test]
    fn initial_geometry_and_mode_attributes_match_the_scan_contract() {
        let catalog = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        for id in [WIA_IPS_XPOS, WIA_IPS_YPOS] {
            assert!(matches!(
                property(&catalog, id).1,
                PropertyAttribute::RangeLong {
                    min: 0,
                    nominal: 0,
                    max: 0,
                    step: 3,
                    ..
                }
            ));
        }
        assert_eq!(
            property(&catalog, WIA_IPS_MIN_HORIZONTAL_SIZE).0,
            &PropertyValue::Long(14)
        );
        assert!(matches!(property(&catalog, WIA_IPA_DEPTH).1,
            PropertyAttribute::ListLong { values, nominal: 8, .. } if values == &[8, 24]));
        assert!(matches!(property(&catalog, WIA_IPS_YRES).1,
            PropertyAttribute::ListLong { values, nominal: 75, .. } if values == &[75]));
    }

    #[test]
    fn initial_bmp_size_includes_padding_header_and_palette_and_is_not_cached() {
        let catalog = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        assert_eq!(
            property(&catalog, WIA_IPA_ITEM_SIZE).0,
            &PropertyValue::Long(562_358)
        );
        for id in [
            WIA_IPA_ITEM_SIZE,
            WIA_IPA_NUMBER_OF_LINES,
            WIA_IPA_PIXELS_PER_LINE,
            WIA_IPA_CHANNELS_PER_PIXEL,
            WIA_IPS_MIN_HORIZONTAL_SIZE,
            WIA_IPS_MIN_VERTICAL_SIZE,
        ] {
            assert_eq!(
                property(&catalog, id).1.access() & WIA_PROP_CACHEABLE,
                0,
                "dynamic property {id}"
            );
        }
    }

    #[test]
    fn flatbed_does_not_advertise_upload_or_mutable_access_rights() {
        let catalog = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        let (value, attribute) = property(&catalog, WIA_IPA_ACCESS_RIGHTS);
        assert_eq!(value, &PropertyValue::Long(WIA_ITEM_READ));
        assert_eq!(attribute.access() & WIA_PROP_WRITE, 0);
    }

    #[test]
    fn tone_ranges_follow_the_wia_contract_with_neutral_default() {
        // Microsoft: WIA_IPS_BRIGHTNESS/CONTRAST are WIA_PROP_RANGE −1000..1000,
        // 0 neutral. A degenerate 0..0 range stopped Windows Fax and Scan and
        // the Windows Scan app right after reading these (2026-09-19 traces).
        let catalog = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        for id in [WIA_IPS_BRIGHTNESS, WIA_IPS_CONTRAST] {
            assert!(matches!(
                property(&catalog, id).1,
                PropertyAttribute::RangeLong {
                    min: -1000,
                    nominal: 0,
                    max: 1000,
                    step: 1,
                    ..
                }
            ));
        }
    }

    #[test]
    fn current_intent_advertises_the_colour_modes_the_device_offers() {
        let catalog = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        let (value, attribute) = property(&catalog, WIA_IPS_CUR_INTENT);
        assert_eq!(value, &PropertyValue::Long(0));
        match attribute {
            PropertyAttribute::FlagLong { valid_bits, .. } => {
                assert_ne!(valid_bits & WIA_INTENT_IMAGE_TYPE_COLOR, 0);
                assert_ne!(valid_bits & WIA_INTENT_IMAGE_TYPE_GRAYSCALE, 0);
                assert_eq!(valid_bits & 0x4, 0, "text intent is not offered");
            }
            other => panic!("unexpected attribute {other:?}"),
        }
    }

    #[test]
    fn root_has_its_category_and_read_access_without_claiming_ready() {
        let catalog = PropertyCatalog::root(&caps(), 0).unwrap();
        assert!(matches!(
            property(&catalog, WIA_IPA_ITEM_CATEGORY).0,
            PropertyValue::Guid(_)
        ));
        assert_eq!(
            property(&catalog, WIA_IPA_ACCESS_RIGHTS).0,
            &PropertyValue::Long(WIA_ITEM_READ)
        );
        assert_eq!(
            property(&catalog, WIA_DPS_DOCUMENT_HANDLING_STATUS).0,
            &PropertyValue::Long(0)
        );
        assert_eq!(
            property(&catalog, WIA_DPS_DOCUMENT_HANDLING_SELECT).0,
            &PropertyValue::Long(DOCUMENT_HANDLING_FLATBED)
        );
        assert_eq!(
            property(&catalog, WIA_SHOW_PREVIEW_CONTROL_ID).0,
            &PropertyValue::Long(WIA_DONT_SHOW_PREVIEW_CONTROL)
        );
    }

    #[test]
    fn flatbed_publishes_the_ui_hint_properties_read_by_windows_clients() {
        let catalog = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        assert_eq!(
            property(&catalog, WIA_SHOW_PREVIEW_CONTROL_ID).0,
            &PropertyValue::Long(WIA_DONT_SHOW_PREVIEW_CONTROL)
        );
        assert_eq!(
            property(&catalog, WIA_IPS_SEGMENTATION).0,
            &PropertyValue::Long(WIA_DONT_USE_SEGMENTATION_FILTER)
        );
        assert!(matches!(
            property(&catalog, WIA_IPS_ROTATION).1,
            PropertyAttribute::ListLong { values, nominal: 0, .. } if values == &[0]
        ));
    }

    #[test]
    fn initialization_preserves_the_profile_owned_by_the_wia_service() {
        let catalog = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        assert!(!catalog.property_ids().contains(&4120));
    }

    #[test]
    fn optical_dimensions_are_model_specs_not_selectable_resolution_maxima() {
        let catalog = PropertyCatalog::flatbed(&caps(), "Flatbed", "Root\\Flatbed").unwrap();
        assert_eq!(
            property(&catalog, WIA_IPS_OPTICAL_XRES).0,
            &PropertyValue::Long(600)
        );
        assert_eq!(
            property(&catalog, WIA_IPS_OPTICAL_YRES).0,
            &PropertyValue::Long(2400)
        );
        assert_eq!(catalog.max_extent(300), (2550, 3510));
    }
}
