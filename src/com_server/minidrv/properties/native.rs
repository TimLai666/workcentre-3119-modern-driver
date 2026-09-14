//! Native WIA service-library publication for a [`PropertyCatalog`].
//!
//! The service context is accepted only at the final synchronous boundary.
//! No test in this module fabricates one or calls the WIA service DLL.

use super::catalog::{PropertyAttribute, PropertyCatalog, PropertyValue};
use crate::com_server::Guid;
use std::ffi::c_void;

const S_OK: i32 = 0;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const E_UNEXPECTED: i32 = 0x8000_ffffu32 as i32;
const E_OUTOFMEMORY: i32 = 0x8007_000eu32 as i32;

const PRSPEC_LPWSTR: u32 = 0;
const PRSPEC_PROPID: u32 = 1;
const VT_I4: u16 = 3;
const VT_BSTR: u16 = 8;
const VT_CLSID: u16 = 72;

// `PropertyCatalog::with_settings` owns these dependent values. Keep the
// native delta boundary closed over this set so item identity, format, and
// unrelated read-only defaults can never be written by an update.
const DEPENDENT_VALUE_IDS: &[u32] = &[
    4103, // WIA_IPA_DATATYPE
    4104, // WIA_IPA_DEPTH
    4109, // WIA_IPA_CHANNELS_PER_PIXEL
    4112, // WIA_IPA_PIXELS_PER_LINE
    4114, // WIA_IPA_NUMBER_OF_LINES
    4116, // WIA_IPA_ITEM_SIZE
    6147, // WIA_IPS_XRES
    6148, // WIA_IPS_YRES
    6149, // WIA_IPS_XPOS
    6150, // WIA_IPS_YPOS
    6151, // WIA_IPS_XEXTENT
    6152, // WIA_IPS_YEXTENT
    6167, // WIA_IPS_MIN_HORIZONTAL_SIZE
    6168, // WIA_IPS_MIN_VERTICAL_SIZE
];
const DEPENDENT_ATTRIBUTE_IDS: &[u32] = &[4104, 6148, 6149, 6150, 6151, 6152];

/// A side-effect-free description used by synthetic tests before a native
/// WIA context is available.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WritePlan {
    property_count: usize,
}

impl WritePlan {
    pub(super) fn from_catalog(catalog: &PropertyCatalog) -> Result<Self, i32> {
        if catalog.property_ids().len() != catalog.initial_values().len()
            || catalog.property_ids().len() != catalog.attributes().len()
            || catalog.property_ids().len() != catalog.names().len()
        {
            return Err(E_INVALIDARG);
        }
        Ok(Self {
            property_count: catalog.property_ids().len(),
        })
    }

    #[cfg(test)]
    pub(super) fn property_count(&self) -> usize {
        self.property_count
    }
}

/// Publish names, initial values, and validity descriptions in the order
/// required by the WIA service contract.
///
/// # Safety
/// `context` must be a live WIA service item context supplied by Windows for
/// this synchronous minidriver callback.  It is never dereferenced by Rust;
/// the SDK helpers own all context validation and storage.
pub(super) unsafe fn publish(context: *mut u8, catalog: &PropertyCatalog) -> Result<(), i32> {
    if context.is_null() {
        return Err(E_INVALIDARG);
    }
    let mut writer = NativeWriter { context };
    // SAFETY: the caller of `publish` has the same live WIA context contract;
    // the generic path below only retains raw pointers for synchronous calls.
    publish_with(catalog, &mut writer)
}

trait ServiceWriter {
    fn set_names(&mut self, ids: *mut u32, names: *mut *mut u16, count: i32) -> i32;
    fn write_values(
        &mut self,
        specs: *const PropSpec,
        values: *const PropVariant,
        count: u32,
    ) -> i32;
    fn set_attributes(&mut self, spec: *mut PropSpec, info: *const WiaPropertyInfo) -> i32;
}

struct NativeWriter {
    context: *mut u8,
}

impl ServiceWriter for NativeWriter {
    fn set_names(&mut self, ids: *mut u32, names: *mut *mut u16, count: i32) -> i32 {
        // SAFETY: `publish_with` guarantees all pointers and their backing
        // storage remain live for this synchronous SDK call.
        unsafe { wiasSetItemPropNames(self.context, count, ids, names) }
    }

    fn write_values(
        &mut self,
        specs: *const PropSpec,
        values: *const PropVariant,
        count: u32,
    ) -> i32 {
        // SAFETY: `publish_with` guarantees all pointers and their backing
        // storage remain live for this synchronous SDK call.
        unsafe { wiasWriteMultiple(self.context, count, specs, values) }
    }

    fn set_attributes(&mut self, spec: *mut PropSpec, info: *const WiaPropertyInfo) -> i32 {
        // SAFETY: `publish_with` guarantees all pointers and their backing
        // storage remain live for this synchronous SDK call.
        unsafe { wiasSetItemPropAttribs(self.context, 1, spec, info) }
    }
}

fn publish_with(catalog: &PropertyCatalog, writer: &mut dyn ServiceWriter) -> Result<(), i32> {
    let plan = WritePlan::from_catalog(catalog)?;
    if plan.property_count == 0 || plan.property_count > i32::MAX as usize {
        return Err(E_INVALIDARG);
    }

    let mut ids = catalog.property_ids().to_vec();
    let mut names = Vec::with_capacity(plan.property_count);
    for name in catalog.names() {
        let mut utf16: Vec<u16> = name.encode_utf16().collect();
        utf16.push(0);
        names.push(utf16);
    }
    let mut name_ptrs: Vec<*mut u16> = names.iter_mut().map(Vec::as_mut_ptr).collect();
    let hr = writer.set_names(
        ids.as_mut_ptr(),
        name_ptrs.as_mut_ptr(),
        plan.property_count as i32,
    );
    ensure_s_ok(hr)?;

    let specs: Vec<PropSpec> = ids.iter().copied().map(PropSpec::from_id).collect();
    let variants = PropVariantStorage::from_values(catalog.initial_values())?;
    let raw_variants: Vec<PropVariant> = variants.iter().map(|value| value.raw).collect();
    let hr = writer.write_values(
        specs.as_ptr(),
        raw_variants.as_ptr(),
        plan.property_count as u32,
    );
    ensure_s_ok(hr)?;

    for (spec, attribute) in specs.iter().zip(catalog.attributes()) {
        let storage = AttributeStorage::new(attribute)?;
        let mut spec = *spec;
        let hr = writer.set_attributes(&mut spec, &storage.info);
        ensure_s_ok(hr)?;
    }
    Ok(())
}

/// Apply the effective value and validity deltas for one already-published
/// item, then run the service's property validation helper.
///
/// The service may have accepted an earlier delta when a later write or the
/// finalizer fails. The caller must quarantine the item connection and
/// reinitialize it after any `Err` result; this function deliberately does not
/// attempt an unverified rollback through the WIA service API.
///
/// # Safety
/// `context` must be a live WIA service item context supplied by Windows for
/// this synchronous minidriver callback. The context is passed only to the
/// SDK helpers and must remain valid until `finalize` returns.
pub(super) unsafe fn update<F>(
    context: *mut u8,
    before: &PropertyCatalog,
    after: &PropertyCatalog,
    finalize: F,
) -> Result<(), i32>
where
    F: FnOnce() -> Result<(), i32>,
{
    if context.is_null() {
        return Err(E_INVALIDARG);
    }
    let mut writer = NativeWriter { context };
    update_with(before, after, &mut writer, finalize)
}

struct DeltaPlan {
    value_specs: Vec<PropSpec>,
    value_storage: Vec<PropVariantStorage>,
    attribute_specs: Vec<PropSpec>,
    attribute_storage: Vec<AttributeStorage>,
}

fn build_delta_plan(before: &PropertyCatalog, after: &PropertyCatalog) -> Result<DeltaPlan, i32> {
    let before_plan = WritePlan::from_catalog(before)?;
    let after_plan = WritePlan::from_catalog(after)?;
    if before_plan.property_count == 0
        || after_plan.property_count == 0
        || before_plan.property_count != after_plan.property_count
        || before_plan.property_count > u32::MAX as usize
        || before.property_ids() != after.property_ids()
        || before.names() != after.names()
    {
        return Err(E_INVALIDARG);
    }

    let value_indices: Vec<usize> = before
        .initial_values()
        .iter()
        .zip(after.initial_values())
        .enumerate()
        .filter_map(|(index, (before, after))| (before != after).then_some(index))
        .collect();
    let attribute_indices: Vec<usize> = before
        .attributes()
        .iter()
        .zip(after.attributes())
        .enumerate()
        .filter_map(|(index, (before, after))| (before != after).then_some(index))
        .collect();
    if value_indices
        .iter()
        .any(|&index| !DEPENDENT_VALUE_IDS.contains(&after.property_ids()[index]))
        || attribute_indices
            .iter()
            .any(|&index| !DEPENDENT_ATTRIBUTE_IDS.contains(&after.property_ids()[index]))
    {
        return Err(E_INVALIDARG);
    }

    let value_specs: Vec<PropSpec> = value_indices
        .iter()
        .map(|&index| PropSpec::from_id(after.property_ids()[index]))
        .collect();
    let changed_values: Vec<PropertyValue> = value_indices
        .iter()
        .map(|&index| after.initial_values()[index].clone())
        .collect();
    let value_storage = PropVariantStorage::from_values(&changed_values)?;
    let attribute_specs: Vec<PropSpec> = attribute_indices
        .iter()
        .map(|&index| PropSpec::from_id(after.property_ids()[index]))
        .collect();
    let attribute_storage: Vec<AttributeStorage> = attribute_indices
        .iter()
        .map(|&index| AttributeStorage::new(&after.attributes()[index]))
        .collect::<Result<_, _>>()?;

    Ok(DeltaPlan {
        value_specs,
        value_storage,
        attribute_specs,
        attribute_storage,
    })
}

fn update_with<F>(
    before: &PropertyCatalog,
    after: &PropertyCatalog,
    writer: &mut dyn ServiceWriter,
    finalize: F,
) -> Result<(), i32>
where
    F: FnOnce() -> Result<(), i32>,
{
    let plan = build_delta_plan(before, after)?;
    let raw_values: Vec<PropVariant> = plan.value_storage.iter().map(|value| value.raw).collect();
    if !plan.value_specs.is_empty() {
        let hr = writer.write_values(
            plan.value_specs.as_ptr(),
            raw_values.as_ptr(),
            plan.value_specs.len() as u32,
        );
        ensure_s_ok(hr)?;
    }

    for (spec, storage) in plan
        .attribute_specs
        .iter()
        .zip(plan.attribute_storage.iter())
    {
        let mut spec = *spec;
        let hr = writer.set_attributes(&mut spec, &storage.info);
        ensure_s_ok(hr)?;
    }

    match finalize() {
        Ok(()) => Ok(()),
        Err(hr) if hr < 0 => Err(hr),
        Err(_) => Err(E_UNEXPECTED),
    }
}

fn ensure_s_ok(hr: i32) -> Result<(), i32> {
    if hr == S_OK {
        Ok(())
    } else if hr < 0 {
        Err(hr)
    } else {
        // The WIA entry contract requires S_OK for successful publication;
        // do not let S_FALSE or another positive helper result pass as a
        // successful COM callback merely because SUCCEEDED(hr) is true.
        Err(E_UNEXPECTED)
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
union PropSpecValue {
    propid: u32,
    lpwstr: *mut u16,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct PropSpec {
    kind: u32,
    value: PropSpecValue,
}

impl PropSpec {
    pub(super) fn from_id(propid: u32) -> Self {
        Self {
            kind: PRSPEC_PROPID,
            value: PropSpecValue { propid },
        }
    }

    pub(super) fn id(&self) -> Option<u32> {
        if self.kind != PRSPEC_PROPID {
            return None;
        }
        // SAFETY: `kind == PRSPEC_PROPID` selects the `propid` member of the
        // ABI union. The caller owns the enclosing live `PropSpec` value.
        Some(unsafe { self.value.propid })
    }

    pub(super) fn name_ptr(&self) -> Option<*const u16> {
        if self.kind != PRSPEC_LPWSTR {
            return None;
        }
        // SAFETY: `kind == PRSPEC_LPWSTR` selects the `lpwstr` member of the
        // ABI union. The returned pointer is borrowed from the input spec.
        Some(unsafe { self.value.lpwstr.cast_const() })
    }
}

/// Standard C PROPVARIANT layout from SDK PropIdlBase.h.  The 16-byte data
/// area is represented as two machine words so the x64 ABI is explicit while
/// constructors below only write the active union member's bytes.
#[repr(C)]
#[derive(Clone, Copy)]
struct PropVariant {
    vt: u16,
    reserved1: u16,
    reserved2: u16,
    reserved3: u16,
    data: [usize; 2],
}

impl PropVariant {
    fn zero(vt: u16) -> Self {
        Self {
            vt,
            reserved1: 0,
            reserved2: 0,
            reserved3: 0,
            data: [0; 2],
        }
    }
}

struct PropVariantStorage {
    raw: PropVariant,
    _guid: Option<Box<Guid>>,
    _bstr: Option<BString>,
}

impl PropVariantStorage {
    fn from_values(values: &[PropertyValue]) -> Result<Vec<Self>, i32> {
        values.iter().map(Self::new).collect()
    }

    fn new(value: &PropertyValue) -> Result<Self, i32> {
        match value {
            PropertyValue::Long(value) => {
                let mut raw = PropVariant::zero(VT_I4);
                raw.data[0] = (*value as i64 as u64) as usize;
                Ok(Self {
                    raw,
                    _guid: None,
                    _bstr: None,
                })
            }
            PropertyValue::Guid(value) => {
                let guid = Box::new(*value);
                let mut raw = PropVariant::zero(VT_CLSID);
                raw.data[0] = (&*guid as *const Guid).cast::<c_void>() as usize;
                Ok(Self {
                    raw,
                    _guid: Some(guid),
                    _bstr: None,
                })
            }
            PropertyValue::String(value) => {
                let bstr = BString::new(value)?;
                let mut raw = PropVariant::zero(VT_BSTR);
                raw.data[0] = bstr.0 as usize;
                Ok(Self {
                    raw,
                    _guid: None,
                    _bstr: Some(bstr),
                })
            }
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LongRange {
    min: i32,
    nominal: i32,
    max: i32,
    step: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LongList {
    count: i32,
    nominal: i32,
    values: *mut u8,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct GuidList {
    count: i32,
    nominal: Guid,
    values: *mut Guid,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct LongFlag {
    nominal: i32,
    valid_bits: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NoneValue {
    dummy: i32,
}

#[repr(C)]
union ValidValue {
    range: LongRange,
    list: LongList,
    list_guid: GuidList,
    flag: LongFlag,
    none: NoneValue,
}

/// SDK 10.0.26100.0 `WIA_PROPERTY_INFO` layout from wiamindr_lh.h.
#[repr(C)]
struct WiaPropertyInfo {
    access: u32,
    vt: u16,
    valid: ValidValue,
}

struct AttributeStorage {
    info: WiaPropertyInfo,
    _longs: Vec<i32>,
    _guids: Vec<Guid>,
}

impl AttributeStorage {
    fn new(attribute: &PropertyAttribute) -> Result<Self, i32> {
        match attribute {
            PropertyAttribute::None { access, vt } => Ok(Self {
                info: WiaPropertyInfo {
                    access: *access,
                    vt: *vt,
                    valid: ValidValue {
                        none: NoneValue { dummy: 0 },
                    },
                },
                _longs: Vec::new(),
                _guids: Vec::new(),
            }),
            PropertyAttribute::RangeLong {
                access,
                min,
                nominal,
                max,
                step,
            } => Ok(Self {
                info: WiaPropertyInfo {
                    access: *access,
                    vt: VT_I4,
                    valid: ValidValue {
                        range: LongRange {
                            min: *min,
                            nominal: *nominal,
                            max: *max,
                            step: *step,
                        },
                    },
                },
                _longs: Vec::new(),
                _guids: Vec::new(),
            }),
            PropertyAttribute::ListLong {
                access,
                values,
                nominal,
            } => {
                if values.is_empty() || values.len() > i32::MAX as usize {
                    return Err(E_INVALIDARG);
                }
                let mut longs = values.clone();
                let values_ptr = longs.as_mut_ptr().cast::<u8>();
                Ok(Self {
                    info: WiaPropertyInfo {
                        access: *access,
                        vt: VT_I4,
                        valid: ValidValue {
                            list: LongList {
                                count: longs.len() as i32,
                                nominal: *nominal,
                                values: values_ptr,
                            },
                        },
                    },
                    _longs: longs,
                    _guids: Vec::new(),
                })
            }
            PropertyAttribute::ListGuid {
                access,
                values,
                nominal,
            } => {
                if values.is_empty() || values.len() > i32::MAX as usize {
                    return Err(E_INVALIDARG);
                }
                let mut guids = values.clone();
                let values_ptr = guids.as_mut_ptr();
                Ok(Self {
                    info: WiaPropertyInfo {
                        access: *access,
                        vt: VT_CLSID,
                        valid: ValidValue {
                            list_guid: GuidList {
                                count: guids.len() as i32,
                                nominal: *nominal,
                                values: values_ptr,
                            },
                        },
                    },
                    _longs: Vec::new(),
                    _guids: guids,
                })
            }
            PropertyAttribute::FlagLong {
                access,
                nominal,
                valid_bits,
            } => Ok(Self {
                info: WiaPropertyInfo {
                    access: *access,
                    vt: VT_I4,
                    valid: ValidValue {
                        flag: LongFlag {
                            nominal: *nominal,
                            valid_bits: *valid_bits,
                        },
                    },
                },
                _longs: Vec::new(),
                _guids: Vec::new(),
            }),
        }
    }
}

struct BString(*mut u16);
impl BString {
    fn new(value: &str) -> Result<Self, i32> {
        let words: Vec<u16> = value.encode_utf16().collect();
        if words.len() > u32::MAX as usize {
            return Err(E_INVALIDARG);
        }
        // SAFETY: the source pointer is valid for exactly `words.len()` UTF-16
        // units during this synchronous allocation call.
        let raw = unsafe { SysAllocStringLen(words.as_ptr(), words.len() as u32) };
        if raw.is_null() {
            Err(E_OUTOFMEMORY)
        } else {
            Ok(Self(raw))
        }
    }
}
impl Drop for BString {
    fn drop(&mut self) {
        // SAFETY: BString owns the allocation returned by SysAllocStringLen.
        unsafe { SysFreeString(self.0) }
    }
}

#[link(name = "Wiaservc")]
unsafe extern "system" {
    fn wiasSetItemPropNames(
        context: *mut u8,
        count: i32,
        ids: *mut u32,
        names: *mut *mut u16,
    ) -> i32;
    fn wiasWriteMultiple(
        context: *mut u8,
        count: u32,
        specs: *const PropSpec,
        values: *const PropVariant,
    ) -> i32;
    fn wiasSetItemPropAttribs(
        context: *mut u8,
        count: i32,
        specs: *mut PropSpec,
        info: *const WiaPropertyInfo,
    ) -> i32;
}

#[link(name = "OleAut32")]
unsafe extern "system" {
    fn SysAllocStringLen(value: *const u16, count: u32) -> *mut u16;
    fn SysFreeString(value: *mut u16);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Capabilities;
    use std::{cell::RefCell, mem::offset_of, mem::size_of, ptr, rc::Rc, slice};

    #[link(name = "OleAut32")]
    unsafe extern "system" {
        fn SysStringLen(value: *mut u16) -> u32;
    }

    const WIA_PROP_RANGE: u32 = 0x10;
    const WIA_PROP_LIST: u32 = 0x20;
    const WIA_IPA_DATATYPE: u32 = 4103;
    const WIA_IPA_DEPTH: u32 = 4104;
    const WIA_IPA_CHANNELS_PER_PIXEL: u32 = 4109;
    const WIA_IPA_PIXELS_PER_LINE: u32 = 4112;
    const WIA_IPA_NUMBER_OF_LINES: u32 = 4114;
    const WIA_IPA_ITEM_SIZE: u32 = 4116;
    const WIA_IPS_XRES: u32 = 6147;
    const WIA_IPS_YRES: u32 = 6148;
    const WIA_IPS_XPOS: u32 = 6149;
    const WIA_IPS_YPOS: u32 = 6150;
    const WIA_IPS_XEXTENT: u32 = 6151;
    const WIA_IPS_YEXTENT: u32 = 6152;
    const WIA_IPS_MIN_HORIZONTAL_SIZE: u32 = 6167;
    const WIA_IPS_MIN_VERTICAL_SIZE: u32 = 6168;

    fn synthetic_catalog() -> PropertyCatalog {
        let capabilities = Capabilities {
            identity: "synthetic".to_owned(),
            resolution_mask: (1 << 0) | (1 << 5),
            mode_mask: 1 << 3,
            width_units: 10_200,
            length_units: 14_040,
            flatbed_length_units: 14_040,
            line_order: 0,
            compression_mask: 1,
        };
        PropertyCatalog::flatbed(&capabilities, "Flatbed", "Root\\Flatbed").unwrap()
    }

    #[test]
    fn sdk_property_abi_layout_matches_x64_headers() {
        assert_eq!(size_of::<PropSpec>(), 16);
        assert_eq!(offset_of!(PropSpec, value), 8);
        assert_eq!(size_of::<PropVariant>(), 24);
        assert_eq!(offset_of!(PropVariant, data), 8);
        assert_eq!(size_of::<WiaPropertyInfo>(), 40);
        assert_eq!(offset_of!(WiaPropertyInfo, valid), 8);
    }

    #[test]
    fn property_variant_scalar_storage_uses_documented_types() {
        let guid = Guid {
            data1: 1,
            data2: 2,
            data3: 3,
            data4: [4; 8],
        };
        let values = vec![
            PropertyValue::Long(42),
            PropertyValue::Guid(guid),
            PropertyValue::String("synthetic".to_owned()),
        ];
        let variants = PropVariantStorage::from_values(&values).unwrap();
        assert_eq!(variants[0].raw.vt, VT_I4);
        assert_eq!(variants[0].raw.data[0] as i32, 42);
        assert_eq!(variants[1].raw.vt, VT_CLSID);
        assert_ne!(variants[1].raw.data[0], 0);
        assert_eq!(variants[2].raw.vt, VT_BSTR);
        assert_ne!(variants[2].raw.data[0], 0);
    }

    #[derive(Default)]
    struct RecordingWriter {
        steps: Vec<&'static str>,
        names_result: i32,
        values_result: i32,
        attribute_result: i32,
        attributes_called: usize,
    }

    impl ServiceWriter for RecordingWriter {
        fn set_names(&mut self, _ids: *mut u32, _names: *mut *mut u16, _count: i32) -> i32 {
            self.steps.push("names");
            self.names_result
        }

        fn write_values(
            &mut self,
            _specs: *const PropSpec,
            _values: *const PropVariant,
            _count: u32,
        ) -> i32 {
            self.steps.push("values");
            self.values_result
        }

        fn set_attributes(&mut self, _spec: *mut PropSpec, _info: *const WiaPropertyInfo) -> i32 {
            self.steps.push("attributes");
            self.attributes_called += 1;
            self.attribute_result
        }
    }

    struct ValidatingWriter<'a> {
        expected: &'a PropertyCatalog,
        steps: Vec<&'static str>,
        attributes_called: usize,
    }

    impl<'a> ValidatingWriter<'a> {
        fn new(expected: &'a PropertyCatalog) -> Self {
            Self {
                expected,
                steps: Vec::new(),
                attributes_called: 0,
            }
        }
    }

    fn assert_utf16_name(raw: *const u16, expected: &str) {
        let expected_units: Vec<u16> = expected.encode_utf16().collect();
        // SAFETY: `publish_with` keeps each name allocation alive and appends
        // one terminator for the duration of this synchronous callback.
        let actual = unsafe { slice::from_raw_parts(raw, expected_units.len() + 1) };
        assert_eq!(actual.last(), Some(&0));
        assert_eq!(&actual[..expected_units.len()], expected_units.as_slice());
    }

    fn assert_bstr(raw: *mut u16, expected: &str) {
        assert!(!raw.is_null());
        // SAFETY: `raw` is the BSTR allocated by `PropVariantStorage` for the
        // current synchronous callback, and the allocation remains owned by
        // the caller until that callback returns.
        let length = unsafe { SysStringLen(raw) } as usize;
        let expected_units: Vec<u16> = expected.encode_utf16().collect();
        assert_eq!(length, expected_units.len());
        // SAFETY: `SysStringLen` reports the exact UTF-16 payload length of
        // this live BSTR allocation.
        let actual = unsafe { slice::from_raw_parts(raw, length) };
        assert_eq!(actual, expected_units.as_slice());
    }

    fn assert_property_variant(raw: PropVariant, expected: &PropertyValue) {
        match expected {
            PropertyValue::Long(expected) => {
                assert_eq!(raw.vt, VT_I4);
                assert_eq!(raw.data[0] as i32, *expected);
            }
            PropertyValue::Guid(expected) => {
                assert_eq!(raw.vt, VT_CLSID);
                let pointer = raw.data[0] as *const Guid;
                assert!(!pointer.is_null());
                // SAFETY: `PropVariantStorage` owns this GUID box for the
                // duration of the synchronous `write_values` callback.
                let actual = unsafe { *pointer };
                assert_eq!(actual, *expected);
            }
            PropertyValue::String(expected) => {
                assert_eq!(raw.vt, VT_BSTR);
                assert_bstr(raw.data[0] as *mut u16, expected);
            }
        }
    }

    fn assert_property_attribute(raw: &WiaPropertyInfo, expected: &PropertyAttribute) {
        match expected {
            PropertyAttribute::None { access, vt } => {
                assert_eq!(raw.access, *access);
                assert_eq!(raw.vt, *vt);
                // SAFETY: `AttributeStorage` initialized the `none` union
                // member and keeps it live through this callback.
                let value = unsafe { raw.valid.none };
                assert_eq!(value.dummy, 0);
            }
            PropertyAttribute::RangeLong {
                access,
                min,
                nominal,
                max,
                step,
            } => {
                assert_eq!(raw.access, *access);
                assert_eq!(raw.vt, VT_I4);
                // SAFETY: `AttributeStorage` initialized the `range` union
                // member and keeps it live through this callback.
                let value = unsafe { raw.valid.range };
                assert_eq!(
                    (value.min, value.nominal, value.max, value.step),
                    (*min, *nominal, *max, *step)
                );
            }
            PropertyAttribute::ListLong {
                access,
                values,
                nominal,
            } => {
                assert_eq!(raw.access, *access);
                assert_eq!(raw.vt, VT_I4);
                // SAFETY: `AttributeStorage` initialized the `list` union
                // member and keeps its cloned vector live through this call.
                let value = unsafe { raw.valid.list };
                assert_eq!(value.count, values.len() as i32);
                assert_eq!(value.nominal, *nominal);
                assert!(!value.values.is_null());
                // SAFETY: the pointer and length came from that live cloned
                // vector and are valid for the synchronous callback.
                let actual =
                    unsafe { slice::from_raw_parts(value.values.cast::<i32>(), values.len()) };
                assert_eq!(actual, values.as_slice());
            }
            PropertyAttribute::ListGuid {
                access,
                values,
                nominal,
            } => {
                assert_eq!(raw.access, *access);
                assert_eq!(raw.vt, VT_CLSID);
                // SAFETY: `AttributeStorage` initialized the `list_guid`
                // union member and keeps its cloned vector live through this call.
                let value = unsafe { raw.valid.list_guid };
                assert_eq!(value.count, values.len() as i32);
                assert_eq!(value.nominal, *nominal);
                assert!(!value.values.is_null());
                // SAFETY: the pointer and length came from that live cloned
                // vector and are valid for the synchronous callback.
                let actual = unsafe { slice::from_raw_parts(value.values, values.len()) };
                assert_eq!(actual, values.as_slice());
            }
            PropertyAttribute::FlagLong {
                access,
                nominal,
                valid_bits,
            } => {
                assert_eq!(raw.access, *access);
                assert_eq!(raw.vt, VT_I4);
                // SAFETY: `AttributeStorage` initialized the `flag` union
                // member and keeps it live through this callback.
                let value = unsafe { raw.valid.flag };
                assert_eq!((value.nominal, value.valid_bits), (*nominal, *valid_bits));
            }
        }
    }

    impl ServiceWriter for ValidatingWriter<'_> {
        fn set_names(&mut self, ids: *mut u32, names: *mut *mut u16, count: i32) -> i32 {
            self.steps.push("names");
            assert_eq!(count as usize, self.expected.property_ids().len());
            // SAFETY: `publish_with` passes arrays with exactly `count`
            // elements, all backed by allocations live for this callback.
            let ids = unsafe { slice::from_raw_parts(ids, count as usize) };
            assert_eq!(ids, self.expected.property_ids());
            // SAFETY: the pointer array has exactly `count` live entries.
            let names = unsafe { slice::from_raw_parts(names, count as usize) };
            for (raw, expected) in names.iter().zip(self.expected.names()) {
                assert_utf16_name(*raw, expected);
            }
            S_OK
        }

        fn write_values(
            &mut self,
            specs: *const PropSpec,
            values: *const PropVariant,
            count: u32,
        ) -> i32 {
            self.steps.push("values");
            assert_eq!(count as usize, self.expected.property_ids().len());
            // SAFETY: `publish_with` passes arrays with exactly `count`
            // elements and keeps all pointed-to storage live for this call.
            let specs = unsafe { slice::from_raw_parts(specs, count as usize) };
            // SAFETY: `publish_with` passes arrays with exactly `count`
            // elements and keeps all pointed-to storage live for this call.
            let values = unsafe { slice::from_raw_parts(values, count as usize) };
            for ((spec, value), (expected_id, expected_value)) in
                specs.iter().zip(values.iter()).zip(
                    self.expected
                        .property_ids()
                        .iter()
                        .zip(self.expected.initial_values()),
                )
            {
                assert_eq!(spec.kind, PRSPEC_PROPID);
                // SAFETY: `publish_with` constructs every spec with the
                // `propid` union member active.
                let actual_id = unsafe { spec.value.propid };
                assert_eq!(actual_id, *expected_id);
                assert_property_variant(*value, expected_value);
            }
            S_OK
        }

        fn set_attributes(&mut self, spec: *mut PropSpec, info: *const WiaPropertyInfo) -> i32 {
            self.steps.push("attributes");
            self.attributes_called += 1;
            // SAFETY: `publish_with` passes one initialized spec and one
            // initialized info object, both live for this callback.
            let spec = unsafe { &*spec };
            // SAFETY: `publish_with` passes one initialized info object that
            // remains live for this synchronous callback.
            let info = unsafe { &*info };
            assert_eq!(spec.kind, PRSPEC_PROPID);
            // SAFETY: `publish_with` constructs this spec with the `propid`
            // union member active.
            let id = unsafe { spec.value.propid };
            let index = self.attributes_called - 1;
            assert!(index < self.expected.property_ids().len());
            assert_eq!(id, self.expected.property_ids()[index]);
            assert_property_attribute(info, &self.expected.attributes()[index]);
            S_OK
        }
    }

    fn synthetic_dual_mode_catalog() -> PropertyCatalog {
        let capabilities = Capabilities {
            identity: "synthetic".to_owned(),
            resolution_mask: (1 << 0) | (1 << 5),
            mode_mask: (1 << 3) | (1 << 5),
            width_units: 10_200,
            length_units: 14_040,
            flatbed_length_units: 14_040,
            line_order: 0,
            compression_mask: 1,
        };
        PropertyCatalog::flatbed(&capabilities, "Flatbed", "Root\\Flatbed").unwrap()
    }

    fn settings(
        x_resolution: i32,
        x_position: i32,
        y_position: i32,
        x_extent: i32,
        y_extent: i32,
        data_type: i32,
        depth: i32,
    ) -> crate::wia::FlatbedSettings {
        crate::wia::FlatbedSettings {
            x_resolution,
            y_resolution: x_resolution,
            x_position,
            y_position,
            x_extent,
            y_extent,
            data_type,
            depth,
            brightness: 0,
            contrast: 0,
            compression: 0,
            format: crate::wia::BMP_FORMAT,
        }
    }

    struct DeltaWriter {
        events: Rc<RefCell<Vec<&'static str>>>,
        names_called: usize,
        value_ids: Vec<u32>,
        value_longs: Vec<(u32, i32)>,
        attribute_ids: Vec<u32>,
        list_values: Vec<(u32, Vec<i32>)>,
        range_values: Vec<(u32, (i32, i32, i32, i32))>,
        values_result: i32,
        attribute_result: i32,
    }

    impl DeltaWriter {
        fn new(events: Rc<RefCell<Vec<&'static str>>>) -> Self {
            Self {
                events,
                names_called: 0,
                value_ids: Vec::new(),
                value_longs: Vec::new(),
                attribute_ids: Vec::new(),
                list_values: Vec::new(),
                range_values: Vec::new(),
                values_result: S_OK,
                attribute_result: S_OK,
            }
        }
    }

    impl ServiceWriter for DeltaWriter {
        fn set_names(&mut self, _ids: *mut u32, _names: *mut *mut u16, _count: i32) -> i32 {
            self.names_called += 1;
            self.events.borrow_mut().push("names");
            S_OK
        }

        fn write_values(
            &mut self,
            specs: *const PropSpec,
            values: *const PropVariant,
            count: u32,
        ) -> i32 {
            self.events.borrow_mut().push("values");
            // SAFETY: `update_with` keeps both arrays and their backing
            // storage live for this synchronous writer callback.
            let specs = unsafe { slice::from_raw_parts(specs, count as usize) };
            // SAFETY: `update_with` passes the matching initialized variants
            // for the exact count above.
            let values = unsafe { slice::from_raw_parts(values, count as usize) };
            for (spec, value) in specs.iter().zip(values) {
                assert_eq!(spec.kind, PRSPEC_PROPID);
                // SAFETY: the production builder initializes the propid union
                // member for every delta spec.
                let id = unsafe { spec.value.propid };
                self.value_ids.push(id);
                assert_eq!(value.vt, VT_I4);
                self.value_longs.push((id, value.data[0] as i32));
            }
            self.values_result
        }

        fn set_attributes(&mut self, spec: *mut PropSpec, info: *const WiaPropertyInfo) -> i32 {
            self.events.borrow_mut().push("attributes");
            // SAFETY: `update_with` passes one initialized spec and info object
            // that remain live for this synchronous writer callback.
            let spec = unsafe { &*spec };
            // SAFETY: `update_with` passes one initialized info object that
            // remains live for this synchronous writer callback.
            let info = unsafe { &*info };
            assert_eq!(spec.kind, PRSPEC_PROPID);
            // SAFETY: the production builder initializes the propid union
            // member for every delta spec.
            let id = unsafe { spec.value.propid };
            self.attribute_ids.push(id);
            if info.access & WIA_PROP_LIST != 0 {
                // SAFETY: this test only sends changed integer-list
                // attributes, whose list member is initialized by the builder.
                let list = unsafe { info.valid.list };
                assert_eq!(info.vt, VT_I4);
                // SAFETY: the list pointer and count refer to live
                // `AttributeStorage` backing memory for this callback.
                let values = unsafe {
                    slice::from_raw_parts(list.values.cast::<i32>(), list.count as usize)
                };
                self.list_values.push((id, values.to_vec()));
            } else {
                assert_ne!(info.access & WIA_PROP_RANGE, 0);
                // SAFETY: this test only sends changed integer-range
                // attributes, whose range member is initialized by the builder.
                let range = unsafe { info.valid.range };
                assert_eq!(info.vt, VT_I4);
                self.range_values
                    .push((id, (range.min, range.nominal, range.max, range.step)));
            }
            self.attribute_result
        }
    }

    #[test]
    fn publish_uses_one_ordered_pipeline_and_stops_after_names_failure() {
        let catalog = synthetic_catalog();
        let mut writer = RecordingWriter {
            names_result: 1,
            ..RecordingWriter::default()
        };
        let result = publish_with(&catalog, &mut writer);
        assert_eq!(result, Err(E_UNEXPECTED));
        assert_eq!(writer.steps, vec!["names"]);
        assert_eq!(writer.attributes_called, 0);
    }

    #[test]
    fn publish_stops_before_attributes_when_values_fail() {
        let catalog = synthetic_catalog();
        let mut writer = RecordingWriter {
            values_result: 0x8000_4005u32 as i32,
            ..RecordingWriter::default()
        };
        let result = publish_with(&catalog, &mut writer);
        assert_eq!(result, Err(0x8000_4005u32 as i32));
        assert_eq!(writer.steps, vec!["names", "values"]);
        assert_eq!(writer.attributes_called, 0);
    }

    #[test]
    fn publish_normalizes_attribute_s_false_and_does_not_continue() {
        let catalog = synthetic_catalog();
        let mut writer = RecordingWriter {
            attribute_result: 1,
            ..RecordingWriter::default()
        };
        let result = publish_with(&catalog, &mut writer);
        assert_eq!(result, Err(E_UNEXPECTED));
        assert_eq!(writer.steps.len(), 3);
        assert_eq!(writer.steps[..2], ["names", "values"]);
        assert_eq!(writer.attributes_called, 1);
    }

    #[test]
    fn publish_success_validates_every_native_payload_and_keeps_order() {
        let catalog = synthetic_catalog();
        let expected_count = catalog.property_ids().len();
        let mut writer = ValidatingWriter::new(&catalog);
        assert_eq!(publish_with(&catalog, &mut writer), Ok(()));
        assert_eq!(writer.attributes_called, expected_count);
        assert_eq!(&writer.steps[..2], ["names", "values"]);
        assert!(writer.steps[2..].iter().all(|step| *step == "attributes"));
    }

    #[test]
    fn update_gray_to_rgb_writes_only_changed_values_and_depth_list() {
        let before = synthetic_dual_mode_catalog();
        let after = before
            .with_settings(settings(75, 0, 0, 637, 877, 3, 24))
            .unwrap();
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut writer = DeltaWriter::new(events.clone());
        let mut finalized = false;
        assert_eq!(
            update_with(&before, &after, &mut writer, || {
                events.borrow_mut().push("finalize");
                finalized = true;
                Ok(())
            }),
            Ok(())
        );
        assert!(finalized);
        assert_eq!(writer.names_called, 0);
        assert_eq!(
            writer.value_ids,
            vec![
                WIA_IPA_CHANNELS_PER_PIXEL,
                WIA_IPA_DATATYPE,
                WIA_IPA_DEPTH,
                WIA_IPA_ITEM_SIZE
            ]
        );
        assert_eq!(
            writer.value_longs,
            vec![
                (WIA_IPA_CHANNELS_PER_PIXEL, 3),
                (WIA_IPA_DATATYPE, 3),
                (WIA_IPA_DEPTH, 24),
                (WIA_IPA_ITEM_SIZE, 1_676_878),
            ]
        );
        assert_eq!(writer.attribute_ids, vec![WIA_IPA_DEPTH]);
        assert_eq!(writer.list_values, vec![(WIA_IPA_DEPTH, vec![24])]);
        assert_eq!(&*events.borrow(), &["values", "attributes", "finalize"]);
    }

    #[test]
    fn update_resolution_and_geometry_writes_only_effective_deltas() {
        let before = synthetic_dual_mode_catalog();
        let after = before
            .with_settings(settings(300, 3, 6, 601, 801, 2, 8))
            .unwrap();
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut writer = DeltaWriter::new(events.clone());
        assert_eq!(update_with(&before, &after, &mut writer, || Ok(())), Ok(()));
        assert_eq!(writer.names_called, 0);
        assert_eq!(
            writer.value_ids,
            vec![
                WIA_IPA_ITEM_SIZE,
                WIA_IPS_MIN_HORIZONTAL_SIZE,
                WIA_IPS_MIN_VERTICAL_SIZE,
                WIA_IPA_NUMBER_OF_LINES,
                WIA_IPA_PIXELS_PER_LINE,
                WIA_IPS_XEXTENT,
                WIA_IPS_XPOS,
                WIA_IPS_XRES,
                WIA_IPS_YEXTENT,
                WIA_IPS_YPOS,
                WIA_IPS_YRES,
            ]
        );
        assert_eq!(
            writer.value_longs,
            vec![
                (WIA_IPA_ITEM_SIZE, 484_882),
                (WIA_IPS_MIN_HORIZONTAL_SIZE, 4),
                (WIA_IPS_MIN_VERTICAL_SIZE, 4),
                (WIA_IPA_NUMBER_OF_LINES, 801),
                (WIA_IPA_PIXELS_PER_LINE, 601),
                (WIA_IPS_XEXTENT, 601),
                (WIA_IPS_XPOS, 3),
                (WIA_IPS_XRES, 300),
                (WIA_IPS_YEXTENT, 801),
                (WIA_IPS_YPOS, 6),
                (WIA_IPS_YRES, 300),
            ]
        );
        assert_eq!(
            writer.attribute_ids,
            vec![
                WIA_IPS_XEXTENT,
                WIA_IPS_XPOS,
                WIA_IPS_YEXTENT,
                WIA_IPS_YPOS,
                WIA_IPS_YRES
            ]
        );
        assert_eq!(writer.list_values, vec![(WIA_IPS_YRES, vec![300])]);
        assert_eq!(
            writer.range_values,
            vec![
                (WIA_IPS_XEXTENT, (1, 2547, 2547, 1)),
                (WIA_IPS_XPOS, (0, 0, 1947, 3)),
                (WIA_IPS_YEXTENT, (1, 3504, 3504, 1)),
                (WIA_IPS_YPOS, (0, 0, 2709, 3)),
            ]
        );
        assert_eq!(
            &*events.borrow(),
            &[
                "values",
                "attributes",
                "attributes",
                "attributes",
                "attributes",
                "attributes"
            ]
        );
    }

    #[test]
    fn update_rejects_mismatched_names_before_any_call() {
        let before = synthetic_dual_mode_catalog();
        let capabilities = Capabilities {
            identity: "synthetic".to_owned(),
            resolution_mask: (1 << 0) | (1 << 5),
            mode_mask: (1 << 3) | (1 << 5),
            width_units: 10_200,
            length_units: 14_040,
            flatbed_length_units: 14_040,
            line_order: 0,
            compression_mask: 1,
        };
        let after = PropertyCatalog::flatbed(&capabilities, "Other", "Root\\Other").unwrap();
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut writer = DeltaWriter::new(events.clone());
        let mut finalized = false;
        assert_eq!(
            update_with(&before, &after, &mut writer, || {
                finalized = true;
                Ok(())
            }),
            Err(E_INVALIDARG)
        );
        assert!(!finalized);
        assert_eq!(writer.names_called, 0);
        assert!(events.borrow().is_empty());
    }

    #[test]
    fn update_stops_on_value_attribute_and_finalize_failures() {
        let before = synthetic_dual_mode_catalog();
        let after = before
            .with_settings(settings(300, 3, 6, 601, 801, 2, 8))
            .unwrap();
        let error = 0x8000_4005u32 as i32;
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut writer = DeltaWriter::new(events.clone());
        writer.values_result = error;
        assert_eq!(
            update_with(&before, &after, &mut writer, || Ok(())),
            Err(error)
        );
        assert_eq!(&*events.borrow(), &["values"]);

        events.borrow_mut().clear();
        let mut writer = DeltaWriter::new(events.clone());
        writer.attribute_result = 1;
        assert_eq!(
            update_with(&before, &after, &mut writer, || Ok(())),
            Err(E_UNEXPECTED)
        );
        assert_eq!(&*events.borrow(), &["values", "attributes"]);

        events.borrow_mut().clear();
        let mut writer = DeltaWriter::new(events.clone());
        assert_eq!(
            update_with(&before, &after, &mut writer, || {
                events.borrow_mut().push("finalize");
                Err(error)
            }),
            Err(error)
        );
        assert_eq!(events.borrow().first(), Some(&"values"));
        assert_eq!(events.borrow().last(), Some(&"finalize"));

        events.borrow_mut().clear();
        let mut writer = DeltaWriter::new(events.clone());
        assert_eq!(
            update_with(&before, &after, &mut writer, || {
                events.borrow_mut().push("finalize");
                Err(S_OK)
            }),
            Err(E_UNEXPECTED)
        );
        assert_eq!(events.borrow().last(), Some(&"finalize"));
    }

    #[test]
    fn publish_rejects_null_before_any_writer_step() {
        let catalog = synthetic_catalog();
        // SAFETY: `publish` rejects the null context before making an SDK call.
        let result = unsafe { publish(ptr::null_mut(), &catalog) };
        assert_eq!(result, Err(E_INVALIDARG));
    }
}
