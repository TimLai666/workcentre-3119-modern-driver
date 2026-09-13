//! WinUSB setup helper. No arguments is read-only candidate enumeration.
//! Installation requires --install-mi00 and the complete expected instance ID.
//! Installation persists the target binding through Windows. No direct registry
//! editing, package-wide operations, selected-driver API, or reboot is used.

use std::io;

#[derive(Debug, PartialEq, Eq)]
enum Action {
    Precheck,
    Install(String),
}

fn parse_args(args: &[std::ffi::OsString]) -> io::Result<Action> {
    match args {
        [] => Ok(Action::Precheck),
        [flag, expected] if flag == "--install-mi00" => {
            let expected = expected
                .to_str()
                .filter(|id| target_id(id))
                .ok_or_else(|| io::Error::other("Expected a complete exact MI_00 instance ID"))?;
            Ok(Action::Install(expected.into()))
        }
        _ => Err(io::Error::other(
            "Use no arguments for precheck, or --install-mi00 with exactly one expected instance ID",
        )),
    }
}

#[cfg(any(windows, test))]
fn same_target(expected: &str, actual: &str) -> bool {
    target_id(expected) && target_id(actual) && expected.eq_ignore_ascii_case(actual)
}

#[cfg(any(windows, test))]
struct Candidate {
    inf: String,
    section: String,
    provider: String,
    hardware_id: String,
}

#[cfg(any(windows, test))]
fn read_z(raw: &[u16]) -> io::Result<String> {
    let end = raw
        .iter()
        .position(|v| *v == 0)
        .ok_or_else(|| io::Error::other("Unterminated UTF-16 property"))?;
    let value =
        String::from_utf16(&raw[..end]).map_err(|_| io::Error::other("Invalid UTF-16 property"))?;
    if value.chars().any(char::is_control) {
        return Err(io::Error::other("Control character in property"));
    }
    Ok(value)
}

fn target_id(id: &str) -> bool {
    workcentre_3119::identify(id) == Some(workcentre_3119::Interface::Scanner)
}

#[cfg(any(windows, test))]
fn precheck_result(device: io::Result<bool>, global: io::Result<bool>) -> io::Result<i32> {
    let device = device?;
    let global = global?;
    if device && global {
        Ok(0)
    } else {
        Err(io::Error::other(
            "Precheck requires one exact WinUSB candidate in each list",
        ))
    }
}

#[cfg(any(windows, test))]
fn exact_candidate(candidate: &Candidate) -> bool {
    candidate
        .inf
        .eq_ignore_ascii_case(r"C:\Windows\INF\winusb.inf")
        && candidate.section.eq_ignore_ascii_case("WINUSB")
        && candidate.provider.eq_ignore_ascii_case("Microsoft")
        && candidate
            .hardware_id
            .eq_ignore_ascii_case(r"USB\MS_COMP_WINUSB")
}

fn main() {
    let result = parse_args(&std::env::args_os().skip(1).collect::<Vec<_>>()).and_then(execute);
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("Setup helper failed: {error}");
            1
        }
    };
    // execute has returned and all owned device sets have been dropped. Windows
    // requires an integer process exit code to preserve ERROR_SUCCESS_REBOOT_REQUIRED.
    std::process::exit(code);
}

#[cfg(all(windows, target_pointer_width = "64"))]
fn execute(action: Action) -> io::Result<i32> {
    windows::run(action)
}

#[cfg(not(all(windows, target_pointer_width = "64")))]
fn execute(_action: Action) -> io::Result<i32> {
    Err(io::Error::other(
        "This setup helper requires 64-bit Windows",
    ))
}

#[cfg(all(windows, target_pointer_width = "64"))]
mod windows {
    use super::{
        Action, Candidate, exact_candidate, precheck_result, read_z, same_target, target_id,
    };
    use std::{
        ffi::c_void,
        io,
        mem::{offset_of, size_of},
        ptr,
    };

    // Windows SDK 10.0.26100.0 setupapi.h: x64 pack(8), lines 749,
    // 919, 1467 and 1548. DiInstallDevice: newdev.h lines 79-86.
    type Handle = *mut c_void;
    const CLASS_DRIVER: u32 = 1;
    const NO_MORE_ITEMS: i32 = 259;
    const INSUFFICIENT_BUFFER: i32 = 122;

    #[repr(C)]
    #[derive(Default)]
    struct Guid {
        a: u32,
        b: u16,
        c: u16,
        d: [u8; 8],
    }
    #[repr(C)]
    struct PropertyKey {
        format: Guid,
        id: u32,
    }
    #[repr(C)]
    #[derive(Default)]
    struct DeviceInfo {
        size: u32,
        class: Guid,
        node: u32,
        reserved: usize,
    }
    #[repr(C)]
    struct InstallParams {
        size: u32,
        flags: u32,
        flags_ex: u32,
        parent: Handle,
        callback: Handle,
        callback_context: Handle,
        queue: Handle,
        class_reserved: usize,
        reserved: u32,
        path: [u16; 260],
    }
    #[repr(C)]
    struct DriverInfo {
        size: u32,
        driver_type: u32,
        reserved: usize,
        description: [u16; 256],
        manufacturer: [u16; 256],
        provider: [u16; 256],
        date: [u32; 2],
        version: u64,
    }
    #[repr(C)]
    struct DriverDetail {
        size: u32,
        date: [u32; 2],
        compatible_offset: u32,
        compatible_length: u32,
        reserved: usize,
        section: [u16; 256],
        inf: [u16; 260],
        description: [u16; 256],
        hardware: [u16; 1],
    }

    #[link(name = "Setupapi")]
    unsafe extern "system" {
        fn SetupDiGetClassDevsW(
            class: *const Guid,
            enumerator: *const u16,
            parent: Handle,
            flags: u32,
        ) -> Handle;
        fn SetupDiCreateDeviceInfoList(class: *const Guid, parent: Handle) -> Handle;
        fn SetupDiDestroyDeviceInfoList(set: Handle) -> i32;
        fn SetupDiEnumDeviceInfo(set: Handle, index: u32, device: *mut DeviceInfo) -> i32;
        fn SetupDiGetDeviceInstanceIdW(
            set: Handle,
            device: *const DeviceInfo,
            buffer: *mut u16,
            length: u32,
            required: *mut u32,
        ) -> i32;
        fn SetupDiGetDeviceRegistryPropertyW(
            set: Handle,
            device: *const DeviceInfo,
            property: u32,
            kind: *mut u32,
            buffer: *mut u8,
            length: u32,
            required: *mut u32,
        ) -> i32;
        fn SetupDiGetDevicePropertyW(
            set: Handle,
            device: *const DeviceInfo,
            key: *const PropertyKey,
            kind: *mut u32,
            buffer: *mut u8,
            length: u32,
            required: *mut u32,
            flags: u32,
        ) -> i32;
        fn SetupDiGetDeviceInstallParamsW(
            set: Handle,
            device: *const DeviceInfo,
            params: *mut InstallParams,
        ) -> i32;
        fn SetupDiSetDeviceInstallParamsW(
            set: Handle,
            device: *const DeviceInfo,
            params: *const InstallParams,
        ) -> i32;
        fn SetupDiBuildDriverInfoList(set: Handle, device: *mut DeviceInfo, kind: u32) -> i32;
        fn SetupDiEnumDriverInfoW(
            set: Handle,
            device: *const DeviceInfo,
            kind: u32,
            index: u32,
            driver: *mut DriverInfo,
        ) -> i32;
        fn SetupDiGetDriverInfoDetailW(
            set: Handle,
            device: *const DeviceInfo,
            driver: *const DriverInfo,
            detail: *mut DriverDetail,
            bytes: u32,
            required: *mut u32,
        ) -> i32;
    }
    #[link(name = "Cfgmgr32")]
    unsafe extern "system" {
        fn CM_Get_DevNode_Status(status: *mut u32, problem: *mut u32, node: u32, flags: u32)
        -> u32;
    }
    #[link(name = "Newdev")]
    unsafe extern "system" {
        fn DiInstallDevice(
            parent: Handle,
            set: Handle,
            device: *const DeviceInfo,
            driver: *const DriverInfo,
            flags: u32,
            need_reboot: *mut i32,
        ) -> i32;
    }

    struct DeviceSet(Handle);
    impl DeviceSet {
        fn new(handle: Handle) -> io::Result<Self> {
            if handle.is_null() || handle as isize == -1 {
                Err(io::Error::last_os_error())
            } else {
                Ok(Self(handle))
            }
        }
    }
    impl Drop for DeviceSet {
        fn drop(&mut self) {
            // SAFETY: owns one valid device information set; destruction also frees lists.
            unsafe {
                SetupDiDestroyDeviceInfoList(self.0);
            }
        }
    }
    fn check(result: i32, operation: &str) -> io::Result<()> {
        if result == 0 {
            Err(io::Error::other(format!(
                "{operation}: {}",
                io::Error::last_os_error()
            )))
        } else {
            Ok(())
        }
    }

    fn instance_id(set: &DeviceSet, device: &DeviceInfo) -> io::Result<String> {
        for _ in 0..3 {
            let mut required = 0;
            // SAFETY: valid live set/device; null buffer queries UTF-16 capacity.
            let result = unsafe {
                SetupDiGetDeviceInstanceIdW(set.0, device, ptr::null_mut(), 0, &mut required)
            };
            if result != 0 || io::Error::last_os_error().raw_os_error() != Some(INSUFFICIENT_BUFFER)
            {
                return Err(io::Error::other("Unexpected instance ID size query result"));
            }
            if !(2..=4096).contains(&required) {
                return Err(io::Error::other("Invalid instance ID length"));
            }
            let mut buffer = vec![0u16; required as usize];
            // SAFETY: writable buffer has the stated capacity and valid output length.
            let result = unsafe {
                SetupDiGetDeviceInstanceIdW(
                    set.0,
                    device,
                    buffer.as_mut_ptr(),
                    buffer.len() as u32,
                    &mut required,
                )
            };
            if result == 0 && io::Error::last_os_error().raw_os_error() == Some(INSUFFICIENT_BUFFER)
            {
                continue;
            }
            check(result, "Read private instance ID")?;
            if required as usize > buffer.len() || required == 0 {
                return Err(io::Error::other("Invalid returned instance ID length"));
            }
            return read_z(&buffer[..required as usize]);
        }
        Err(io::Error::other("Instance ID changed repeatedly"))
    }

    fn unique_target(set: &DeviceSet) -> io::Result<DeviceInfo> {
        let mut found = None;
        for index in 0..65536 {
            let mut device = DeviceInfo {
                size: size_of::<DeviceInfo>() as u32,
                ..Default::default()
            };
            // SAFETY: live set and correctly sized writable SP_DEVINFO_DATA.
            let result = unsafe { SetupDiEnumDeviceInfo(set.0, index, &mut device) };
            if result == 0 && io::Error::last_os_error().raw_os_error() == Some(NO_MORE_ITEMS) {
                return found.ok_or_else(|| io::Error::other("No present exact MI_00 target"));
            }
            check(result, "Enumerate present USB devices")?;
            if target_id(&instance_id(set, &device)?) && found.replace(device).is_some() {
                return Err(io::Error::other(
                    "Multiple exact MI_00 targets; refusing selection",
                ));
            }
        }
        Err(io::Error::other("Device enumeration limit exceeded"))
    }

    fn missing_property(set: &DeviceSet, device: &DeviceInfo, property: u32) -> io::Result<bool> {
        let (mut kind, mut required) = (0, 0);
        let mut buffer = [0u16; 1024];
        // SAFETY: aligned buffer has 2048 writable bytes and output pointers are valid.
        let result = unsafe {
            SetupDiGetDeviceRegistryPropertyW(
                set.0,
                device,
                property,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                size_of_val(&buffer) as u32,
                &mut required,
            )
        };
        if result == 0 && io::Error::last_os_error().raw_os_error() == Some(13) {
            // ERROR_INVALID_DATA is the documented result for an absent property.
            return Ok(true);
        }
        check(result, "Read existing driver binding")?;
        if kind != 1 || required > size_of_val(&buffer) as u32 || required % 2 != 0 || required < 2
        {
            return Err(io::Error::other("Invalid existing binding property"));
        }
        Ok(read_z(&buffer[..required as usize / 2])?.is_empty())
    }

    fn verify_unbound(set: &DeviceSet, device: &DeviceInfo) -> io::Result<()> {
        if !target_id(&instance_id(set, device)?) {
            return Err(io::Error::other("Target identity changed"));
        }
        let (mut status, mut problem) = (0, 0);
        // SAFETY: writable status outputs and DEVINST returned by SetupAPI.
        let code = unsafe { CM_Get_DevNode_Status(&mut status, &mut problem, device.node, 0) };
        if code != 0 {
            return Err(io::Error::other(format!(
                "Read target status: CONFIGRET={code:#x}"
            )));
        }
        if status & 0x400 == 0
            || status & 8 != 0
            || problem != 28
            || !missing_property(set, device, 4)?
            || !missing_property(set, device, 9)?
            || !missing_inf_path(set, device)?
        {
            return Err(io::Error::other(
                "Target no longer has missing service/driver and problem 28",
            ));
        }
        Ok(())
    }

    fn missing_inf_path(set: &DeviceSet, device: &DeviceInfo) -> io::Result<bool> {
        // SDK shared/devpkey.h line 230: DEVPKEY_Device_DriverInfPath.
        let key = PropertyKey {
            format: Guid {
                a: 0xa8b865dd,
                b: 0x2e3d,
                c: 0x4094,
                d: [0xad, 0x97, 0xe5, 0x93, 0xa7, 0x0c, 0x75, 0xd6],
            },
            id: 5,
        };
        let (mut kind, mut required) = (0, 0);
        let mut buffer = [0u16; 1024];
        // SAFETY: known property key, same-set device, aligned bounded output buffer.
        let result = unsafe {
            SetupDiGetDevicePropertyW(
                set.0,
                device,
                &key,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                size_of_val(&buffer) as u32,
                &mut required,
                0,
            )
        };
        if result == 0 && io::Error::last_os_error().raw_os_error() == Some(1168) {
            return Ok(true); // ERROR_NOT_FOUND: requested property is absent.
        }
        check(result, "Read current DriverInfPath")?;
        if kind != 0x12
            || required > size_of_val(&buffer) as u32
            || required < 2
            || required % 2 != 0
        {
            return Err(io::Error::other("Invalid DriverInfPath property"));
        }
        Ok(read_z(&buffer[..required as usize / 2])?.is_empty())
    }

    fn detail(
        set: &DeviceSet,
        device: *const DeviceInfo,
        driver: &DriverInfo,
    ) -> io::Result<Candidate> {
        // usize backing provides the x64 struct's 8-byte alignment. A hard 64 KiB
        // cap covers fixed fields plus IDs; oversized results fail without retry.
        let mut storage = vec![0usize; 65536 / size_of::<usize>()];
        let raw = storage.as_mut_ptr().cast::<DriverDetail>();
        // SAFETY: allocated buffer is aligned and larger than DriverDetail.
        unsafe {
            (*raw).size = size_of::<DriverDetail>() as u32;
        }
        let mut required = 0;
        check(
            // SAFETY: live list node, optional same-set device, 65536 writable bytes.
            unsafe {
                SetupDiGetDriverInfoDetailW(set.0, device, driver, raw, 65536, &mut required)
            },
            "Read candidate details",
        )?;
        let start = offset_of!(DriverDetail, hardware);
        if required as usize > 65536 || (required as usize) < start + 2 || required % 2 != 0 {
            return Err(io::Error::other("Invalid candidate detail size"));
        }
        // SAFETY: fixed fields lie within allocation; API succeeded and required
        // length bounds the flexible UTF-16 tail within the same live allocation.
        let (data, ids) = unsafe {
            (
                &*raw,
                std::slice::from_raw_parts(
                    storage.as_ptr().cast::<u8>().add(start).cast::<u16>(),
                    (required as usize - start) / 2,
                ),
            )
        };
        Ok(Candidate {
            inf: read_z(&data.inf)?,
            section: read_z(&data.section)?,
            provider: read_z(&driver.provider)?,
            hardware_id: read_z(ids)?,
        })
    }

    fn candidates(
        set: &DeviceSet,
        device: Option<&mut DeviceInfo>,
        scope: &str,
    ) -> io::Result<Option<DriverInfo>> {
        let device = device.map_or(ptr::null_mut(), |value| value as *mut DeviceInfo);
        // SAFETY: InstallParams consists entirely of integers, arrays and nullable pointers.
        let mut params: InstallParams = unsafe { std::mem::zeroed() };
        params.size = size_of::<InstallParams>() as u32;
        check(
            // SAFETY: same-set optional device and correctly sized writable parameters.
            unsafe { SetupDiGetDeviceInstallParamsW(set.0, device, &mut params) },
            "Read temporary install parameters",
        )?;
        params.flags |= 0x10000; // DI_ENUMSINGLEINF
        params.flags_ex |= 0x800; // DI_FLAGSEX_ALLOWEXCLUDEDDRVS
        let path: Vec<u16> = r"C:\Windows\INF\winusb.inf".encode_utf16().collect();
        params.path.fill(0);
        params.path[..path.len()].copy_from_slice(&path);
        check(
            // SAFETY: only updates this temporary set's install parameters. This
            // function does not call any install, selected-driver or registry-write API.
            unsafe { SetupDiSetDeviceInstallParamsW(set.0, device, &params) },
            "Set temporary single-INF search",
        )?;
        check(
            // SAFETY: CLASS only, never COMPAT (which may update the device class).
            unsafe { SetupDiBuildDriverInfoList(set.0, device, CLASS_DRIVER) },
            "Build class driver list",
        )?;
        let mut matches = 0;
        let mut selected = None;
        for index in 0..256 {
            // SAFETY: DriverInfo has only integer and array fields; zero is valid.
            let mut driver: DriverInfo = unsafe { std::mem::zeroed() };
            driver.size = size_of::<DriverInfo>() as u32;
            // SAFETY: same live list and correctly sized output driver record.
            let result =
                unsafe { SetupDiEnumDriverInfoW(set.0, device, CLASS_DRIVER, index, &mut driver) };
            if result == 0 && io::Error::last_os_error().raw_os_error() == Some(NO_MORE_ITEMS) {
                println!("{scope}: candidates={index}, exact_winusb={matches}");
                return Ok(if matches == 1 { selected } else { None });
            }
            check(result, "Enumerate class driver candidates")?;
            let item = detail(set, device, &driver)?;
            let exact = exact_candidate(&item);
            matches += u32::from(exact);
            println!(
                "{scope}: INF={:?}, section={:?}, provider={:?}, HWID={:?}, exact={exact}",
                item.inf, item.section, item.provider, item.hardware_id
            );
            if exact {
                selected = Some(driver);
            }
        }
        Err(io::Error::other("Driver candidate limit exceeded"))
    }

    fn present_usb() -> io::Result<DeviceSet> {
        let usb = [85u16, 83, 66, 0];
        // SAFETY: null class, valid USB string, null UI owner; PRESENT | ALLCLASSES.
        DeviceSet::new(unsafe {
            SetupDiGetClassDevsW(ptr::null(), usb.as_ptr(), ptr::null_mut(), 0x6)
        })
    }

    pub fn run(action: Action) -> io::Result<i32> {
        let devices = present_usb()?;
        let mut target = unique_target(&devices)?;
        verify_unbound(&devices, &target)?;
        if let Action::Install(expected) = &action
            && !same_target(expected, &instance_id(&devices, &target)?)
        {
            return Err(io::Error::other(
                "Expected instance does not match actual unique MI_00",
            ));
        }
        println!(
            "Exact MI_00 targets=1; missing service/driver; problem=28; read-only CLASS enumeration."
        );
        let device_result = candidates(&devices, Some(&mut target), "device-associated");
        if let Action::Install(expected) = action {
            let chosen = device_result?.ok_or_else(|| {
                io::Error::other(
                    "No unique exact device-associated WinUSB candidate; installation refused",
                )
            })?;
            // Refresh presence and multiplicity independently of the original set.
            let fresh = present_usb()?;
            let fresh_target = unique_target(&fresh)?;
            if fresh_target.node != target.node
                || !same_target(&expected, &instance_id(&fresh, &fresh_target)?)
            {
                return Err(io::Error::other(
                    "Target identity or presence changed before installation",
                ));
            }
            if !exact_candidate(&detail(&devices, &target, &chosen)?) {
                return Err(io::Error::other(
                    "Device-associated candidate changed before installation",
                ));
            }
            verify_unbound(&devices, &target)?;
            let mut need_reboot = 0;
            // SAFETY: explicit action and full expected ID passed all checks; target
            // and chosen driver belong to this same live associated CLASS list.
            // flags=0 installs only this node. No CopyINF flag, selected-driver API,
            // package-level install, direct registry write or automatic restart is used.
            let result = unsafe {
                DiInstallDevice(
                    ptr::null_mut(),
                    devices.0,
                    &target,
                    &chosen,
                    0,
                    &mut need_reboot,
                )
            };
            let install_error = (result == 0).then(io::Error::last_os_error);
            println!(
                "DiInstallDevice: success={}, need_reboot={}",
                result != 0,
                need_reboot != 0
            );
            if let Some(error) = install_error {
                return Err(io::Error::other(format!(
                    "Install failed; inspect target for partial changes: {error}"
                )));
            }
            return Ok(if need_reboot != 0 { 3010 } else { 0 });
        }
        if let Err(error) = &device_result {
            eprintln!("device-associated: {error}");
        }
        // SAFETY: no class filter creates an empty, local temporary set. No devnode.
        let global =
            DeviceSet::new(unsafe { SetupDiCreateDeviceInfoList(ptr::null(), ptr::null_mut()) })?;
        let global_result = candidates(&global, None, "global-classless");
        verify_unbound(&devices, &target)?;
        println!(
            "Target binding remains absent. Candidate enumeration does not prove installation or scanning."
        );
        precheck_result(
            device_result.map(|candidate| candidate.is_some()),
            global_result.map(|candidate| candidate.is_some()),
        )
    }

    #[cfg(test)]
    mod abi_tests {
        use super::*;
        #[test]
        fn x64_sdk_layouts_match() {
            assert_eq!(size_of::<DeviceInfo>(), 32);
            assert_eq!(size_of::<InstallParams>(), 584);
            assert_eq!(offset_of!(InstallParams, path), 60);
            assert_eq!(size_of::<DriverInfo>(), 1568);
            assert_eq!(size_of::<DriverDetail>(), 1584);
            assert_eq!(offset_of!(DriverDetail, hardware), 1576);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precheck_requires_one_exact_candidate_in_both_lists() {
        assert_eq!(precheck_result(Ok(true), Ok(true)).unwrap(), 0);
        assert!(precheck_result(Ok(false), Ok(true)).is_err());
        assert!(precheck_result(Ok(true), Ok(false)).is_err());
        assert!(precheck_result(Ok(false), Ok(false)).is_err());
        assert!(precheck_result(Err(io::Error::other("enumeration failed")), Ok(true)).is_err());
        assert!(precheck_result(Ok(true), Err(io::Error::other("enumeration failed"))).is_err());
    }

    fn args(values: &[&str]) -> Vec<std::ffi::OsString> {
        values.iter().map(std::ffi::OsString::from).collect()
    }

    #[test]
    fn arguments_default_to_precheck_and_require_explicit_exact_install_target() {
        assert_eq!(parse_args(&args(&[])).unwrap(), Action::Precheck);
        let id = r"USB\VID_0924&PID_4265&MI_00\private";
        assert_eq!(
            parse_args(&args(&["--install-mi00", id])).unwrap(),
            Action::Install(id.into())
        );
        for invalid in [
            args(&["--install-mi00"]),
            args(&["--install-mi00", id, "extra"]),
            args(&["--unknown"]),
            args(&["--install-mi00", r"USB\VID_0924&PID_4265&MI_01\private"]),
            args(&["--install-mi00", r"USB\VID_0924&PID_4265\private"]),
        ] {
            assert!(parse_args(&invalid).is_err());
        }
    }

    #[test]
    fn expected_instance_must_match_complete_actual_instance() {
        let id = r"USB\VID_0924&PID_4265&MI_00\private";
        assert!(same_target(id, &id.to_ascii_lowercase()));
        assert!(!same_target(id, r"USB\VID_0924&PID_4265&MI_00\other"));
        assert!(!same_target(id, r"USB\VID_0924&PID_4265&MI_01\private"));
    }

    #[test]
    fn nul_string_requires_termination_and_valid_utf16() {
        assert_eq!(read_z(&[65, 0, 66]).unwrap(), "A");
        assert!(read_z(&[65]).is_err());
        assert!(read_z(&[0xd800, 0]).is_err());
        assert!(read_z(&[10, 0]).is_err());
    }

    #[test]
    fn target_is_only_complete_mi00_instance() {
        assert!(target_id(r"usb\vid_0924&pid_4265&mi_00\private"));
        for id in [
            r"USB\VID_0924&PID_4265\private",
            r"USB\VID_0924&PID_4265&MI_01\private",
            r"USB\VID_0924&PID_42650&MI_00\private",
            r"USB\VID_0924&PID_4265&MI_00",
            r"USB\VID_0924&PID_4265&MI_00\private\extra",
        ] {
            assert!(!target_id(id));
        }
    }

    #[test]
    fn candidate_requires_inbox_path_section_provider_and_primary_id() {
        let mut candidate = Candidate {
            inf: r"C:\Windows\INF\winusb.inf".into(),
            section: "WINUSB".into(),
            provider: "Microsoft".into(),
            hardware_id: r"USB\MS_COMP_WINUSB".into(),
        };
        assert!(exact_candidate(&candidate));
        candidate.section = "WINUSB_LowerFilter".into();
        assert!(!exact_candidate(&candidate));
        candidate.section = "winusb".into();
        candidate.hardware_id = r"USB\MS_COMP_WINUSB_extra".into();
        assert!(!exact_candidate(&candidate));
        candidate.hardware_id = r"USB\MS_COMP_WINUSB".into();
        candidate.inf = r"C:\other\winusb.inf".into();
        assert!(!exact_candidate(&candidate));
        candidate.inf = r"c:\windows\inf\WINUSB.INF".into();
        candidate.provider = "Other".into();
        assert!(!exact_candidate(&candidate));
    }
}
