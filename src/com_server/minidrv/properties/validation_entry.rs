//! WIA's live property-storage boundary. Tests never fabricate service contexts.

use super::{
    BOOL_TRUE, E_INVALIDARG, E_POINTER, FlatbedSettings, Guid, MAX_BSTR_CODE_UNITS, S_OK,
    WIA_IPA_COMPRESSION, WIA_IPA_DATATYPE, WIA_IPA_DEPTH, WIA_IPA_FORMAT, WIA_IPS_BRIGHTNESS,
    WIA_IPS_CONTRAST, WIA_IPS_XEXTENT, WIA_IPS_XPOS, WIA_IPS_XRES, WIA_IPS_YEXTENT, WIA_IPS_YPOS,
    WIA_IPS_YRES, catalog::PropertyCatalog, guid_bytes, helper_failure, native,
};
use std::{ffi::c_void, ptr, slice};

pub(super) const MAX_PROPERTY_SPECS: u32 = 128;
const SETTING_LONGS: [u32; 11] = [
    WIA_IPS_XRES,
    WIA_IPS_YRES,
    WIA_IPS_XPOS,
    WIA_IPS_YPOS,
    WIA_IPS_XEXTENT,
    WIA_IPS_YEXTENT,
    WIA_IPA_DATATYPE,
    WIA_IPA_DEPTH,
    WIA_IPS_BRIGHTNESS,
    WIA_IPS_CONTRAST,
    WIA_IPA_COMPRESSION,
];

/// # Safety
/// The receiver and context are live objects supplied by WIA. `specs` contains
/// `count` initialized SDK PROPSPECs, with valid terminated strings for names;
/// `error` is writable LONG storage throughout this synchronous callback.
pub(in crate::com_server::minidrv) unsafe extern "system" fn entry(
    this: *mut c_void,
    context: *mut u8,
    flags: i32,
    count: u32,
    specs: *const c_void,
    error: *mut i32,
) -> i32 {
    if error.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller supplies writable output storage.
    unsafe { *error = 0 };
    let result = super::super::super::catch_hresult(|| {
        if this.is_null() || context.is_null() || flags != 0 || count > MAX_PROPERTY_SPECS {
            return E_INVALIDARG;
        }
        if count != 0 && specs.is_null() {
            return E_POINTER;
        }
        // SAFETY: the WIA callback contract supplies the array and strings;
        // count is bounded above, and zero count never dereferences the pointer.
        let requested = match unsafe { read_requested(specs.cast(), count) } {
            Ok(requested) => requested,
            Err(error) => return error,
        };
        // SAFETY: the caller retains our live embedded COM interface.
        let interface = unsafe { &*this.cast::<super::super::Interface>() };
        // SAFETY: the borrow keeps the COM object live and Busy through every
        // synchronous SDK read/write and any failure quarantine.
        unsafe {
            super::super::locking::with_live_capabilities_guarded(interface, |caps, quarantine| {
                let mut reader = super::NativeReader { context };
                let item_type =
                    super::PropertyReader::item_type(&mut reader).map_err(helper_failure)?;
                let catalog = if item_type & super::WIA_ITEM_TYPE_ROOT != 0 {
                    if item_type & (super::WIA_ITEM_TYPE_IMAGE | super::WIA_ITEM_TYPE_TRANSFER) != 0
                    {
                        return Err(E_INVALIDARG);
                    }
                    PropertyCatalog::root(&caps, super::ROOT_STATUS_UNKNOWN)?
                } else {
                    let required = super::WIA_ITEM_TYPE_IMAGE
                        | super::WIA_ITEM_TYPE_FILE
                        | super::WIA_ITEM_TYPE_TRANSFER
                        | super::WIA_ITEM_TYPE_PROGRAMMABLE;
                    if item_type & required != required {
                        return Err(E_INVALIDARG);
                    }
                    let (name, full_name) = super::read_item_names(context)?;
                    PropertyCatalog::flatbed(&caps, &name, &full_name)?
                };
                let ids = canonical_ids(&catalog, &requested)?;
                if ids.is_empty() {
                    return Ok(());
                }
                // All currently advertised root properties are read-only and have
                // already been rejected above. Only flatbed settings reach here.
                let fixed: Vec<_> = ids
                    .iter()
                    .copied()
                    .filter(|id| !SETTING_LONGS.contains(id) && *id != WIA_IPA_FORMAT)
                    .map(native::PropSpec::from_id)
                    .collect();
                validate_specs(context, &fixed)?;
                let mut ids = ids;
                let (old, mut current) = read_settings_pair(context, &ids)?;
                // A written intent selects the data type on the application's
                // behalf; the dependent depth/size then follow as for an
                // explicit WIA_IPA_DATATYPE write.
                if ids.contains(&super::WIA_IPS_CUR_INTENT) {
                    let intent = read_long(context, super::WIA_IPS_CUR_INTENT)?;
                    if let Some(data_type) = super::validation::data_type_for_intent(intent)? {
                        current.data_type = data_type;
                        if !ids.contains(&WIA_IPA_DATATYPE) {
                            ids.push(WIA_IPA_DATATYPE);
                        }
                    }
                }
                let resolved = super::validation::resolve(&catalog, old, current, &ids)?;
                // Attributes baseline from the old state; value baseline from
                // what the item holds now, so snapped/repaired fields are
                // written back even when they equal the old value.
                let before = catalog.with_settings(old)?.with_service_values(current)?;
                let after = catalog.with_settings(resolved)?;
                let canonical: Vec<_> = ids.into_iter().map(native::PropSpec::from_id).collect();
                // SDK publication is not a transaction. A failure after this point
                // may leave partial item state; the lifecycle guard then rejects
                // every subsequent acquisition on this COM object. The service
                // must release it and create a fresh object before scanning again.
                *quarantine = true;
                native::update(context, &before, &after, || {
                    validate_specs(context, &canonical)
                })
            })
        }
        .map_or_else(|error| error, |_| S_OK)
    });
    // SAFETY: output storage remains valid until the callback returns.
    unsafe { super::super::report(error, result) }
}

pub(super) unsafe fn read_requested(
    specs: *const native::PropSpec,
    count: u32,
) -> Result<Vec<RequestedProperty>, i32> {
    if count == 0 {
        return Ok(Vec::new());
    }
    // SAFETY: entry checks pointer/count; WIA owns count initialized specs.
    let specs = unsafe { slice::from_raw_parts(specs, count as usize) };
    let mut requested = Vec::with_capacity(specs.len());
    for spec in specs {
        if let Some(id) = spec.id() {
            requested.push(RequestedProperty::Id(id));
        } else if let Some(name) = spec.name_ptr() {
            if name.is_null() {
                return Err(E_POINTER);
            }
            let mut length = 0;
            // SAFETY: PRSPEC_LPWSTR is a valid NUL-terminated UTF-16 name under
            // the COM contract. The limit bounds work; it is not a pointer probe.
            while length < MAX_BSTR_CODE_UNITS && unsafe { *name.add(length) } != 0 {
                length += 1;
            }
            if length == MAX_BSTR_CODE_UNITS {
                return Err(E_INVALIDARG);
            }
            // SAFETY: the preceding loop visited these initialized code units.
            let words = unsafe { slice::from_raw_parts(name, length) };
            requested.push(RequestedProperty::Name(
                String::from_utf16(words).map_err(|_| E_INVALIDARG)?,
            ));
        } else {
            return Err(E_INVALIDARG);
        }
    }
    Ok(requested)
}

fn read_settings_pair(
    context: *mut u8,
    written: &[u32],
) -> Result<(FlatbedSettings, FlatbedSettings), i32> {
    let mut old = [0; 11];
    let mut current = [0; 11];
    for (index, id) in SETTING_LONGS.into_iter().enumerate() {
        let changed = written.contains(&id);
        let old_out = if changed {
            &mut old[index] as *mut i32
        } else {
            ptr::null_mut()
        };
        // SAFETY: entry retains a real WIA context; both outputs are initialized
        // local LONG storage. Only explicit writes need a previous-value output.
        let hr = unsafe {
            super::wiasReadPropLong(context, id, &mut current[index], old_out, BOOL_TRUE)
        };
        if hr != S_OK {
            return Err(helper_failure(hr));
        }
        if !changed {
            old[index] = current[index];
        }
    }
    let mut format = Guid {
        data1: 0,
        data2: 0,
        data3: 0,
        data4: [0; 8],
    };
    let mut old_format = format;
    let changed = written.contains(&WIA_IPA_FORMAT);
    let old_out = if changed {
        &mut old_format as *mut Guid
    } else {
        ptr::null_mut()
    };
    // SAFETY: the live opaque context and both GUID outputs have the SDK layout.
    let hr = unsafe {
        super::wiasReadPropGuid(context, WIA_IPA_FORMAT, &mut format, old_out, BOOL_TRUE)
    };
    if hr != S_OK {
        return Err(helper_failure(hr));
    }
    if !changed {
        old_format = format;
    }
    let settings = |values: [i32; 11], format: Guid| FlatbedSettings {
        x_resolution: values[0],
        y_resolution: values[1],
        x_position: values[2],
        y_position: values[3],
        x_extent: values[4],
        y_extent: values[5],
        data_type: values[6],
        depth: values[7],
        brightness: values[8],
        contrast: values[9],
        compression: values[10],
        format: guid_bytes(format),
    };
    Ok((settings(old, old_format), settings(current, format)))
}

fn read_long(context: *mut u8, propid: u32) -> Result<i32, i32> {
    let mut value = 0;
    // SAFETY: entry retains a real WIA context; the output is initialized local
    // LONG storage and no previous-value output is requested.
    let hr =
        unsafe { super::wiasReadPropLong(context, propid, &mut value, ptr::null_mut(), BOOL_TRUE) };
    if hr != S_OK {
        return Err(helper_failure(hr));
    }
    Ok(value)
}

fn validate_specs(context: *mut u8, specs: &[native::PropSpec]) -> Result<(), i32> {
    if specs.is_empty() {
        return Ok(());
    }
    // SAFETY: the live service context and bounded owned spec array remain valid
    // for this synchronous helper. No pointer is retained by this wrapper.
    let hr = unsafe { wiasValidateItemProperties(context, specs.len() as u32, specs.as_ptr()) };
    if hr == S_OK {
        Ok(())
    } else {
        Err(helper_failure(hr))
    }
}

#[link(name = "Wiaservc")]
unsafe extern "system" {
    fn wiasValidateItemProperties(
        context: *mut u8,
        count: u32,
        specs: *const native::PropSpec,
    ) -> i32;
}

pub(super) enum RequestedProperty {
    Id(u32),
    Name(String),
}

fn canonical_ids(
    catalog: &PropertyCatalog,
    requested: &[RequestedProperty],
) -> Result<Vec<u32>, i32> {
    let mut ids = Vec::with_capacity(requested.len());
    for property in requested {
        let index = match property {
            RequestedProperty::Id(id) => {
                catalog.property_ids().iter().position(|value| value == id)
            }
            // These are the ASCII SDK names this driver itself registered.
            // Do not assume wiasCreatePropContext resolves name-based specs:
            // a real SDK-only probe showed it does not mark their IDs changed.
            RequestedProperty::Name(name) => catalog
                .names()
                .iter()
                .position(|value| value.eq_ignore_ascii_case(name)),
        }
        .ok_or(E_INVALIDARG)?;
        if catalog.attributes()[index].access() & 2 == 0 {
            return Err(E_INVALIDARG);
        }
        let id = catalog.property_ids()[index];
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Capabilities;

    fn catalog() -> super::super::catalog::PropertyCatalog {
        super::super::catalog::PropertyCatalog::flatbed(
            &Capabilities {
                identity: "synthetic".into(),
                resolution_mask: (1 << 0) | (1 << 5),
                mode_mask: (1 << 3) | (1 << 5),
                width_units: 10200,
                length_units: 14040,
                flatbed_length_units: 14040,
                line_order: 0,
                compression_mask: 1,
            },
            "Flatbed",
            "Root\\Flatbed",
        )
        .unwrap()
    }

    #[test]
    fn property_names_resolve_to_registered_ids_and_duplicates_collapse() {
        let catalog = catalog();
        let requested = [
            RequestedProperty::Name("horizontal resolution".into()),
            RequestedProperty::Id(6147),
            RequestedProperty::Id(4103),
        ];
        assert_eq!(canonical_ids(&catalog, &requested), Ok(vec![6147, 4103]));
        assert_eq!(canonical_ids(&catalog, &[]), Ok(vec![]));
    }

    #[test]
    fn unadvertised_and_read_only_properties_are_rejected_before_publication() {
        let catalog = catalog();
        for requested in [
            RequestedProperty::Name("".into()),
            RequestedProperty::Name("unsupported".into()),
            RequestedProperty::Id(u32::MAX),
            RequestedProperty::Id(4098),
            RequestedProperty::Name("Item Size".into()),
        ] {
            assert_eq!(
                canonical_ids(&catalog, &[requested]),
                Err(super::super::E_INVALIDARG)
            );
        }
    }

    #[test]
    fn native_specs_accept_ids_and_utf16_names_without_a_service_context() {
        #[repr(C)]
        struct NameSpec {
            kind: u32,
            name: *const u16,
        }
        let name: Vec<_> = "Horizontal Resolution\0".encode_utf16().collect();
        let raw = NameSpec {
            kind: 0,
            name: name.as_ptr(),
        };
        assert_eq!(
            std::mem::size_of::<NameSpec>(),
            std::mem::size_of::<native::PropSpec>()
        );
        assert_eq!(std::mem::offset_of!(NameSpec, name), 8);
        // SAFETY: these are real initialized SDK-compatible PROPSPECs with
        // live UTF-16 strings. No opaque WIA service context is constructed.
        unsafe {
            let requested = read_requested((&raw as *const NameSpec).cast(), 1).unwrap();
            assert_eq!(canonical_ids(&catalog(), &requested), Ok(vec![6147]));
            let id = native::PropSpec::from_id(4103);
            let requested = read_requested(&id, 1).unwrap();
            assert_eq!(canonical_ids(&catalog(), &requested), Ok(vec![4103]));
            assert!(read_requested(ptr::null(), 0).unwrap().is_empty());
            let null_name = NameSpec {
                kind: 0,
                name: ptr::null(),
            };
            assert!(matches!(
                read_requested((&null_name as *const NameSpec).cast(), 1),
                Err(E_POINTER)
            ));
            let invalid = NameSpec {
                kind: 2,
                name: ptr::null(),
            };
            assert!(matches!(
                read_requested((&invalid as *const NameSpec).cast(), 1),
                Err(E_INVALIDARG)
            ));
            let invalid_utf16 = [0xd800, 0];
            let invalid = NameSpec {
                kind: 0,
                name: invalid_utf16.as_ptr(),
            };
            assert!(matches!(
                read_requested((&invalid as *const NameSpec).cast(), 1),
                Err(E_INVALIDARG)
            ));
        }
    }
}
