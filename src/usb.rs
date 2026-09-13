//! Exclusive, bounded scanner transfers through Windows WinUSB.

use std::{ffi::c_void, io, mem::size_of, ptr};

// ABI verified against Windows SDK 10.0.26100.0: setupapi.h, winusb.h,
// winusbio.h and usb.h. These APIs do not install or register a driver.
type Handle = *mut c_void;
const INVALID_HANDLE: Handle = -1isize as Handle;
const ERROR_INSUFFICIENT_BUFFER: i32 = 122;
const ERROR_NO_MORE_ITEMS: i32 = 259;
const MAX_TRANSFER_BYTES: usize = 1024 * 1024;
const MAXIMUM_TRANSFER_SIZE_POLICY: u32 = 0x08;

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct Guid {
    a: u32,
    b: u16,
    c: u16,
    d: [u8; 8],
}

const SCANNER_GUID: Guid = Guid {
    a: 0xc4147e4a,
    b: 0x9c41,
    c: 0x4846,
    d: [0xa5, 0x3c, 0x5e, 0x62, 0x5c, 0x68, 0x02, 0x1a],
};

// SP_DEVICE_INTERFACE_DATA and SP_DEVINFO_DATA have the same ABI layout.
// `value` is Flags for interface data and DevInst for device information.
#[repr(C)]
#[derive(Default)]
struct SetupData {
    size: u32,
    guid: Guid,
    value: u32,
    reserved: usize,
}

impl SetupData {
    fn new() -> Self {
        Self {
            size: size_of::<Self>() as u32,
            ..Self::default()
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PipeInfo {
    kind: i32,
    id: u8,
    maximum_packet: u16,
    interval: u8,
}

#[derive(Clone, Copy)]
struct BulkPipes {
    input: u8,
    output: u8,
    input_max_packet: u16,
    output_max_packet: u16,
}

#[link(name = "Setupapi")]
unsafe extern "system" {
    fn SetupDiGetClassDevsW(
        guid: *const Guid,
        enumerator: *const u16,
        parent: Handle,
        flags: u32,
    ) -> Handle;
    fn SetupDiDestroyDeviceInfoList(set: Handle) -> i32;
    fn SetupDiEnumDeviceInterfaces(
        set: Handle,
        device: *const SetupData,
        guid: *const Guid,
        index: u32,
        interface: *mut SetupData,
    ) -> i32;
    fn SetupDiGetDeviceInterfaceDetailW(
        set: Handle,
        interface: *const SetupData,
        detail: *mut c_void,
        bytes: u32,
        required: *mut u32,
        device: *mut SetupData,
    ) -> i32;
    fn SetupDiGetDeviceInstanceIdW(
        set: Handle,
        device: *const SetupData,
        id: *mut u16,
        length: u32,
        required: *mut u32,
    ) -> i32;
}

#[link(name = "Kernel32")]
unsafe extern "system" {
    fn CompareStringOrdinal(
        string1: *const u16,
        count1: i32,
        string2: *const u16,
        count2: i32,
        ignore_case: i32,
    ) -> i32;
    fn CreateFileW(
        path: *const u16,
        access: u32,
        share: u32,
        security: *const c_void,
        creation: u32,
        flags: u32,
        template: Handle,
    ) -> Handle;
    fn CloseHandle(handle: Handle) -> i32;
}

#[link(name = "Winusb")]
unsafe extern "system" {
    fn WinUsb_Initialize(file: Handle, interface: *mut Handle) -> i32;
    fn WinUsb_Free(interface: Handle) -> i32;
    fn WinUsb_GetDescriptor(
        interface: Handle,
        kind: u8,
        index: u8,
        language: u16,
        buffer: *mut u8,
        length: u32,
        transferred: *mut u32,
    ) -> i32;
    fn WinUsb_GetCurrentAlternateSetting(interface: Handle, setting: *mut u8) -> i32;
    fn WinUsb_QueryInterfaceSettings(interface: Handle, setting: u8, descriptor: *mut u8) -> i32;
    fn WinUsb_QueryPipe(interface: Handle, setting: u8, index: u8, pipe: *mut PipeInfo) -> i32;
    fn WinUsb_SetPipePolicy(
        interface: Handle,
        pipe: u8,
        policy: u32,
        length: u32,
        value: *const c_void,
    ) -> i32;
    fn WinUsb_GetPipePolicy(
        interface: Handle,
        pipe: u8,
        policy: u32,
        length: *mut u32,
        value: *mut c_void,
    ) -> i32;
    fn WinUsb_WritePipe(
        interface: Handle,
        pipe: u8,
        buffer: *mut u8,
        length: u32,
        transferred: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
    fn WinUsb_ReadPipe(
        interface: Handle,
        pipe: u8,
        buffer: *mut u8,
        length: u32,
        transferred: *mut u32,
        overlapped: *mut c_void,
    ) -> i32;
}

struct DeviceSet(Handle);
impl Drop for DeviceSet {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner of a successful SetupDiGetClassDevsW result.
        unsafe {
            SetupDiDestroyDeviceInfoList(self.0);
        }
    }
}

struct DeviceFile(Handle);
impl Drop for DeviceFile {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner of a successful CreateFileW result.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

struct UsbHandle(Handle);
impl Drop for UsbHandle {
    fn drop(&mut self) {
        // SAFETY: this is the sole owner of a successful WinUsb_Initialize result.
        unsafe {
            WinUsb_Free(self.0);
        }
    }
}

/// An exclusively opened scanner interface and its validated bulk transport.
///
/// `usb` is declared before `file` because struct fields are dropped in
/// declaration order. This releases the WinUSB interface before closing the
/// underlying device file.
pub(crate) struct UsbSession {
    usb: UsbHandle,
    #[allow(dead_code)]
    // Kept alive for the WinUSB interface lifetime; field declaration order
    // releases the USB handle before this underlying file handle.
    file: DeviceFile,
    pub(crate) descriptor: [u8; 18],
    pub(crate) interface: [u8; 9],
    pub(crate) bulk_in: u8,
    pub(crate) bulk_out: u8,
    pub(crate) bulk_in_max_packet: u16,
    pub(crate) bulk_out_max_packet: u16,
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn win_result(ok: i32, operation: &str) -> io::Result<()> {
    if ok != 0 {
        Ok(())
    } else {
        let error = io::Error::last_os_error();
        Err(io::Error::new(
            error.kind(),
            format!("{operation}: {error}"),
        ))
    }
}

fn validate_scanner_id(id: &str) -> io::Result<()> {
    if crate::identify(id) != Some(crate::Interface::Scanner) {
        return Err(invalid(
            "Registered interface does not belong to the exact scanner MI_00",
        ));
    }
    Ok(())
}

fn interface_detail_with_identity(
    set: &DeviceSet,
    interface: &SetupData,
) -> io::Result<(Vec<u16>, String)> {
    let mut required = 0;
    let mut device = SetupData::new();
    // SAFETY: null detail with zero length queries size; both outputs are writable.
    let sized = unsafe {
        SetupDiGetDeviceInterfaceDetailW(
            set.0,
            interface,
            ptr::null_mut(),
            0,
            &mut required,
            &mut device,
        )
    };
    if sized != 0 || io::Error::last_os_error().raw_os_error() != Some(ERROR_INSUFFICIENT_BUFFER) {
        return Err(io::Error::other("Unable to size scanner interface path"));
    }
    if !(8..=64 * 1024).contains(&required) || required % 2 != 0 {
        return Err(invalid("Invalid scanner interface path size"));
    }
    // u64 storage meets native structure alignment. DevicePath begins at byte 4,
    // while cbSize is 8 on Windows x64 and 6 on Windows x86 (SDK packing).
    let mut detail = vec![0u64; (required as usize).div_ceil(8)];
    let detail_size: u32 = if cfg!(target_pointer_width = "64") {
        8
    } else {
        6
    };
    // SAFETY: allocation is aligned and has at least 8 writable bytes.
    unsafe {
        detail.as_mut_ptr().cast::<u32>().write(detail_size);
    }
    let capacity = required;
    win_result(
        // SAFETY: detail has at least capacity bytes, valid header, and live outputs.
        unsafe {
            SetupDiGetDeviceInterfaceDetailW(
                set.0,
                interface,
                detail.as_mut_ptr().cast(),
                capacity,
                &mut required,
                &mut device,
            )
        },
        "Read scanner interface path",
    )?;
    if required > capacity || required < 8 || required % 2 != 0 {
        return Err(invalid("Scanner interface path length changed"));
    }

    // MAX_DEVICE_ID_LEN is 200 UTF-16 units including terminator (cfgmgr32.h).
    let mut id = [0u16; 200];
    let mut id_length = 0;
    win_result(
        // SAFETY: device is from this set; id_length and all 200 units are writable.
        unsafe {
            SetupDiGetDeviceInstanceIdW(
                set.0,
                &device,
                id.as_mut_ptr(),
                id.len() as u32,
                &mut id_length,
            )
        },
        "Read scanner identity",
    )?;
    if id_length == 0 || id_length as usize > id.len() || id[id_length as usize - 1] != 0 {
        return Err(invalid("Invalid scanner identity length"));
    }
    let identity = String::from_utf16(&id[..id_length as usize - 1])
        .map_err(|_| invalid("Invalid scanner identity encoding"))?;

    // SAFETY: DevicePath is at byte 4, u16 aligned, and within returned allocation.
    let path = unsafe {
        std::slice::from_raw_parts(
            detail.as_ptr().cast::<u8>().add(4).cast::<u16>(),
            (required as usize - 4) / 2,
        )
    };
    let end = path
        .iter()
        .position(|v| *v == 0)
        .ok_or_else(|| invalid("Unterminated scanner path"))?;
    if end == 0 {
        return Err(invalid("Empty scanner path"));
    }
    Ok((path[..=end].to_vec(), identity))
}

struct ScannerCandidates {
    active: Vec<Vec<u16>>,
    inactive: usize,
}

fn unique_scanner_path(candidates: &ScannerCandidates) -> io::Result<&[u16]> {
    if candidates.active.len().saturating_add(candidates.inactive) > 1 {
        return Err(io::Error::other(
            "Multiple scanner interfaces are present; refusing to choose a device",
        ));
    }
    if candidates.active.len() != 1 {
        if candidates.active.is_empty() {
            return if candidates.inactive > 0 {
                Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "Scanner interface is not active",
                ))
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotFound,
                    "Scanner WinUSB interface is unavailable; MI_00 must be paired and its interface GUID registered before inquiry",
                ))
            };
        }
        return Err(io::Error::other(
            "Multiple scanner interfaces are present; refusing to choose a device",
        ));
    }
    Ok(&candidates.active[0])
}

fn scanner_candidate_paths() -> io::Result<ScannerCandidates> {
    // DIGCF_PRESENT | DIGCF_DEVICEINTERFACE. No enumerator or parent window.
    // SAFETY: GUID is live; null optional arguments are permitted.
    let handle = unsafe { SetupDiGetClassDevsW(&SCANNER_GUID, ptr::null(), ptr::null_mut(), 0x12) };
    if handle == INVALID_HANDLE {
        return Err(io::Error::last_os_error());
    }
    let set = DeviceSet(handle);
    let mut active = Vec::new();
    let mut inactive = 0usize;
    let mut index = 0u32;
    loop {
        let mut interface = SetupData::new();
        // SAFETY: set is live and interface has the required writable ABI and cbSize.
        let has_item = unsafe {
            SetupDiEnumDeviceInterfaces(set.0, ptr::null(), &SCANNER_GUID, index, &mut interface)
        };
        if has_item == 0 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(ERROR_NO_MORE_ITEMS) {
                break;
            }
            return Err(error);
        }
        // Bound enumeration without treating a truncated candidate list as complete.
        if index == 256 {
            return Err(invalid("Too many registered scanner interfaces"));
        }
        index += 1;
        let is_active = interface.value & 1 != 0 && interface.value & 4 == 0;
        let (path, identity) = interface_detail_with_identity(&set, &interface)?;
        validate_scanner_id(&identity)?;
        if is_active {
            active.push(path);
        } else {
            inactive += 1;
        }
    }
    Ok(ScannerCandidates { active, inactive })
}

fn validate_matching_path(path: &[u16]) -> io::Result<&[u16]> {
    if path.is_empty() {
        return Err(invalid("Scanner path is empty"));
    }
    if path.len() > 32_768 {
        return Err(invalid("Scanner path is too long"));
    }
    let end = path
        .iter()
        .position(|v| *v == 0)
        .ok_or_else(|| invalid("Scanner path is not null-terminated"))?;
    if end == 0 {
        return Err(invalid("Scanner path is empty"));
    }
    if end + 1 != path.len() {
        return Err(invalid("Scanner path has data after its terminator"));
    }
    Ok(&path[..=end])
}

fn equal_utf16_path(a: &[u16], b: &[u16]) -> io::Result<bool> {
    let count_a =
        i32::try_from(a.len()).map_err(|_| invalid("Scanner path length is unsupported"))?;
    let count_b =
        i32::try_from(b.len()).map_err(|_| invalid("Scanner path length is unsupported"))?;
    // SAFETY: pointers are valid for their lengths; CompareStringOrdinal is UTF-16 aware.
    let result = unsafe { CompareStringOrdinal(a.as_ptr(), count_a, b.as_ptr(), count_b, 1) };
    match result {
        2 => Ok(true),
        1 | 3 => Ok(false),
        0 => Err(io::Error::last_os_error()),
        _ => Err(io::Error::other("Unexpected path compare result")),
    }
}

fn select_matching_scanner_path<'a>(
    candidates: &'a [Vec<u16>],
    requested: &[u16],
) -> io::Result<&'a [u16]> {
    let requested = validate_matching_path(requested)?;
    let mut found = None;
    for candidate in candidates {
        let matches = equal_utf16_path(candidate, requested)?;
        if matches {
            if found.is_some() {
                return Err(io::Error::other(
                    "Multiple matching MI_00 scanner interfaces are present; refusing to choose a device",
                ));
            }
            found = Some(candidate.as_slice());
        }
    }
    found.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "No matching MI_00 scanner interface is connected",
        )
    })
}

fn open_with_path(path: &[u16]) -> io::Result<UsbSession> {
    // GENERIC_READ | GENERIC_WRITE, exclusive sharing, OPEN_EXISTING,
    // FILE_FLAG_OVERLAPPED (required by WinUSB even for synchronous transfers).
    // SAFETY: path is owned and terminated; optional pointers are null.
    let raw_file = unsafe {
        CreateFileW(
            path.as_ptr(),
            0xc000_0000,
            0,
            ptr::null(),
            3,
            0x4000_0000,
            ptr::null_mut(),
        )
    };
    if raw_file == INVALID_HANDLE {
        return Err(io::Error::last_os_error());
    }
    let file = DeviceFile(raw_file);
    let mut raw_usb = ptr::null_mut();
    win_result(
        // SAFETY: file is valid and remains alive longer than the resulting handle.
        unsafe { WinUsb_Initialize(file.0, &mut raw_usb) },
        "Initialize scanner WinUSB",
    )?;
    if raw_usb.is_null() {
        return Err(invalid("WinUSB returned an empty interface handle"));
    }
    let usb = UsbHandle(raw_usb);

    let mut descriptor = [0u8; 18];
    let mut transferred = 0;
    win_result(
        // SAFETY: owned USB handle, valid descriptor selector and 18-byte output.
        unsafe {
            WinUsb_GetDescriptor(
                usb.0,
                1,
                0,
                0,
                descriptor.as_mut_ptr(),
                descriptor.len() as u32,
                &mut transferred,
            )
        },
        "Read USB device descriptor",
    )?;
    if transferred != descriptor.len() as u32 {
        return Err(invalid("Truncated USB device descriptor"));
    }
    validate_device(&descriptor)?;

    let mut setting = 0;
    win_result(
        // SAFETY: live USB handle and writable one-byte output.
        unsafe { WinUsb_GetCurrentAlternateSetting(usb.0, &mut setting) },
        "Read current USB alternate setting",
    )?;
    let mut interface = [0u8; 9];
    win_result(
        // SAFETY: USB_INTERFACE_DESCRIPTOR is packed, exactly 9 bytes, alignment 1.
        unsafe { WinUsb_QueryInterfaceSettings(usb.0, setting, interface.as_mut_ptr()) },
        "Read scanner USB interface",
    )?;
    let count = validate_interface(&interface, setting)?;
    let mut pipes = Vec::with_capacity(count as usize);
    for index in 0..count {
        let mut pipe = PipeInfo::default();
        win_result(
            // SAFETY: index is bounded by descriptor endpoint count; pipe ABI matches SDK.
            unsafe { WinUsb_QueryPipe(usb.0, setting, index, &mut pipe) },
            "Read scanner endpoint",
        )?;
        pipes.push(pipe);
    }
    let selected = select_bulk_pipes(&pipes)?;
    for pipe in [selected.input, selected.output] {
        let timeout: u32 = 5000;
        win_result(
            // SAFETY: discovered pipe; PIPE_TRANSFER_TIMEOUT consumes a live ULONG.
            unsafe { WinUsb_SetPipePolicy(usb.0, pipe, 3, 4, (&timeout as *const u32).cast()) },
            "Set USB transfer timeout",
        )?;
        let disabled: u8 = 0;
        // AUTO_CLEAR_STALL and RESET_PIPE_ON_RESUME must not reset hardware.
        for policy in [2, 9] {
            win_result(
                // SAFETY: both policies consume a BOOLEAN (one byte), not Win32 BOOL.
                unsafe {
                    WinUsb_SetPipePolicy(usb.0, pipe, policy, 1, (&disabled as *const u8).cast())
                },
                "Disable automatic USB recovery",
            )?;
        }
    }
    // Do not discard excess data, accept partial packets, or ignore short packets.
    let disabled: u8 = 0;
    for policy in [4, 5, 6] {
        win_result(
            // SAFETY: read-pipe policies each consume a live BOOLEAN.
            unsafe {
                WinUsb_SetPipePolicy(
                    usb.0,
                    selected.input,
                    policy,
                    1,
                    (&disabled as *const u8).cast(),
                )
            },
            "Set bounded USB read policy",
        )?;
    }

    Ok(UsbSession {
        usb,
        file,
        descriptor,
        interface,
        bulk_in: selected.input,
        bulk_out: selected.output,
        bulk_in_max_packet: selected.input_max_packet,
        bulk_out_max_packet: selected.output_max_packet,
    })
}

fn validate_device(descriptor: &[u8]) -> io::Result<()> {
    if descriptor.len() != 18
        || descriptor[0] != 18
        || descriptor[1] != 1
        || u16::from_le_bytes([descriptor[8], descriptor[9]]) != 0x0924
        || u16::from_le_bytes([descriptor[10], descriptor[11]]) != 0x4265
    {
        return Err(invalid(
            "USB device descriptor does not match WorkCentre 3119",
        ));
    }
    Ok(())
}

fn validate_interface(descriptor: &[u8; 9], current: u8) -> io::Result<u8> {
    if descriptor[0] != 9
        || descriptor[1] != 4
        || descriptor[2] != 0
        || descriptor[3] != current
        || descriptor[5] != 0xff
        || !(2..=30).contains(&descriptor[4])
    {
        return Err(invalid(
            "USB interface is not a valid vendor-specific scanner MI_00",
        ));
    }
    Ok(descriptor[4])
}

fn select_bulk_pipes(pipes: &[PipeInfo]) -> io::Result<BulkPipes> {
    let (mut input, mut output) = (None, None);
    let mut addresses = [false; 256];
    for pipe in pipes {
        if pipe.id & 0x70 != 0
            || pipe.id & 0x0f == 0
            || addresses[pipe.id as usize]
            || pipe.maximum_packet == 0
            || pipe.maximum_packet > 1024
        {
            return Err(invalid("Invalid or duplicate USB endpoint descriptor"));
        }
        addresses[pipe.id as usize] = true;
        if pipe.kind != 2 {
            continue;
        }
        if ![8, 16, 32, 64, 512, 1024].contains(&pipe.maximum_packet) {
            return Err(invalid("Invalid bulk maximum packet size"));
        }
        let slot = if pipe.id & 0x80 != 0 {
            &mut input
        } else {
            &mut output
        };
        if slot.replace((pipe.id, pipe.maximum_packet)).is_some() {
            return Err(invalid("Ambiguous bulk endpoints; no command was sent"));
        }
    }
    let (input, output) = input
        .zip(output)
        .ok_or_else(|| invalid("Scanner requires a unique bulk IN and OUT endpoint"))?;
    Ok(BulkPipes {
        input: input.0,
        output: output.0,
        input_max_packet: input.1,
        output_max_packet: output.1,
    })
}

#[cfg(test)]
fn select_pipes(pipes: &[PipeInfo]) -> io::Result<(u8, u8)> {
    let selected = select_bulk_pipes(pipes)?;
    Ok((selected.input, selected.output))
}

#[cfg(test)]
fn validate_write(transferred: u32) -> io::Result<()> {
    if transferred != 4 {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "Incomplete INQUIRY write; command will not be retried",
        ));
    }
    Ok(())
}

fn validate_transfer_buffer(length: usize) -> io::Result<()> {
    if length == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "USB transfer buffer must not be empty",
        ));
    }
    if length > MAX_TRANSFER_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "USB transfer buffer exceeds the 1 MiB limit",
        ));
    }
    Ok(())
}

fn validate_read_buffer(length: usize, maximum_packet: u16) -> io::Result<()> {
    validate_transfer_buffer(length)?;
    let maximum_packet = usize::from(maximum_packet);
    if maximum_packet == 0 || !length.is_multiple_of(maximum_packet) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "USB read buffer must be a multiple of the bulk IN maximum packet",
        ));
    }
    Ok(())
}

fn validate_maximum_read_transfer_size(
    value_length: u32,
    value: u32,
    maximum_packet: u16,
) -> io::Result<usize> {
    if value_length != size_of::<u32>() as u32 {
        return Err(invalid("Invalid MAXIMUM_TRANSFER_SIZE policy length"));
    }
    if ![8, 16, 32, 64, 512, 1024].contains(&maximum_packet) {
        return Err(invalid("Invalid bulk IN maximum packet size"));
    }
    let value = u64::from(value);
    let maximum_packet = u64::from(maximum_packet);
    if maximum_packet == 0 || value == 0 || value < maximum_packet {
        return Err(invalid("Invalid MAXIMUM_TRANSFER_SIZE policy value"));
    }
    // Windows targets supported by this crate have usize wide enough for ULONG.
    Ok(value as usize)
}

fn validate_complete_write(transferred: u32, expected: usize) -> io::Result<()> {
    if expected == 0 || expected > MAX_TRANSFER_BYTES || u64::from(transferred) != expected as u64 {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "Incomplete USB write; command will not be retried",
        ));
    }
    Ok(())
}

fn validate_bounded_read(transferred: u32, capacity: usize) -> io::Result<usize> {
    if u64::from(transferred) > capacity as u64 {
        return Err(invalid("Oversized USB response"));
    }
    Ok(transferred as usize)
}

fn validate_read(transferred: u32, capacity: usize) -> io::Result<usize> {
    if transferred == 0 || u64::from(transferred) > capacity as u64 {
        return Err(invalid("Empty or oversized INQUIRY response"));
    }
    Ok(transferred as usize)
}

impl UsbSession {
    /// Open and validate the one present scanner MI_00 transport.
    ///
    /// This performs descriptor and pipe discovery plus the existing bounded
    /// WinUSB policy setup. It does not send a device command.
    pub(crate) fn open() -> io::Result<Self> {
        let candidates = scanner_candidate_paths()?;
        open_with_path(unique_scanner_path(&candidates)?)
    }

    /// Open and validate scanner MI_00 transport by exact device interface path.
    /// Path input must be a single NUL-terminated UTF-16 string.
    pub(crate) fn open_matching_path(path: &[u16]) -> io::Result<Self> {
        validate_matching_path(path)?;
        let candidates = scanner_candidate_paths()?;
        let matched = select_matching_scanner_path(&candidates.active, path)?;
        open_with_path(matched)
    }

    /// Query WinUSB's current maximum transfer size for the bulk IN pipe.
    ///
    /// WinUSB documents `MAXIMUM_TRANSFER_SIZE` (policy `0x08`) as a read-only
    /// policy returned as a `ULONG`. The value is returned without the session's
    /// application buffer cap; callers must apply their own finite buffer limit.
    pub(crate) fn maximum_read_transfer_size(&self) -> io::Result<usize> {
        let mut value: u32 = 0;
        let mut value_length = size_of::<u32>() as u32;
        win_result(
            // SAFETY: the session owns a live WinUSB handle and bulk IN pipe;
            // both output pointers refer to writable ULONG-sized storage.
            unsafe {
                WinUsb_GetPipePolicy(
                    self.usb.0,
                    self.bulk_in,
                    MAXIMUM_TRANSFER_SIZE_POLICY,
                    &mut value_length,
                    (&mut value as *mut u32).cast(),
                )
            },
            "Read WinUSB maximum read transfer size",
        )?;
        validate_maximum_read_transfer_size(value_length, value, self.bulk_in_max_packet)
    }

    /// Write one complete command without retrying a short transfer.
    pub(crate) fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        validate_transfer_buffer(bytes.len())?;
        let mut transferred = 0;
        win_result(
            // SAFETY: the session owns a live WinUSB handle; the input slice remains
            // valid for the synchronous call and the API writes no more than its length.
            unsafe {
                WinUsb_WritePipe(
                    self.usb.0,
                    self.bulk_out,
                    bytes.as_ptr().cast_mut(),
                    bytes.len() as u32,
                    &mut transferred,
                    ptr::null_mut(),
                )
            },
            "Write scanner command",
        )?;
        validate_complete_write(transferred, bytes.len())
    }

    /// Read one bounded bulk-IN transfer. A zero-byte completion is returned to
    /// the caller so scan-state logic can decide whether it is acceptable.
    pub(crate) fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        validate_read_buffer(bytes.len(), self.bulk_in_max_packet)?;
        let mut transferred = 0;
        win_result(
            // SAFETY: the session owns a live WinUSB handle; the output slice remains
            // valid and writable for the synchronous call and its length is bounded.
            unsafe {
                WinUsb_ReadPipe(
                    self.usb.0,
                    self.bulk_in,
                    bytes.as_mut_ptr(),
                    bytes.len() as u32,
                    &mut transferred,
                    ptr::null_mut(),
                )
            },
            "Read scanner data",
        )?;
        validate_bounded_read(transferred, bytes.len())
    }
}

/// Query capabilities once. Requires an already-installed WinUSB binding for
/// scanner MI_00. Does not scan, install, reset, clear stalls, or retry commands.
/// The returned bytes still require protocol validation by the caller.
pub fn inquiry() -> io::Result<Vec<u8>> {
    let mut session = UsbSession::open()?;
    // Protocol facts: SANE 1.4.0 xerox_mfp.h CMD_INQUIRY / REQ_CODE_A/B,
    // xerox_mfp.c inquiry command has no payload. Original Rust implementation.
    let command = [0x1b, 0xa8, 0x12, 0];
    session.write(&command)?;
    // 1024 is a multiple of the protocol's 512-byte USB block and of any valid
    // bulk maximum packet size. One read only; parser rejects unrelated replies.
    let mut response = vec![0u8; 1024];
    let transferred = session.read(&mut response)?;
    response.truncate(validate_read(transferred as u32, response.len())?);
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    // All descriptors and lengths below are synthetic, not hardware captures.
    #[test]
    fn rejects_wrong_or_truncated_device_before_commands() {
        let mut descriptor = [
            18, 1, 0, 2, 0, 0, 0, 64, 0x24, 9, 0x65, 0x42, 0, 1, 0, 0, 0, 1,
        ];
        assert!(validate_device(&descriptor).is_ok());
        assert!(validate_device(&descriptor[..17]).is_err());
        descriptor[8] = 0;
        assert!(validate_device(&descriptor).is_err());
        descriptor[8] = 0x24;
        descriptor[1] = 2;
        assert!(validate_device(&descriptor).is_err());
    }

    #[test]
    fn refuses_printer_and_invalid_scanner_interface() {
        let mut descriptor = [9, 4, 0, 0, 2, 0xff, 0, 0, 0];
        assert_eq!(validate_interface(&descriptor, 0).unwrap(), 2);
        descriptor[2] = 1;
        assert!(validate_interface(&descriptor, 0).is_err());
        descriptor[2] = 0;
        descriptor[5] = 7;
        assert!(validate_interface(&descriptor, 0).is_err());
        descriptor[5] = 0xff;
        assert!(validate_interface(&descriptor, 1).is_err());
    }

    fn bulk(id: u8) -> PipeInfo {
        PipeInfo {
            kind: 2,
            id,
            maximum_packet: 64,
            interval: 0,
        }
    }

    #[test]
    fn requires_unique_valid_bulk_directions() {
        let input = bulk(0x83);
        let output = bulk(0x05);
        assert_eq!(select_pipes(&[input, output]).unwrap(), (0x83, 0x05));
        assert!(select_pipes(&[input]).is_err());
        assert!(select_pipes(&[input, output, bulk(0x84)]).is_err());
        assert!(select_pipes(&[input, output, output]).is_err());
        assert!(select_pipes(&[input, bulk(0)]).is_err());
        assert!(select_pipes(&[input, bulk(0x75)]).is_err());
        assert!(
            select_pipes(&[
                input,
                PipeInfo {
                    maximum_packet: 0,
                    ..output
                }
            ])
            .is_err()
        );
        assert!(
            select_pipes(&[
                input,
                PipeInfo {
                    maximum_packet: 1000,
                    ..output
                }
            ])
            .is_err()
        );
        assert!(select_pipes(&[input, PipeInfo { kind: 3, ..output }]).is_err());
    }

    #[test]
    fn rejects_partial_writes_empty_and_oversized_reads() {
        assert!(validate_write(4).is_ok());
        for transferred in [0, 1, 3, 5, u32::MAX] {
            assert!(validate_write(transferred).is_err());
        }
        assert_eq!(validate_read(70, 512).unwrap(), 70);
        assert_eq!(validate_read(512, 512).unwrap(), 512);
        assert!(validate_read(0, 512).is_err());
        assert!(validate_read(513, 512).is_err());
    }

    #[test]
    fn session_transfer_boundaries_are_finite_and_variable_without_panics() {
        for length in [1, 4, 64, 512, MAX_TRANSFER_BYTES] {
            assert!(validate_transfer_buffer(length).is_ok());
            assert!(validate_complete_write(length as u32, length).is_ok());
            assert!(validate_complete_write(length as u32 - 1, length).is_err());
        }
        for length in [0, MAX_TRANSFER_BYTES + 1, usize::MAX] {
            let result = std::panic::catch_unwind(|| validate_transfer_buffer(length));
            assert!(result.is_ok(), "buffer validation panicked for {length}");
            assert!(result.unwrap().is_err());
        }
    }

    #[test]
    fn session_read_allows_zero_transfer_but_rejects_bad_buffers() {
        assert_eq!(validate_bounded_read(0, 512).unwrap(), 0);
        assert_eq!(validate_bounded_read(511, 512).unwrap(), 511);
        assert_eq!(validate_bounded_read(512, 512).unwrap(), 512);
        assert!(validate_bounded_read(513, 512).is_err());
        assert!(validate_read_buffer(512, 64).is_ok());
        assert!(validate_read_buffer(1024, 1024).is_ok());
        for (length, max_packet) in [(0, 64), (1, 64), (513, 64), (512, 1024)] {
            assert!(validate_read_buffer(length, max_packet).is_err());
        }
        assert!(validate_read_buffer(MAX_TRANSFER_BYTES + 1, 64).is_err());
    }

    #[test]
    fn validates_maximum_transfer_policy_shape_and_packet_size() {
        let ulong_bytes = size_of::<u32>() as u32;
        assert_eq!(
            validate_maximum_read_transfer_size(ulong_bytes, 64, 64).unwrap(),
            64
        );
        assert_eq!(
            validate_maximum_read_transfer_size(ulong_bytes, 1024, 512).unwrap(),
            1024
        );
        assert_eq!(
            validate_maximum_read_transfer_size(ulong_bytes, 65_535, 512).unwrap(),
            65_535
        );
        // Do not impose the session's 1 MiB application buffer cap on the OS
        // policy value. The caller still chooses a separately bounded buffer.
        assert_eq!(
            validate_maximum_read_transfer_size(ulong_bytes, 0xffff_ffc0, 64).unwrap(),
            0xffff_ffc0
        );
        for (length, value, packet) in [
            (0, 64, 64),
            (ulong_bytes - 1, 64, 64),
            (ulong_bytes + 1, 64, 64),
            (ulong_bytes, 0, 64),
            (ulong_bytes, 63, 64),
            (ulong_bytes, 512, 0),
            (ulong_bytes, 512, 3),
        ] {
            assert!(
                validate_maximum_read_transfer_size(length, value, packet).is_err(),
                "accepted malformed MAXIMUM_TRANSFER_SIZE length={length} value={value} packet={packet}"
            );
        }
    }

    #[test]
    fn validates_registered_identity_as_complete_fields() {
        assert!(validate_scanner_id("USB\\VID_0924&PID_4265&MI_00\\synthetic").is_ok());
        for id in [
            "USB\\VID_0924&PID_4265&MI_01\\synthetic",
            "USB\\VID_0924&PID_4265\\synthetic",
            "USB\\VID_0924&PID_4265&MI_00_SUFFIX\\synthetic",
            "OTHER\\VID_0924&PID_4265&MI_00\\synthetic",
            "USB\\VID_0924&PID_4265&MI_00\\",
            "USB\\VID_0924&PID_4265&MI_00\\synthetic\\extra",
        ] {
            assert!(validate_scanner_id(id).is_err());
        }
    }

    fn u16z(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    #[test]
    fn unspecified_target_rejects_active_plus_inactive_candidates() {
        let candidates = ScannerCandidates {
            active: vec![u16z(r"\\?\usb#synthetic-a")],
            inactive: 1,
        };
        assert!(
            unique_scanner_path(&candidates).is_err(),
            "unspecified target must reject every ambiguous enumeration"
        );
    }

    #[test]
    fn selects_matching_scanner_path_and_rejects_non_matching_inputs() {
        let candidates = vec![
            u16z(r"\\?\usb#vid_0924&pid_4265&mi_00#a#{synthetic-guid}"),
            u16z(r"\\?\usb#vid_0924&pid_4265&mi_00#b#{synthetic-guid}"),
            u16z(r"\\?\usb#vid_0924&pid_4265&mi_00#c#{synthetic-guid}"),
        ];
        let input_a = u16z(r"\\?\USB#VID_0924&PID_4265&MI_00#B#{SYNTHETIC-GUID}");
        assert_eq!(
            select_matching_scanner_path(&candidates, &input_a).unwrap(),
            candidates[1].as_slice()
        );

        let missing = u16z(r"\\?\usb#vid_0924&pid_4265&mi_00#d#{synthetic-guid}");
        assert!(select_matching_scanner_path(&candidates, &missing).is_err());
        let missing = u16z(r"\\?\usb#vid_0924&pid_4265&mi_01#a#{synthetic-guid}");
        assert!(select_matching_scanner_path(&candidates, &missing).is_err());
        assert!(
            validate_matching_path(&[] as &[u16]).is_err(),
            "empty scanner path is invalid"
        );
        let unterminated = vec![b'X' as u16, b'Y' as u16];
        assert!(validate_matching_path(&unterminated).is_err());
        let mut double_null = candidates[0].clone();
        double_null.push(0);
        assert!(validate_matching_path(&double_null).is_err());
        let mut oversized = vec![b'x' as u16; 32_768];
        oversized.push(0);
        assert!(validate_matching_path(&oversized).is_err());
        assert!(select_matching_scanner_path(&candidates, &u16z(r"C:\unrelated-file")).is_err());
        assert!(equal_utf16_path(&u16z("é-device"), &u16z("É-DEVICE")).unwrap());
    }

    #[test]
    fn open_matching_path_prefers_unique_match_only() {
        let candidates = vec![
            u16z(r"\\?\usb#vid_0924&pid_4265&mi_00#a#{synthetic-guid}"),
            u16z(r"\\?\usb#vid_0924&pid_4265&mi_00#b#{synthetic-guid}"),
        ];
        let requested = candidates[0].clone();
        assert_eq!(
            select_matching_scanner_path(&candidates, &requested).unwrap(),
            candidates[0].as_slice()
        );
        let duplicate = vec![requested.clone(), requested];
        assert!(select_matching_scanner_path(&duplicate, &duplicate[0]).is_err());
    }
}
