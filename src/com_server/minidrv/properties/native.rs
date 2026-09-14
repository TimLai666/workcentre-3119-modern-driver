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

const PRSPEC_PROPID: u32 = 1;
const VT_I4: u16 = 3;
const VT_BSTR: u16 = 8;
const VT_CLSID: u16 = 72;

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
struct PropSpec {
    kind: u32,
    value: PropSpecValue,
}

impl PropSpec {
    fn from_id(propid: u32) -> Self {
        Self {
            kind: PRSPEC_PROPID,
            value: PropSpecValue { propid },
        }
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
    use std::{mem::offset_of, mem::size_of, ptr, slice};

    #[link(name = "OleAut32")]
    unsafe extern "system" {
        fn SysStringLen(value: *mut u16) -> u32;
    }

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
    fn publish_rejects_null_before_any_writer_step() {
        let catalog = synthetic_catalog();
        // SAFETY: `publish` rejects the null context before making an SDK call.
        let result = unsafe { publish(ptr::null_mut(), &catalog) };
        assert_eq!(result, Err(E_INVALIDARG));
    }
}
