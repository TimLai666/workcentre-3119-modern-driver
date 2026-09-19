#![allow(clippy::upper_case_acronyms)]

use super::session::{AccessError, SessionSlot};
use crate::{protocol, usb};
use std::{
    ffi::c_void,
    io,
    mem::size_of,
    sync::{Mutex, MutexGuard},
};

type HRESULT = i32;
type DWORD = u32;
type HKEY = *mut c_void;

type QueryInterfaceFn = super::QueryInterfaceFn;
type AddRefFn = super::AddRefFn;
type ReleaseFn = super::ReleaseFn;

type InitializeFn = unsafe extern "system" fn(*mut c_void, *mut c_void, DWORD, HKEY) -> HRESULT;
type GetCapabilitiesFn = unsafe extern "system" fn(*mut c_void, *mut StiUsdCaps) -> HRESULT;
type GetStatusFn = unsafe extern "system" fn(*mut c_void, *mut StiDeviceStatus) -> HRESULT;
type DeviceResetFn = unsafe extern "system" fn(*mut c_void) -> HRESULT;
type DiagnosticFn = unsafe extern "system" fn(*mut c_void, *mut StiDiag) -> HRESULT;
type EscapeFn = unsafe extern "system" fn(
    *mut c_void,
    DWORD,
    *mut c_void,
    DWORD,
    *mut c_void,
    DWORD,
    *mut DWORD,
) -> HRESULT;
type GetLastErrorFn = unsafe extern "system" fn(*mut c_void, *mut DWORD) -> HRESULT;
type LockDeviceFn = unsafe extern "system" fn(*mut c_void) -> HRESULT;
type UnLockDeviceFn = unsafe extern "system" fn(*mut c_void) -> HRESULT;
type RawReadDataFn =
    unsafe extern "system" fn(*mut c_void, *mut c_void, *mut DWORD, *mut c_void) -> HRESULT;
type RawWriteDataFn =
    unsafe extern "system" fn(*mut c_void, *mut c_void, DWORD, *mut c_void) -> HRESULT;
type RawReadCommandFn =
    unsafe extern "system" fn(*mut c_void, *mut c_void, *mut DWORD, *mut c_void) -> HRESULT;
type RawWriteCommandFn =
    unsafe extern "system" fn(*mut c_void, *mut c_void, DWORD, *mut c_void) -> HRESULT;
type SetNotificationHandleFn = unsafe extern "system" fn(*mut c_void, *mut c_void) -> HRESULT;
type GetNotificationDataFn = unsafe extern "system" fn(*mut c_void, *mut StiNotify) -> HRESULT;
type GetLastErrorInfoFn = unsafe extern "system" fn(*mut c_void, *mut StiErrorInfo) -> HRESULT;

type QueryInterfaceDeviceControlFn = super::QueryInterfaceFn;
type AddRefDeviceControlFn = super::AddRefFn;
type ReleaseDeviceControlFn = super::ReleaseFn;
type GetMyDevicePortNameDeviceControlFn =
    unsafe extern "system" fn(*mut c_void, *mut u16, DWORD) -> HRESULT;

const S_OK: HRESULT = 0;
const E_POINTER: HRESULT = 0x8000_4003u32 as HRESULT;
const E_INVALIDARG: HRESULT = 0x8007_0057u32 as HRESULT;

const ERROR_NOT_READY: u32 = 21;
const ERROR_ALREADY_INITIALIZED: u32 = 1247;
const ERROR_LOCK_VIOLATION: u32 = 33;
const ERROR_NOT_LOCKED: u32 = 158;
const ERROR_OLD_WIN_VERSION: u32 = 1150;
const ERROR_INSUFFICIENT_BUFFER: u32 = 122;
const ERROR_INVALID_NAME: u32 = 123;

const STIERR_INVALID_PARAM: HRESULT = E_INVALIDARG;
const STIERR_NOT_INITIALIZED: HRESULT = hresult_from_win32(ERROR_NOT_READY);
const STIERR_ALREADY_INITIALIZED: HRESULT = hresult_from_win32(ERROR_ALREADY_INITIALIZED);
const STIERR_DEVICE_LOCKED: HRESULT = hresult_from_win32(ERROR_LOCK_VIOLATION);
const STIERR_NEEDS_LOCK: HRESULT = hresult_from_win32(ERROR_NOT_LOCKED);
const STIERR_OLD_VERSION: HRESULT = hresult_from_win32(ERROR_OLD_WIN_VERSION);
const STIERR_INVALID_DEVICE_NAME: HRESULT = hresult_from_win32(ERROR_INVALID_NAME);
const STIERR_UNSUPPORTED: HRESULT = 0x8000_4001u32 as HRESULT;
const STIERR_GENERIC: HRESULT = 0x8000_4005u32 as HRESULT;

const STI_VERSION_FLAG_MASK: DWORD = 0xff00_0000;
const STI_VERSION_FLAG_UNICODE: DWORD = 0x0100_0000;
const STI_VERSION_REAL: DWORD = 0x0000_0002;
pub(super) const STI_VERSION: DWORD = STI_VERSION_REAL | STI_VERSION_FLAG_UNICODE;

// SDK 10.0.26100.0 sti.h: the USD also exposes IWiaMiniDrv.
const STI_GENCAP_WIA: DWORD = 0x0000_0010;
const STI_MAX_INTERNAL_NAME_LENGTH: usize = 128;
const MAX_DEVICE_PATH_WORDS: usize = 32_768;

const STI_DIAGCODE_HWPRESENCE: DWORD = 0x0000_0001;

const INQUIRY_COMMAND: [u8; 4] = [0x1b, 0xa8, 0x12, 0x00];
const INQUIRY_READ_BYTES: usize = 1024;

const fn hresult_from_win32(error: u32) -> HRESULT {
    if error == 0 {
        S_OK
    } else {
        (0x8000_0000u32 | (7 << 16) | (error & 0xffff)) as HRESULT
    }
}

#[repr(C)]
pub(super) struct Vtable {
    pub(super) query_interface: QueryInterfaceFn,
    pub(super) add_ref: AddRefFn,
    pub(super) release: ReleaseFn,

    pub(super) initialize: InitializeFn,
    pub(super) get_capabilities: GetCapabilitiesFn,
    pub(super) get_status: GetStatusFn,
    pub(super) device_reset: DeviceResetFn,
    pub(super) diagnostic: DiagnosticFn,
    pub(super) escape: EscapeFn,
    pub(super) get_last_error: GetLastErrorFn,
    pub(super) lock_device: LockDeviceFn,
    pub(super) un_lock_device: UnLockDeviceFn,
    pub(super) raw_read_data: RawReadDataFn,
    pub(super) raw_write_data: RawWriteDataFn,
    pub(super) raw_read_command: RawReadCommandFn,
    pub(super) raw_write_command: RawWriteCommandFn,
    pub(super) set_notification_handle: SetNotificationHandleFn,
    pub(super) get_notification_data: GetNotificationDataFn,
    pub(super) get_last_error_info: GetLastErrorInfoFn,
}

#[repr(C)]
pub(super) struct StiUsdCaps {
    pub(super) dw_version: DWORD,
    pub(super) dw_generic_caps: DWORD,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct StiDeviceStatus {
    pub(super) dw_size: DWORD,
    pub(super) status_mask: DWORD,
    pub(super) dw_online_state: DWORD,
    pub(super) dw_hardware_status_code: DWORD,
    pub(super) dw_event_handling_state: DWORD,
    pub(super) dw_polling_interval: DWORD,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct StiErrorInfo {
    pub(super) dw_size: DWORD,
    pub(super) dw_generic_error: DWORD,
    pub(super) dw_vendor_error: DWORD,
    pub(super) sz_extended_error_text: [u16; 255],
}

impl StiErrorInfo {
    const fn new() -> Self {
        Self {
            dw_size: size_of::<Self>() as DWORD,
            dw_generic_error: 0,
            dw_vendor_error: 0,
            sz_extended_error_text: [0; 255],
        }
    }

    fn with_message(code: HRESULT, text: &str) -> Self {
        let mut info = Self::new();
        info.dw_generic_error = code as DWORD;
        let limit = info.sz_extended_error_text.len() - 1;
        for (slot, chr) in info
            .sz_extended_error_text
            .iter_mut()
            .zip(text.encode_utf16())
            .take(limit)
        {
            *slot = chr;
        }
        info
    }
}

impl Default for StiErrorInfo {
    fn default() -> Self {
        Self::new()
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct StiDiag {
    pub(super) dw_size: DWORD,
    pub(super) dw_basic_diag_code: DWORD,
    pub(super) dw_vendor_diag_code: DWORD,
    pub(super) dw_status_mask: DWORD,
    pub(super) s_error_info: StiErrorInfo,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct StiNotify {
    pub(super) dw_size: DWORD,
    pub(super) guid_notification_code: super::Guid,
    pub(super) ab_notification_data: [u8; 64],
}

#[repr(C)]
struct StiDeviceControl {
    vtable: *const StiDeviceControlVtable,
}

#[repr(C)]
struct StiDeviceControlVtable {
    query_interface: QueryInterfaceDeviceControlFn,
    add_ref: AddRefDeviceControlFn,
    release: ReleaseDeviceControlFn,

    _initialize: usize,
    _raw_read_data: usize,
    _raw_write_data: usize,
    _raw_read_command: usize,
    _raw_write_command: usize,
    _raw_device_control: usize,
    _get_last_error: usize,
    get_my_device_port_name: GetMyDevicePortNameDeviceControlFn,
    _get_my_device_handle: usize,
    _get_my_device_open_mode: usize,
    _write_to_error_log: usize,
}

pub(super) struct State {
    inner: Mutex<StateInner>,
    session: SessionSlot<usb::UsbSession>,
}

struct StateInner {
    initialized: bool,
    helper: Option<*mut c_void>,
    helper_port_name: Vec<u16>,
    last_error: HRESULT,
    last_error_info: StiErrorInfo,
}

impl State {
    pub(super) const fn new() -> Self {
        Self {
            inner: Mutex::new(StateInner {
                initialized: false,
                helper: None,
                helper_port_name: Vec::new(),
                last_error: S_OK,
                last_error_info: StiErrorInfo::new(),
            }),
            session: SessionSlot::new(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, StateInner> {
        self.inner
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn set_last_error(&self, code: HRESULT, text: &str) {
        let mut inner = self.lock();
        set_last_error_inner(&mut inner, code, text);
    }

    fn clear_last_error(&self) {
        self.set_last_error(S_OK, "");
    }

    fn last_error(&self) -> HRESULT {
        self.lock().last_error
    }

    fn last_error_info(&self) -> StiErrorInfo {
        self.lock().last_error_info
    }

    pub(super) fn scan_bmp<W: io::Write + io::Seek>(
        &self,
        settings: crate::wia::FlatbedSettings,
        cancel: &std::sync::atomic::AtomicBool,
        output: &mut W,
    ) -> io::Result<crate::scan::ScanSummary> {
        let request = settings.to_request()?;
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "Scan cancelled"));
        }
        self.with_scan_session(|usb| {
            crate::wia::scan_request_bmp_in_session(usb, request, cancel, output)
        })
    }

    pub(super) unsafe fn transfer_bmp(
        &self,
        settings: crate::wia::FlatbedSettings,
        cancel: &std::sync::atomic::AtomicBool,
        callback: *mut c_void,
        item: &str,
        full_item: &str,
    ) -> io::Result<crate::wia_transfer::TransferOutcome> {
        let request = settings.to_request()?;
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(crate::wia_transfer::TransferOutcome::Cancelled);
        }
        self.with_scan_session(|usb| {
            // SAFETY: caller supplies a live apartment-compatible callback for
            // the synchronous transfer. The USB lease covers every external call.
            unsafe {
                crate::wia_transfer::transfer_in_session(
                    usb,
                    request,
                    settings.y_extent as u32,
                    cancel,
                    callback,
                    item,
                    full_item,
                )
            }
        })
    }

    pub(super) fn live_capabilities(&self) -> Result<protocol::Capabilities, (HRESULT, String)> {
        let result = self.presence_capabilities();
        match &result {
            Ok(_) => self.clear_last_error(),
            Err((code, text)) => self.set_last_error(*code, text),
        }
        result
    }

    fn presence_capabilities(&self) -> Result<protocol::Capabilities, (HRESULT, String)> {
        if !self.lock().initialized {
            return Err((
                STIERR_NOT_INITIALIZED,
                "IStiUSD is not initialized".to_owned(),
            ));
        }
        self.session
            .with_session(|session| {
                let result = perform_presence_check(session);
                let reusable = result.is_ok();
                (result, reusable)
            })
            .map_err(access_error)
            .and_then(|result| result)
    }

    fn with_scan_session<T>(
        &self,
        run: impl FnOnce(&mut usb::UsbSession) -> (io::Result<T>, crate::scan::SessionHealth),
    ) -> io::Result<T> {
        if !self.lock().initialized {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "IStiUSD is not initialized",
            ));
        }
        let result = self
            .session
            .with_session(|usb| {
                let (result, health) = run(usb);
                (result, health == crate::scan::SessionHealth::Ready)
            })
            .map_err(|error| match error {
                AccessError::Open(error) => error,
                AccessError::Busy => io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "Scanner operation is already active",
                ),
                AccessError::NotLocked => {
                    io::Error::new(io::ErrorKind::NotConnected, "LockDevice is required")
                }
                AccessError::Quarantined => io::Error::new(
                    io::ErrorKind::ConnectionAborted,
                    "Scanner session is quarantined; reconnect before creating a new driver object",
                ),
            })
            .and_then(|result| result);
        match &result {
            Ok(_) => self.clear_last_error(),
            Err(error) => self.set_last_error(io_error_hresult(error), &error.to_string()),
        }
        result
    }
}

fn set_last_error_inner(inner: &mut StateInner, code: HRESULT, text: &str) {
    inner.last_error = code;
    inner.last_error_info = StiErrorInfo::with_message(code, text);
}

impl Drop for State {
    fn drop(&mut self) {
        let _ = self.session.unlock();
        let helper = match self.inner.get_mut() {
            Ok(inner) => inner.helper.take(),
            Err(poisoned) => poisoned.into_inner().helper.take(),
        };
        if let Some(helper) = helper {
            // SAFETY: the helper was AddRef'd during successful Initialize and
            // the call is made after the state lock has been released.
            unsafe { release_helper(helper) };
        }
    }
}

pub(super) static VTABLE: Vtable = Vtable {
    query_interface: super::instance_query_interface,
    add_ref: super::instance_add_ref,
    release: super::instance_release,

    initialize,
    get_capabilities,
    get_status,
    device_reset,
    diagnostic,
    escape,
    get_last_error,
    lock_device,
    un_lock_device,
    raw_read_data,
    raw_write_data,
    raw_read_command,
    raw_write_command,
    set_notification_handle,
    get_notification_data,
    get_last_error_info,
};

unsafe fn state_from_this<'a>(this: *mut c_void) -> Result<&'a State, HRESULT> {
    if this.is_null() {
        return Err(E_POINTER);
    }
    // SAFETY: COM keeps `this` alive for the duration of this call and the
    // parent object layout places State after its vtable and reference count.
    Ok(unsafe { &(*this.cast::<super::Instance>()).state })
}

unsafe fn helper_table<'a>(helper: *mut c_void) -> Result<&'a StiDeviceControlVtable, HRESULT> {
    if helper.is_null() {
        return Err(E_POINTER);
    }
    // SAFETY: Initialize receives a live IStiDeviceControl interface pointer.
    let interface = unsafe { &*helper.cast::<StiDeviceControl>() };
    if interface.vtable.is_null() {
        return Err(E_POINTER);
    }
    // SAFETY: a valid IStiDeviceControl has a live vtable for the call.
    Ok(unsafe { &*interface.vtable })
}

unsafe fn add_ref_helper(helper: *mut c_void) -> Result<(), HRESULT> {
    // SAFETY: caller validates and retains the helper for this synchronous call.
    let table = unsafe { helper_table(helper)? };
    // SAFETY: the helper is a live COM interface and AddRef is slot 1.
    unsafe { (table.add_ref)(helper) };
    Ok(())
}

unsafe fn release_helper(helper: *mut c_void) {
    if helper.is_null() {
        return;
    }
    // SAFETY: the helper reference is owned by State and remains valid until
    // this one Release call completes.
    let interface = unsafe { &*helper.cast::<StiDeviceControl>() };
    if interface.vtable.is_null() {
        return;
    }
    // SAFETY: the helper vtable is valid under the IStiDeviceControl contract.
    let table = unsafe { &*interface.vtable };
    // SAFETY: this releases exactly the reference retained by Initialize.
    unsafe { (table.release)(helper) };
}

fn normalize_external_failure(hr: HRESULT) -> HRESULT {
    if hr < 0 { hr } else { STIERR_GENERIC }
}

fn read_helper_port(helper: *mut c_void) -> Result<Vec<u16>, HRESULT> {
    let table = unsafe {
        // SAFETY: caller owns a valid helper interface for this synchronous call.
        helper_table(helper)?
    };
    let insufficient = hresult_from_win32(ERROR_INSUFFICIENT_BUFFER);
    let mut capacity = STI_MAX_INTERNAL_NAME_LENGTH;
    loop {
        let mut buffer = vec![0u16; capacity];
        // SAFETY: the helper owns the COM method; buffer is writable for the
        // supplied element count and remains live for the synchronous call.
        let hr = unsafe {
            (table.get_my_device_port_name)(helper, buffer.as_mut_ptr(), capacity as DWORD)
        };
        if hr == S_OK {
            let Some(end) = buffer.iter().position(|&value| value == 0) else {
                return Err(STIERR_INVALID_DEVICE_NAME);
            };
            if end == 0 {
                return Err(STIERR_INVALID_DEVICE_NAME);
            }
            buffer.truncate(end + 1);
            return Ok(buffer);
        }
        if hr == insufficient && capacity < MAX_DEVICE_PATH_WORDS {
            capacity = (capacity * 2).min(MAX_DEVICE_PATH_WORDS);
            continue;
        }
        return Err(normalize_external_failure(hr));
    }
}

fn fail(state: &State, code: HRESULT, text: &str) -> HRESULT {
    state.set_last_error(code, text);
    code
}

fn unsupported(state: &State, method: &str) -> HRESULT {
    fail(state, STIERR_UNSUPPORTED, method)
}

unsafe extern "system" fn initialize(
    this: *mut c_void,
    helper: *mut c_void,
    sti_version: DWORD,
    _parameters_key: HKEY,
) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates pointers before dereferencing them.
    super::catch_hresult(|| unsafe { initialize_impl(this, helper, sti_version) })
}

unsafe fn initialize_impl(this: *mut c_void, helper: *mut c_void, sti_version: DWORD) -> HRESULT {
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    if helper.is_null() {
        return fail(state, E_POINTER, "IStiDeviceControl is null");
    }
    if sti_version != STI_VERSION
        || (sti_version & STI_VERSION_FLAG_MASK) != STI_VERSION_FLAG_UNICODE
        || (sti_version & !STI_VERSION_FLAG_MASK) != STI_VERSION_REAL
    {
        return fail(state, STIERR_OLD_VERSION, "Unsupported STI version");
    }
    if state.lock().initialized {
        return fail(
            state,
            STIERR_ALREADY_INITIALIZED,
            "IStiUSD is already initialized",
        );
    }

    // SAFETY: the helper pointer is validated before reading its ABI vtable.
    if let Err(error) = unsafe { helper_table(helper) } {
        return fail(state, error, "Invalid IStiDeviceControl interface");
    }
    // SAFETY: the helper is a live IStiDeviceControl supplied by the STI service.
    if let Err(error) = unsafe { add_ref_helper(helper) } {
        return fail(state, error, "Unable to retain IStiDeviceControl");
    }

    let port_name = match read_helper_port(helper) {
        Ok(port_name) => port_name,
        Err(error) => {
            // SAFETY: AddRef succeeded and no state lock is held here.
            unsafe { release_helper(helper) };
            return fail(state, error, "GetMyDevicePortName failed");
        }
    };

    let mut release_after_publish = None;
    let result = {
        let mut inner = state.lock();
        if inner.initialized {
            set_last_error_inner(
                &mut inner,
                STIERR_ALREADY_INITIALIZED,
                "IStiUSD is already initialized",
            );
            release_after_publish = Some(helper);
            STIERR_ALREADY_INITIALIZED
        } else {
            inner.initialized = true;
            inner.helper = Some(helper);
            inner.helper_port_name = port_name;
            set_last_error_inner(&mut inner, S_OK, "");
            S_OK
        }
    };
    if let Some(helper) = release_after_publish {
        // SAFETY: this is the extra AddRef held by the losing initializer, and
        // the state lock has been released before calling external COM.
        unsafe { release_helper(helper) };
    }
    result
}

unsafe extern "system" fn get_capabilities(
    this: *mut c_void,
    capabilities: *mut StiUsdCaps,
) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates pointers before dereferencing them.
    super::catch_hresult(|| unsafe { get_capabilities_impl(this, capabilities) })
}

unsafe fn get_capabilities_impl(this: *mut c_void, capabilities: *mut StiUsdCaps) -> HRESULT {
    if capabilities.is_null() {
        return E_POINTER;
    }
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    if !state.lock().initialized {
        return fail(state, STIERR_NOT_INITIALIZED, "IStiUSD is not initialized");
    }
    // SAFETY: the caller supplied writable STI_USD_CAPS storage.
    unsafe { *capabilities = usd_capabilities() };
    state.clear_last_error();
    S_OK
}

fn usd_capabilities() -> StiUsdCaps {
    StiUsdCaps {
        dw_version: STI_VERSION,
        dw_generic_caps: STI_GENCAP_WIA,
    }
}

unsafe extern "system" fn get_status(this: *mut c_void, status: *mut StiDeviceStatus) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates pointers before dereferencing them.
    super::catch_hresult(|| unsafe { get_status_impl(this, status) })
}

unsafe fn get_status_impl(this: *mut c_void, status: *mut StiDeviceStatus) -> HRESULT {
    if status.is_null() {
        return E_POINTER;
    }
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    if !state.lock().initialized {
        return fail(state, STIERR_NOT_INITIALIZED, "IStiUSD is not initialized");
    }
    // SAFETY: the caller supplied writable STI_DEVICE_STATUS storage; read its
    // fixed-size input field before writing the full output structure.
    let supplied_size = unsafe { (*status).dw_size };
    if supplied_size != size_of::<StiDeviceStatus>() as DWORD {
        return fail(
            state,
            STIERR_INVALID_PARAM,
            "Invalid STI_DEVICE_STATUS size",
        );
    }
    // The device has no verified status/event command. Return an empty status
    // and E_NOTIMPL instead of claiming that the scanner is operational.
    // SAFETY: the caller supplied writable STI_DEVICE_STATUS storage.
    unsafe {
        *status = StiDeviceStatus {
            dw_size: size_of::<StiDeviceStatus>() as DWORD,
            status_mask: 0,
            dw_online_state: 0,
            dw_hardware_status_code: 0,
            dw_event_handling_state: 0,
            dw_polling_interval: 0,
        };
    }
    unsupported(state, "GetStatus is not supported")
}

unsafe extern "system" fn device_reset(this: *mut c_void) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the receiver before dereferencing it.
    super::catch_hresult(|| unsafe { device_reset_impl(this) })
}

unsafe fn device_reset_impl(this: *mut c_void) -> HRESULT {
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    unsupported(state, "DeviceReset is not supported")
}

unsafe extern "system" fn diagnostic(this: *mut c_void, buffer: *mut StiDiag) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates pointers and structure size.
    super::catch_hresult(|| unsafe { diagnostic_impl(this, buffer) })
}

unsafe fn write_diagnostic_error(buffer: *mut StiDiag, code: HRESULT, text: &str) {
    // SAFETY: callers check the pointer and exact SDK structure size before
    // writing this complete diagnostic response.
    unsafe {
        (*buffer).dw_status_mask = 0;
        (*buffer).s_error_info = StiErrorInfo::with_message(code, text);
    }
}

unsafe fn write_diagnostic_success(buffer: *mut StiDiag, basic: DWORD, vendor: DWORD) {
    // SAFETY: callers check the pointer and exact SDK structure size before
    // writing this complete diagnostic response.
    unsafe {
        *buffer = StiDiag {
            dw_size: size_of::<StiDiag>() as DWORD,
            dw_basic_diag_code: basic,
            dw_vendor_diag_code: vendor,
            dw_status_mask: 0,
            s_error_info: StiErrorInfo::new(),
        };
    }
}

fn diagnostic_failure(state: &State, buffer: *mut StiDiag, code: HRESULT, text: &str) -> HRESULT {
    state.set_last_error(code, text);
    // SAFETY: diagnostic_impl validates the exact SDK structure size first.
    unsafe { write_diagnostic_error(buffer, code, text) };
    code
}

fn perform_presence_check(
    session: &mut usb::UsbSession,
) -> Result<protocol::Capabilities, (HRESULT, String)> {
    session.write(&INQUIRY_COMMAND).map_err(|error| {
        (
            io_error_hresult(&error),
            format!("INQUIRY write failed: {error}"),
        )
    })?;
    let mut response = [0u8; INQUIRY_READ_BYTES];
    let transferred = session.read(&mut response).map_err(|error| {
        (
            io_error_hresult(&error),
            format!("INQUIRY read failed: {error}"),
        )
    })?;
    if transferred > response.len() {
        return Err((
            STIERR_GENERIC,
            "INQUIRY returned more data than its bounded buffer".to_owned(),
        ));
    }
    protocol::Capabilities::parse(&response[..transferred]).map_err(|error| {
        (
            STIERR_GENERIC,
            format!("INQUIRY capability response was invalid: {error}"),
        )
    })
}

unsafe fn diagnostic_impl(this: *mut c_void, buffer: *mut StiDiag) -> HRESULT {
    if buffer.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller owns an inout STI_DIAG structure for this call; read
    // only the fixed size field before accepting any output writes.
    let supplied_size = unsafe { (*buffer).dw_size };
    if supplied_size != size_of::<StiDiag>() as DWORD {
        // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
        let state = match unsafe { state_from_this(this) } {
            Ok(state) => state,
            Err(error) => return error,
        };
        return fail(state, STIERR_INVALID_PARAM, "Invalid STI_DIAG size");
    }
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    // SAFETY: the structure size was checked above.
    let (basic, vendor) = unsafe { ((*buffer).dw_basic_diag_code, (*buffer).dw_vendor_diag_code) };
    if basic != STI_DIAGCODE_HWPRESENCE || vendor != 0 {
        return diagnostic_failure(
            state,
            buffer,
            STIERR_INVALID_PARAM,
            "Only STI_DIAGCODE_HWPRESENCE is supported",
        );
    }

    let result = state.live_capabilities();
    match result {
        Ok(_) => {
            state.clear_last_error();
            // SAFETY: the exact SDK structure size was validated above.
            unsafe { write_diagnostic_success(buffer, basic, vendor) };
            S_OK
        }
        Err((code, text)) => diagnostic_failure(state, buffer, code, &text),
    }
}

unsafe extern "system" fn escape(
    this: *mut c_void,
    _escape_function: DWORD,
    _input: *mut c_void,
    _input_size: DWORD,
    _output: *mut c_void,
    _output_size: DWORD,
    actual_size: *mut DWORD,
) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the output pointer.
    super::catch_hresult(|| unsafe { escape_impl(this, actual_size) })
}

unsafe fn escape_impl(this: *mut c_void, actual_size: *mut DWORD) -> HRESULT {
    if actual_size.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller supplied writable output-count storage.
    unsafe { *actual_size = 0 };
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    unsupported(state, "Escape is not supported")
}

unsafe extern "system" fn get_last_error(this: *mut c_void, error_code: *mut DWORD) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the output pointer.
    super::catch_hresult(|| unsafe { get_last_error_impl(this, error_code) })
}

unsafe fn get_last_error_impl(this: *mut c_void, error_code: *mut DWORD) -> HRESULT {
    if error_code.is_null() {
        return E_POINTER;
    }
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    let error = state.last_error();
    // SAFETY: the caller supplied writable DWORD storage.
    unsafe { *error_code = error as DWORD };
    S_OK
}

unsafe extern "system" fn lock_device(this: *mut c_void) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the receiver before dereferencing it.
    super::catch_hresult(|| unsafe { lock_device_impl(this) })
}

fn io_error_hresult(error: &io::Error) -> HRESULT {
    if let Some(code) = error.raw_os_error().filter(|code| *code >= 0) {
        let hresult = hresult_from_win32(code as u32);
        if hresult != S_OK {
            return hresult;
        }
    }
    if error.kind() == io::ErrorKind::Unsupported {
        STIERR_UNSUPPORTED
    } else {
        STIERR_GENERIC
    }
}

fn access_error(error: AccessError) -> (HRESULT, String) {
    match error {
        AccessError::Busy => (STIERR_DEVICE_LOCKED, "Scanner operation is already active".to_owned()),
        AccessError::NotLocked => (STIERR_NEEDS_LOCK, "LockDevice is required".to_owned()),
        AccessError::Quarantined => (STIERR_GENERIC, "Scanner session is quarantined after uncertain transport state; reconnect the device before creating a new driver object".to_owned()),
        AccessError::Open(error) => (io_error_hresult(&error), format!("Opening scanner failed: {error}")),
    }
}

unsafe fn lock_device_impl(this: *mut c_void) -> HRESULT {
    // SAFETY: this is a live IStiUSD pointer for the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    let path = {
        let inner = state.lock();
        if !inner.initialized {
            drop(inner);
            return fail(state, STIERR_NOT_INITIALIZED, "IStiUSD is not initialized");
        }
        inner.helper_port_name.clone()
    };
    match state
        .session
        .open(|| usb::UsbSession::open_matching_path(&path))
    {
        Ok(()) => {
            state.clear_last_error();
            S_OK
        }
        Err(error) => {
            let (code, text) = access_error(error);
            fail(state, code, &text)
        }
    }
}

unsafe extern "system" fn un_lock_device(this: *mut c_void) -> HRESULT {
    // SAFETY: receiver validity is required by the COM contract.
    super::catch_hresult(|| unsafe { un_lock_device_impl(this) })
}

unsafe fn un_lock_device_impl(this: *mut c_void) -> HRESULT {
    // SAFETY: this is a live IStiUSD pointer for the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    if !state.lock().initialized {
        return fail(state, STIERR_NOT_INITIALIZED, "IStiUSD is not initialized");
    }
    match state.session.unlock() {
        Ok(()) => {
            state.clear_last_error();
            S_OK
        }
        Err(error) => {
            let (code, text) = access_error(error);
            fail(state, code, &text)
        }
    }
}

unsafe extern "system" fn raw_read_data(
    this: *mut c_void,
    _buffer: *mut c_void,
    number_of_bytes: *mut DWORD,
    _overlapped: *mut c_void,
) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the transfer-count pointer.
    super::catch_hresult(|| unsafe { raw_read_data_impl(this, number_of_bytes) })
}

unsafe fn raw_read_data_impl(this: *mut c_void, number_of_bytes: *mut DWORD) -> HRESULT {
    if number_of_bytes.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller supplied writable transfer-count storage.
    unsafe { *number_of_bytes = 0 };
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    unsupported(state, "RawReadData is not supported")
}

unsafe extern "system" fn raw_write_data(
    this: *mut c_void,
    _buffer: *mut c_void,
    _number_of_bytes: DWORD,
    _overlapped: *mut c_void,
) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the receiver before dereferencing it.
    super::catch_hresult(|| unsafe { raw_write_data_impl(this) })
}

unsafe fn raw_write_data_impl(this: *mut c_void) -> HRESULT {
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    unsupported(state, "RawWriteData is not supported")
}

unsafe extern "system" fn raw_read_command(
    this: *mut c_void,
    _buffer: *mut c_void,
    number_of_bytes: *mut DWORD,
    _overlapped: *mut c_void,
) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the transfer-count pointer.
    super::catch_hresult(|| unsafe { raw_read_command_impl(this, number_of_bytes) })
}

unsafe fn raw_read_command_impl(this: *mut c_void, number_of_bytes: *mut DWORD) -> HRESULT {
    if number_of_bytes.is_null() {
        return E_POINTER;
    }
    // SAFETY: the caller supplied writable transfer-count storage.
    unsafe { *number_of_bytes = 0 };
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    unsupported(state, "RawReadCommand is not supported")
}

unsafe extern "system" fn raw_write_command(
    this: *mut c_void,
    _buffer: *mut c_void,
    _number_of_bytes: DWORD,
    _overlapped: *mut c_void,
) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the receiver before dereferencing it.
    super::catch_hresult(|| unsafe { raw_write_command_impl(this) })
}

unsafe fn raw_write_command_impl(this: *mut c_void) -> HRESULT {
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    unsupported(state, "RawWriteCommand is not supported")
}

unsafe extern "system" fn set_notification_handle(
    this: *mut c_void,
    _event: *mut c_void,
) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the receiver before dereferencing it.
    super::catch_hresult(|| unsafe { set_notification_handle_impl(this) })
}

unsafe fn set_notification_handle_impl(this: *mut c_void) -> HRESULT {
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    unsupported(state, "SetNotificationHandle is not supported")
}

unsafe extern "system" fn get_notification_data(
    this: *mut c_void,
    notification: *mut StiNotify,
) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the notification pointer.
    super::catch_hresult(|| unsafe { get_notification_data_impl(this, notification) })
}

unsafe fn get_notification_data_impl(this: *mut c_void, notification: *mut StiNotify) -> HRESULT {
    if notification.is_null() {
        return E_POINTER;
    }
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    // SAFETY: LPSTINOTIFY points to the fixed SDK notification structure.
    unsafe {
        *notification = StiNotify {
            dw_size: size_of::<StiNotify>() as DWORD,
            guid_notification_code: super::Guid {
                data1: 0,
                data2: 0,
                data3: 0,
                data4: [0; 8],
            },
            ab_notification_data: [0; 64],
        };
    }
    unsupported(state, "GetNotificationData is not supported")
}

unsafe extern "system" fn get_last_error_info(
    this: *mut c_void,
    error_info: *mut StiErrorInfo,
) -> HRESULT {
    // SAFETY: the COM entry point forwards the caller's ABI arguments to the
    // implementation, which validates the output pointer.
    super::catch_hresult(|| unsafe { get_last_error_info_impl(this, error_info) })
}

unsafe fn get_last_error_info_impl(this: *mut c_void, error_info: *mut StiErrorInfo) -> HRESULT {
    if error_info.is_null() {
        return E_POINTER;
    }
    // SAFETY: `this` is a live IStiUSD pointer for the duration of the COM call.
    let state = match unsafe { state_from_this(this) } {
        Ok(state) => state,
        Err(error) => return error,
    };
    let info = state.last_error_info();
    // SAFETY: GetLastErrorInfo has an output-only fixed SDK structure. Its
    // incoming dwSize is not read or trusted.
    unsafe { *error_info = info };
    S_OK
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usd_declares_wia_support_with_the_unicode_sti_version() {
        let caps = usd_capabilities();
        assert_eq!(caps.dw_version, STI_VERSION);
        assert_eq!(caps.dw_generic_caps, STI_GENCAP_WIA);
    }

    #[test]
    fn live_capabilities_rejects_uninitialized_state_before_session_access() {
        let state = State::new();
        let error = state.live_capabilities().unwrap_err();
        assert_eq!(error.0, STIERR_NOT_INITIALIZED);
    }
}
