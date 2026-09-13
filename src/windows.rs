//! Read-only Configuration Manager access. No installation or USB transfers.
use crate::{Device, identify};
use std::{ffi::c_void, io, ptr};

// Values and signatures verified against Windows SDK 10.0.26100.0,
// um/cfgmgr32.h and shared/cfg.h.
const CR_SUCCESS: u32 = 0;
const CR_BUFFER_SMALL: u32 = 0x1a;
const CR_NO_SUCH_VALUE: u32 = 0x25;
const CM_GETIDLIST_FILTER_PRESENT: u32 = 0x100;
const CM_DRP_SERVICE: u32 = 5;
const DN_STARTED: u32 = 8;

#[link(name = "Cfgmgr32")]
unsafe extern "system" {
    fn CM_Get_Device_ID_List_SizeW(length: *mut u32, filter: *const u16, flags: u32) -> u32;
    fn CM_Get_Device_ID_ListW(filter: *const u16, buffer: *mut u16, length: u32, flags: u32)
    -> u32;
    fn CM_Locate_DevNodeW(node: *mut u32, id: *const u16, flags: u32) -> u32;
    fn CM_Get_DevNode_Status(status: *mut u32, problem: *mut u32, node: u32, flags: u32) -> u32;
    fn CM_Get_DevNode_Registry_PropertyW(
        node: u32,
        property: u32,
        kind: *mut u32,
        buffer: *mut c_void,
        length: *mut u32,
        flags: u32,
    ) -> u32;
}

fn check(code: u32, operation: &str) -> io::Result<()> {
    if code == CR_SUCCESS {
        Ok(())
    } else {
        Err(io::Error::other(format!(
            "{operation}: Configuration Manager error 0x{code:08x}"
        )))
    }
}

fn present_ids() -> io::Result<Vec<u16>> {
    // Plug/unplug can change the list between the size query and the read.
    for _ in 0..3 {
        let mut length = 0;
        check(
            // SAFETY: length is writable; null filter is valid for PRESENT.
            unsafe {
                CM_Get_Device_ID_List_SizeW(&mut length, ptr::null(), CM_GETIDLIST_FILTER_PRESENT)
            },
            "Get device list size",
        )?;
        if length == 0 || length > 16 * 1024 * 1024 {
            return Err(io::Error::other("Invalid device list size"));
        }
        let mut ids = vec![0u16; length as usize];
        // SAFETY: ids contains length writable UTF-16 code units.
        let result = unsafe {
            CM_Get_Device_ID_ListW(
                ptr::null(),
                ids.as_mut_ptr(),
                length,
                CM_GETIDLIST_FILTER_PRESENT,
            )
        };
        if result == CR_BUFFER_SMALL {
            continue;
        }
        check(result, "Get device list")?;
        return Ok(ids);
    }
    Err(io::Error::other(
        "Device list changed repeatedly; reconnect and retry",
    ))
}

fn service(node: u32) -> io::Result<Option<String>> {
    for _ in 0..3 {
        let mut bytes = 0;
        let mut kind = 0;
        // SAFETY: output pointers are valid; a null data buffer queries size.
        let code = unsafe {
            CM_Get_DevNode_Registry_PropertyW(
                node,
                CM_DRP_SERVICE,
                &mut kind,
                ptr::null_mut(),
                &mut bytes,
                0,
            )
        };
        if code == CR_NO_SUCH_VALUE {
            return Ok(None);
        }
        if code != CR_BUFFER_SMALL {
            check(code, "Get service size")?;
        }
        if bytes == 0 {
            return Ok(None);
        }
        if bytes > 64 * 1024 || bytes % 2 != 0 {
            return Err(io::Error::other("Invalid service property length"));
        }
        let mut buffer = vec![0u16; bytes as usize / 2];
        // SAFETY: buffer has bytes writable bytes and u16 alignment.
        let code = unsafe {
            CM_Get_DevNode_Registry_PropertyW(
                node,
                CM_DRP_SERVICE,
                &mut kind,
                buffer.as_mut_ptr().cast(),
                &mut bytes,
                0,
            )
        };
        if code == CR_BUFFER_SMALL {
            continue;
        }
        if code == CR_NO_SUCH_VALUE {
            return Ok(None);
        }
        check(code, "Get service")?;
        if kind != 1 || bytes % 2 != 0 || bytes as usize > buffer.len() * 2 {
            return Err(io::Error::other("Service is not a valid REG_SZ property"));
        }
        buffer.truncate(bytes as usize / 2);
        let end = buffer
            .iter()
            .position(|v| *v == 0)
            .ok_or_else(|| io::Error::other("Unterminated service property"))?;
        let value = String::from_utf16(&buffer[..end])
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        return Ok((!value.is_empty()).then_some(value));
    }
    Err(io::Error::other(
        "Driver binding changed repeatedly; retry discovery",
    ))
}

pub fn discover() -> io::Result<Vec<Device>> {
    let ids = present_ids()?;
    let mut devices = Vec::new();
    for raw in ids.split(|v| *v == 0).take_while(|v| !v.is_empty()) {
        let id =
            String::from_utf16(raw).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let Some(interface) = identify(&id) else {
            continue;
        };
        let terminated: Vec<u16> = raw.iter().copied().chain(Some(0)).collect();
        let mut node = 0;
        check(
            // SAFETY: terminated is a live NUL-terminated string; node is writable.
            unsafe { CM_Locate_DevNodeW(&mut node, terminated.as_ptr(), 0) },
            "Locate Xerox interface",
        )?;
        let (mut status, mut problem_code) = (0, 0);
        check(
            // SAFETY: output pointers are writable and node came from CM_Locate_DevNodeW.
            unsafe { CM_Get_DevNode_Status(&mut status, &mut problem_code, node, 0) },
            "Get Xerox interface status",
        )?;
        devices.push(Device {
            interface,
            service: service(node)?,
            problem_code,
            started: status & DN_STARTED != 0,
        });
    }
    devices.sort_by_key(|d| match d.interface {
        crate::Interface::Composite => 0,
        crate::Interface::Scanner => 1,
        crate::Interface::Printer => 2,
    });
    Ok(devices)
}
