use crate::{com_server::Guid, wia::FlatbedSettings};
use std::{ffi::c_void, mem::MaybeUninit, ptr, slice};

use super::{E_INVALIDARG, E_POINTER};

mod catalog;
mod native;
mod read_entry;
mod validation;
mod validation_entry;
pub(super) use read_entry::entry as read_entry;
pub(super) use validation_entry::entry as validate_entry;

const S_OK: i32 = 0;
const E_UNEXPECTED: i32 = 0x8000_ffffu32 as i32;
const BOOL_TRUE: i32 = 1;
const MAX_BSTR_CODE_UNITS: usize = 16 * 1024;

// SDK 10.0.26100.0 wiadef.h item type constants.
const WIA_ITEM_TYPE_IMAGE: i32 = 0x0000_0001;
const WIA_ITEM_TYPE_FILE: i32 = 0x0000_0002;
const WIA_ITEM_TYPE_PROGRAMMABLE: i32 = 0x0008_0000;
const WIA_ITEM_TYPE_ROOT: i32 = 0x0000_0008;
const WIA_ITEM_TYPE_TRANSFER: i32 = 0x0000_2000;
const ROOT_STATUS_UNKNOWN: i32 = 0;

// SDK 10.0.26100.0 wiadef.h property identifiers.
const WIA_IPA_ITEM_NAME: u32 = 4098;
const WIA_IPA_FULL_ITEM_NAME: u32 = 4099;
const WIA_IPA_DATATYPE: u32 = 4103;
const WIA_IPA_DEPTH: u32 = 4104;
const WIA_IPA_FORMAT: u32 = 4106;
const WIA_IPA_COMPRESSION: u32 = 4107;
const WIA_IPS_XRES: u32 = 6147;
const WIA_IPS_YRES: u32 = 6148;
const WIA_IPS_XPOS: u32 = 6149;
const WIA_IPS_YPOS: u32 = 6150;
const WIA_IPS_XEXTENT: u32 = 6151;
const WIA_IPS_YEXTENT: u32 = 6152;
const WIA_IPS_BRIGHTNESS: u32 = 6154;
const WIA_IPS_CONTRAST: u32 = 6155;

pub(super) struct Snapshot {
    pub(super) settings: FlatbedSettings,
    pub(super) item: String,
    pub(super) full_item: String,
}

/// Initialize the property set for one real WIA service item.
///
/// The live capability query and the service-library writes share the one
/// lifecycle borrow created by `locking::with_live_capabilities_guarded`. The WIA
/// context is only passed to SDK helpers; no service object is fabricated by
/// this module.  The root status is intentionally initialized to zero because
/// the current STI layer exposes no cover/paper status field.  A later status
/// implementation must publish the corresponding WIA flags.
///
/// # Safety
/// `this` is this module's live `IWiaMiniDrv` interface, `context` is a live
/// WIA service item context for this synchronous callback, and `error` points
/// to writable SDK `LONG` storage.
pub(super) unsafe extern "system" fn init_entry(
    this: *mut c_void,
    context: *mut u8,
    flags: i32,
    error: *mut i32,
) -> i32 {
    if error.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller supplied writable error storage and it remains live
    // until all synchronous WIA helper calls return.
    unsafe { *error = 0 };
    let result = super::super::catch_hresult(|| {
        if this.is_null() || context.is_null() || flags != 0 {
            return E_INVALIDARG;
        }
        // SAFETY: `this` is the embedded interface supplied by the COM caller;
        // the locking helper retains the connection and blocks reentrancy for
        // the entire capability/publish closure.
        let interface = unsafe { &*this.cast::<super::Interface>() };
        // SAFETY: the interface is the live COM object above; the helper owns
        // the connection borrow and keeps all SDK calls synchronous within the
        // closure, including the final publication.
        unsafe {
            super::locking::with_live_capabilities_guarded(interface, |capabilities, quarantine| {
                let mut item_type = MaybeUninit::<i32>::uninit();
                // SAFETY: WIA supplied the live opaque context; this local LONG is
                // writable storage matching the SDK declaration.
                let hr = wiasGetItemType(context, item_type.as_mut_ptr());
                if hr != S_OK {
                    return Err(helper_failure(hr));
                }
                // SAFETY: S_OK from wiasGetItemType initializes this output.
                let item_type = item_type.assume_init();
                let catalog = if item_type & WIA_ITEM_TYPE_ROOT != 0 {
                    if item_type & (WIA_ITEM_TYPE_IMAGE | WIA_ITEM_TYPE_TRANSFER) != 0 {
                        return Err(E_INVALIDARG);
                    }
                    catalog::PropertyCatalog::root(&capabilities, ROOT_STATUS_UNKNOWN)
                } else if item_type
                    & (WIA_ITEM_TYPE_IMAGE
                        | WIA_ITEM_TYPE_FILE
                        | WIA_ITEM_TYPE_TRANSFER
                        | WIA_ITEM_TYPE_PROGRAMMABLE)
                    == (WIA_ITEM_TYPE_IMAGE
                        | WIA_ITEM_TYPE_FILE
                        | WIA_ITEM_TYPE_TRANSFER
                        | WIA_ITEM_TYPE_PROGRAMMABLE)
                {
                    // The service creates and owns these standard item-name
                    // properties. Preserve their actual values instead of
                    // replacing them with a guessed tree path.
                    let (item_name, full_item_name) = read_item_names(context)?;
                    catalog::PropertyCatalog::flatbed(&capabilities, &item_name, &full_item_name)
                } else {
                    Err(E_INVALIDARG)
                }?;
                // SAFETY: the service context is live for this synchronous SDK
                // publication; `catalog` owns all temporary pointers until return.
                // Initial publication can partially succeed just like updates.
                // Never reuse this COM object after a failed SDK write.
                *quarantine = true;
                native::publish(context, &catalog)
            })
        }
        .map_or_else(|error| error, |_| S_OK)
    });
    // SAFETY: `error` was checked non-null above and remains writable.
    unsafe { super::report(error, result) }
}

fn helper_failure(hr: i32) -> i32 {
    if hr < 0 { hr } else { E_UNEXPECTED }
}

fn read_item_names(context: *mut u8) -> Result<(String, String), i32> {
    let item = read_service_string(context, WIA_IPA_ITEM_NAME)?;
    if !valid_item_name(&item) {
        return Err(E_INVALIDARG);
    }
    let full_item = read_service_string(context, WIA_IPA_FULL_ITEM_NAME)?;
    if !valid_item_name(&full_item) {
        return Err(E_INVALIDARG);
    }
    Ok((item, full_item))
}

fn read_service_string(context: *mut u8, propid: u32) -> Result<String, i32> {
    let mut raw = ptr::null_mut();
    // SAFETY: `init_entry` received this live service context from WIA and the
    // returned BSTR is released on every path below.
    let hr = unsafe { wiasReadPropStr(context, propid, &mut raw, ptr::null_mut(), BOOL_TRUE) };
    if hr != S_OK {
        if !raw.is_null() {
            // SAFETY: failed helper output is still an owned BSTR slot.
            unsafe { SysFreeString(raw) };
        }
        return Err(helper_failure(hr));
    }
    // SAFETY: S_OK transfers ownership of the returned BSTR to this wrapper.
    let value = unsafe { BString::from_raw(raw)? };
    // SAFETY: BString owns a valid BSTR and releases it after conversion.
    unsafe { value.into_string() }
}

trait PropertyReader {
    fn item_type(&mut self) -> Result<i32, i32>;
    fn long(&mut self, propid: u32) -> Result<i32, i32>;
    fn guid(&mut self, propid: u32) -> Result<Guid, i32>;
    fn string(&mut self, propid: u32) -> Result<String, i32>;
}

/// Read the current settings from a WIA application-item context.
///
/// The context is owned by the WIA service and is only borrowed for the
/// duration of this call. This function never dereferences it directly. The
/// service-library helpers receive it as an opaque pointer instead.
///
/// # Safety
/// `context` must be a live WIA service context supplied to a minidriver entry
/// point by the WIA service, and it must remain valid for the synchronous
/// helper calls made by this function. A null pointer is rejected.
pub(super) unsafe fn read(context: *mut u8) -> Result<Snapshot, i32> {
    if context.is_null() {
        return Err(E_INVALIDARG);
    }
    let mut reader = NativeReader { context };
    read_values(&mut reader)
}

fn read_values<R: PropertyReader>(reader: &mut R) -> Result<Snapshot, i32> {
    let item_type = reader.item_type()?;
    if item_type & WIA_ITEM_TYPE_ROOT != 0
        || item_type & WIA_ITEM_TYPE_IMAGE == 0
        || item_type & WIA_ITEM_TYPE_TRANSFER == 0
    {
        return Err(E_INVALIDARG);
    }

    let settings = FlatbedSettings {
        x_resolution: reader.long(WIA_IPS_XRES)?,
        y_resolution: reader.long(WIA_IPS_YRES)?,
        x_position: reader.long(WIA_IPS_XPOS)?,
        y_position: reader.long(WIA_IPS_YPOS)?,
        x_extent: reader.long(WIA_IPS_XEXTENT)?,
        y_extent: reader.long(WIA_IPS_YEXTENT)?,
        data_type: reader.long(WIA_IPA_DATATYPE)?,
        depth: reader.long(WIA_IPA_DEPTH)?,
        brightness: reader.long(WIA_IPS_BRIGHTNESS)?,
        contrast: reader.long(WIA_IPS_CONTRAST)?,
        compression: reader.long(WIA_IPA_COMPRESSION)?,
        format: guid_bytes(reader.guid(WIA_IPA_FORMAT)?),
    };
    let item = reader.string(WIA_IPA_ITEM_NAME)?;
    if !valid_item_name(&item) {
        return Err(E_INVALIDARG);
    }
    let full_item = reader.string(WIA_IPA_FULL_ITEM_NAME)?;
    if !valid_item_name(&full_item) {
        return Err(E_INVALIDARG);
    }

    // Keep the existing WIA-to-core validation in one place. This reader only
    // snapshots service properties and never writes or normalizes them.
    settings.to_request().map_err(|_| E_INVALIDARG)?;
    Ok(Snapshot {
        settings,
        item,
        full_item,
    })
}

fn valid_item_name(value: &str) -> bool {
    !value.is_empty() && !value.encode_utf16().any(|unit| unit == 0)
}

fn guid_bytes(value: Guid) -> [u8; 16] {
    let mut bytes = [0; 16];
    bytes[0..4].copy_from_slice(&value.data1.to_le_bytes());
    bytes[4..6].copy_from_slice(&value.data2.to_le_bytes());
    bytes[6..8].copy_from_slice(&value.data3.to_le_bytes());
    bytes[8..16].copy_from_slice(&value.data4);
    bytes
}

struct NativeReader {
    context: *mut u8,
}

impl PropertyReader for NativeReader {
    fn item_type(&mut self) -> Result<i32, i32> {
        let mut value = MaybeUninit::<i32>::uninit();
        // SAFETY: `read` validated the borrowed service context; the local
        // output has space for one SDK LONG and is live for this call.
        let hr = unsafe { wiasGetItemType(self.context, value.as_mut_ptr()) };
        if hr != S_OK {
            return Err(hr);
        }
        // SAFETY: the SDK writes the output before returning S_OK.
        Ok(unsafe { value.assume_init() })
    }

    fn long(&mut self, propid: u32) -> Result<i32, i32> {
        let mut value = MaybeUninit::<i32>::uninit();
        // SAFETY: the context is borrowed for this call and the local LONG is
        // writable storage matching the SDK declaration.
        let hr = unsafe {
            wiasReadPropLong(
                self.context,
                propid,
                value.as_mut_ptr(),
                ptr::null_mut(),
                BOOL_TRUE,
            )
        };
        if hr != S_OK {
            return Err(hr);
        }
        // SAFETY: the SDK writes the output before returning S_OK.
        Ok(unsafe { value.assume_init() })
    }

    fn guid(&mut self, propid: u32) -> Result<Guid, i32> {
        let mut value = MaybeUninit::<Guid>::uninit();
        // SAFETY: the context is borrowed for this call and `Guid` has the
        // SDK-compatible repr(C) layout used by the WIA GUID output.
        let hr = unsafe {
            wiasReadPropGuid(
                self.context,
                propid,
                value.as_mut_ptr(),
                ptr::null_mut(),
                BOOL_TRUE,
            )
        };
        if hr != S_OK {
            return Err(hr);
        }
        // SAFETY: the SDK writes the output before returning S_OK.
        Ok(unsafe { value.assume_init() })
    }

    fn string(&mut self, propid: u32) -> Result<String, i32> {
        let mut raw = ptr::null_mut();
        // SAFETY: the context is borrowed for this call; `raw` is writable
        // BSTR output storage and the old-value output is intentionally unused.
        let hr =
            unsafe { wiasReadPropStr(self.context, propid, &mut raw, ptr::null_mut(), BOOL_TRUE) };
        if hr != S_OK {
            if !raw.is_null() {
                // SAFETY: a non-null failed output is still the BSTR slot
                // documented by wiasReadPropStr; release it before returning.
                unsafe { SysFreeString(raw) };
            }
            return Err(hr);
        }
        // SAFETY: S_OK transfers the returned BSTR to the caller, and BString
        // releases it on every conversion path.
        unsafe { BString::from_raw(raw)?.into_string() }
    }
}

struct BString(*mut u16);

impl BString {
    unsafe fn from_raw(raw: *mut u16) -> Result<Self, i32> {
        if raw.is_null() {
            Err(E_POINTER)
        } else {
            Ok(Self(raw))
        }
    }

    unsafe fn into_string(self) -> Result<String, i32> {
        // SAFETY: the wrapper was constructed only from a non-null BSTR.
        let length = unsafe { SysStringLen(self.0) } as usize;
        if length > MAX_BSTR_CODE_UNITS {
            return Err(E_INVALIDARG);
        }
        // SAFETY: SysStringLen bounds the initialized UTF-16 code units.
        let words = unsafe { slice::from_raw_parts(self.0, length) };
        String::from_utf16(words).map_err(|_| E_INVALIDARG)
    }
}

impl Drop for BString {
    fn drop(&mut self) {
        // SAFETY: BString owns exactly the BSTR returned by wiasReadPropStr.
        unsafe { SysFreeString(self.0) }
    }
}

#[link(name = "Wiaservc")]
unsafe extern "system" {
    // SDK 10.0.26100.0 wiamdef.h lines 128-135 and 152.
    fn wiasGetItemType(context: *mut u8, item_type: *mut i32) -> i32;
    fn wiasReadPropStr(
        context: *mut u8,
        propid: u32,
        value: *mut *mut u16,
        old_value: *mut *mut u16,
        must_exist: i32,
    ) -> i32;
    fn wiasReadPropLong(
        context: *mut u8,
        propid: u32,
        value: *mut i32,
        old_value: *mut i32,
        must_exist: i32,
    ) -> i32;
    fn wiasReadPropGuid(
        context: *mut u8,
        propid: u32,
        value: *mut Guid,
        old_value: *mut Guid,
        must_exist: i32,
    ) -> i32;
}

#[link(name = "OleAut32")]
unsafe extern "system" {
    fn SysFreeString(value: *mut u16);
    #[cfg(test)]
    fn SysAllocStringLen(value: *const u16, length: u32) -> *mut u16;
    fn SysStringLen(value: *mut u16) -> u32;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wia::BMP_FORMAT;

    #[test]
    fn capability_catalog_derives_supported_modes_resolutions_and_geometry() {
        let capabilities = crate::protocol::Capabilities {
            identity: "synthetic".to_owned(),
            resolution_mask: (1 << 0) | (1 << 5) | (1 << 8),
            mode_mask: (1 << 3) | (1 << 5),
            width_units: 10_200,
            length_units: 14_040,
            flatbed_length_units: 14_040,
            line_order: 0,
            compression_mask: 1,
        };

        let catalog = catalog::PropertyCatalog::flatbed(&capabilities, "Flatbed", "Root\\Flatbed")
            .expect("synthetic capabilities produce a catalog");
        assert_eq!(catalog.resolutions(), &[75, 300, 600]);
        assert_eq!(catalog.data_types(), &[2, 3]);
        assert_eq!(catalog.default_resolution(), 75);
        assert_eq!(catalog.max_extent(75), (637, 877));
        assert_eq!(catalog.property_ids().len(), catalog.initial_values().len());
        assert_eq!(catalog.property_ids().len(), catalog.attributes().len());
    }

    #[test]
    fn capability_catalog_rejects_untransferable_reported_capabilities() {
        let capabilities = crate::protocol::Capabilities {
            identity: "synthetic".to_owned(),
            resolution_mask: 1 << 19,
            mode_mask: 1 << 1,
            width_units: 10_200,
            length_units: 14_040,
            flatbed_length_units: 14_040,
            line_order: 0,
            compression_mask: 1,
        };

        assert_eq!(
            catalog::PropertyCatalog::flatbed(&capabilities, "Flatbed", "Root\\Flatbed")
                .unwrap_err(),
            E_INVALIDARG
        );
    }

    #[test]
    fn native_plan_validates_catalog_shape_without_context() {
        let capabilities = crate::protocol::Capabilities {
            identity: "synthetic".to_owned(),
            resolution_mask: 1 << 5,
            mode_mask: 1 << 3,
            width_units: 10_200,
            length_units: 14_040,
            flatbed_length_units: 14_040,
            line_order: 0,
            compression_mask: 1,
        };
        let catalog =
            catalog::PropertyCatalog::flatbed(&capabilities, "Flatbed", "Root\\Flatbed").unwrap();
        let plan = native::WritePlan::from_catalog(&catalog).unwrap();
        assert_eq!(plan.property_count(), catalog.property_ids().len());
    }

    const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
    const S_FALSE: i32 = 1;
    const ITEM_TYPE_FLATBED: i32 = 0x0008_2003u32 as i32;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Call {
        ItemType,
        Long(u32),
        Guid(u32),
        String(u32),
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Failure {
        ItemType,
        Long(u32),
        Guid(u32),
        String(u32),
    }

    struct SyntheticReader {
        item_type: i32,
        longs: Vec<(u32, i32)>,
        format: Guid,
        item: String,
        full_item: String,
        fail: Option<(Failure, i32)>,
        calls: Vec<Call>,
    }

    impl SyntheticReader {
        fn valid() -> Self {
            Self {
                item_type: ITEM_TYPE_FLATBED,
                longs: vec![
                    (6147, 300),
                    (6148, 300),
                    (6149, 0),
                    (6150, 0),
                    (6151, 600),
                    (6152, 800),
                    (4103, 2),
                    (4104, 8),
                    (6154, 0),
                    (6155, 0),
                    (4107, 0),
                ],
                format: guid_from_bytes(BMP_FORMAT),
                item: "Flatbed".to_owned(),
                full_item: "synthetic\\Root\\Flatbed".to_owned(),
                fail: None,
                calls: Vec::new(),
            }
        }

        fn should_fail(&self, failure: Failure) -> Option<i32> {
            self.fail
                .filter(|(expected, _)| *expected == failure)
                .map(|(_, error)| error)
        }

        fn long_value(&self, propid: u32) -> i32 {
            self.longs
                .iter()
                .find_map(|(id, value)| (*id == propid).then_some(*value))
                .expect("test property exists")
        }
    }

    impl PropertyReader for SyntheticReader {
        fn item_type(&mut self) -> Result<i32, i32> {
            self.calls.push(Call::ItemType);
            self.should_fail(Failure::ItemType)
                .map_or(Ok(self.item_type), Err)
        }

        fn long(&mut self, propid: u32) -> Result<i32, i32> {
            self.calls.push(Call::Long(propid));
            self.should_fail(Failure::Long(propid))
                .map_or_else(|| Ok(self.long_value(propid)), Err)
        }

        fn guid(&mut self, propid: u32) -> Result<Guid, i32> {
            self.calls.push(Call::Guid(propid));
            self.should_fail(Failure::Guid(propid))
                .map_or(Ok(self.format), Err)
        }

        fn string(&mut self, propid: u32) -> Result<String, i32> {
            self.calls.push(Call::String(propid));
            if let Some(error) = self.should_fail(Failure::String(propid)) {
                return Err(error);
            }
            match propid {
                4098 => Ok(self.item.clone()),
                4099 => Ok(self.full_item.clone()),
                _ => panic!("test property exists"),
            }
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

    #[test]
    fn valid_flatbed_snapshot_reads_every_required_property() {
        let mut reader = SyntheticReader::valid();
        let snapshot = read_values(&mut reader).expect("valid synthetic properties");

        assert_eq!(snapshot.item, "Flatbed");
        assert_eq!(snapshot.full_item, "synthetic\\Root\\Flatbed");
        assert_eq!(snapshot.settings.x_resolution, 300);
        assert_eq!(snapshot.settings.y_resolution, 300);
        assert_eq!(snapshot.settings.x_extent, 600);
        assert_eq!(snapshot.settings.y_extent, 800);
        assert_eq!(snapshot.settings.data_type, 2);
        assert_eq!(snapshot.settings.depth, 8);
        assert_eq!(snapshot.settings.format, BMP_FORMAT);

        assert_eq!(
            reader.calls,
            vec![
                Call::ItemType,
                Call::Long(6147),
                Call::Long(6148),
                Call::Long(6149),
                Call::Long(6150),
                Call::Long(6151),
                Call::Long(6152),
                Call::Long(4103),
                Call::Long(4104),
                Call::Long(6154),
                Call::Long(6155),
                Call::Long(4107),
                Call::Guid(4106),
                Call::String(4098),
                Call::String(4099),
            ]
        );
    }

    #[test]
    fn invalid_item_type_is_rejected_before_property_reads() {
        for item_type in [0x0000_0008, 0x0008_2002, 0x0000_0001] {
            let mut reader = SyntheticReader::valid();
            reader.item_type = item_type;

            assert_eq!(read_values(&mut reader).map(|_| ()), Err(E_INVALIDARG));
            assert_eq!(reader.calls, vec![Call::ItemType]);
        }
    }

    #[test]
    fn non_s_ok_property_hresult_is_preserved_and_stops_reads() {
        let mut reader = SyntheticReader::valid();
        reader.fail = Some((Failure::Long(6148), S_FALSE));

        assert_eq!(read_values(&mut reader).map(|_| ()), Err(S_FALSE));
        assert_eq!(
            reader.calls,
            vec![Call::ItemType, Call::Long(6147), Call::Long(6148)]
        );
    }

    #[test]
    fn string_hresult_is_preserved_without_reading_the_next_name() {
        let mut reader = SyntheticReader::valid();
        let error = 0x8021_0006u32 as i32;
        reader.fail = Some((Failure::String(4098), error));

        assert_eq!(read_values(&mut reader).map(|_| ()), Err(error));
        assert_eq!(reader.calls.last(), Some(&Call::String(4098)));
        assert!(!reader.calls.contains(&Call::String(4099)));
    }

    #[test]
    fn invalid_settings_are_rejected_after_the_snapshot_is_read() {
        let mut reader = SyntheticReader::valid();
        reader.longs.retain(|(id, _)| *id != 6154);
        reader.longs.push((6154, 1));

        assert_eq!(read_values(&mut reader).map(|_| ()), Err(E_INVALIDARG));
        assert_eq!(reader.calls.len(), 15);
    }

    #[test]
    fn empty_item_name_is_rejected_after_required_properties_are_read() {
        let mut reader = SyntheticReader::valid();
        reader.item.clear();

        assert_eq!(read_values(&mut reader).map(|_| ()), Err(E_INVALIDARG));
        assert_eq!(reader.calls.len(), 14);
        assert!(!reader.calls.contains(&Call::String(4099)));
    }

    #[test]
    fn bstr_conversion_accepts_valid_text_and_rejects_overlong_text() {
        let valid: Vec<u16> = "Flatbed".encode_utf16().collect();
        // SAFETY: the source slice contains exactly the requested UTF-16 units;
        // the returned BSTR is immediately wrapped by its owning RAII type.
        let raw = unsafe { SysAllocStringLen(valid.as_ptr(), valid.len() as u32) };
        assert!(!raw.is_null());
        // SAFETY: raw is a live BSTR allocated by SysAllocStringLen above.
        let text = unsafe { BString::from_raw(raw).unwrap().into_string() };
        assert_eq!(text.as_deref(), Ok("Flatbed"));

        let overlong = vec![b'x' as u16; MAX_BSTR_CODE_UNITS + 1];
        // SAFETY: the source slice contains exactly the requested UTF-16 units;
        // the returned BSTR is immediately wrapped by its owning RAII type.
        let raw = unsafe { SysAllocStringLen(overlong.as_ptr(), overlong.len() as u32) };
        assert!(!raw.is_null());
        // SAFETY: raw is a live BSTR allocated by SysAllocStringLen above.
        let result = unsafe { BString::from_raw(raw).unwrap().into_string() };
        assert_eq!(result, Err(E_INVALIDARG));
    }

    #[test]
    fn native_entry_rejects_null_without_calling_wia() {
        assert_eq!(
            // SAFETY: null is rejected before any native helper is reached.
            unsafe { read(std::ptr::null_mut()) }.map(|_| ()),
            Err(E_INVALIDARG)
        );
    }
}
