#![cfg(windows)]

use std::{ffi::c_void, io, ptr, slice};

type HResult = i32;
type BStr = *mut u16;

const S_OK: HResult = 0;
const S_FALSE: HResult = 1;
const E_NOINTERFACE: HResult = 0x8000_4002u32 as HResult;
const WIA_STATUS_SKIP_ITEM: HResult = 0x0021_0009;

#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq)]
struct Guid {
    data1: u32,
    data2: u16,
    data3: u16,
    data4: [u8; 8],
}

const IID_IWIA_MINI_DRV_TRANSFER_CALLBACK: Guid = Guid {
    data1: 0xa9d2_ee89,
    data2: 0x2ce5,
    data3: 0x4ff0,
    data4: [0x8a, 0xdb, 0xc9, 0x61, 0xd1, 0xd7, 0x74, 0xca],
};

#[repr(C)]
struct TransferVtable {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    get_next_stream:
        unsafe extern "system" fn(*mut c_void, i32, BStr, BStr, *mut *mut c_void) -> HResult,
    send_message: unsafe extern "system" fn(*mut c_void, i32, *const WiaTransferParams) -> HResult,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct WiaTransferParams {
    message: i32,
    percent: i32,
    bytes: u64,
    error_status: HResult,
}

#[repr(C)]
struct StreamVtable {
    query_interface:
        unsafe extern "system" fn(*mut c_void, *const Guid, *mut *mut c_void) -> HResult,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    read: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut u32) -> HResult,
    write: unsafe extern "system" fn(*mut c_void, *const c_void, u32, *mut u32) -> HResult,
    seek: unsafe extern "system" fn(*mut c_void, i64, u32, *mut u64) -> HResult,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NextStreamPlan {
    Stream,
    Cancelled,
    Skipped,
    NullSuccess,
    Error(HResult),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SendMessagePlan {
    Continue,
    CancelAt(usize),
    ErrorAt(usize, HResult),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Message {
    pub flags: i32,
    pub message: i32,
    pub percent: i32,
    pub bytes: u64,
    pub error_status: HResult,
}

#[repr(C)]
pub struct FakeTransferCallback {
    vtable: *const TransferVtable,
    reference_count: u32,
    release_calls: u32,
    query_result: HResult,
    next_plan: NextStreamPlan,
    send_plan: SendMessagePlan,
    next_stream_calls: usize,
    send_message_calls: usize,
    last_item_name: Option<String>,
    last_full_item_name: Option<String>,
    stream: *mut c_void,
    messages: Vec<Message>,
    hook: Option<Box<dyn FnMut()>>,
}

impl FakeTransferCallback {
    pub fn new() -> Self {
        Self {
            vtable: &VTABLE,
            reference_count: 1,
            release_calls: 0,
            query_result: S_OK,
            next_plan: NextStreamPlan::Stream,
            send_plan: SendMessagePlan::Continue,
            next_stream_calls: 0,
            send_message_calls: 0,
            last_item_name: None,
            last_full_item_name: None,
            stream: ptr::null_mut(),
            messages: Vec::new(),
            hook: None,
        }
    }

    pub fn set_hook(&mut self, hook: Box<dyn FnMut()>) {
        self.hook = Some(hook);
    }

    fn call_hook(&mut self) {
        if let Some(hook) = self.hook.as_mut() {
            hook();
        }
    }

    pub fn as_raw(&mut self) -> *mut c_void {
        self as *mut Self as *mut c_void
    }

    pub fn set_query_result(&mut self, result: HResult) {
        self.query_result = result;
    }

    pub fn set_next_stream(&mut self, plan: NextStreamPlan) {
        self.next_plan = plan;
    }

    pub fn set_send_plan(&mut self, plan: SendMessagePlan) {
        self.send_plan = plan;
    }

    pub fn reference_count(&self) -> u32 {
        self.reference_count
    }

    pub fn release_calls(&self) -> u32 {
        self.release_calls
    }

    pub fn next_stream_calls(&self) -> usize {
        self.next_stream_calls
    }

    pub fn send_message_calls(&self) -> usize {
        self.send_message_calls
    }

    pub fn last_item_name(&self) -> Option<&str> {
        self.last_item_name.as_deref()
    }

    pub fn last_full_item_name(&self) -> Option<&str> {
        self.last_full_item_name.as_deref()
    }

    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    pub fn stream_reference_count(&self) -> u32 {
        if self.stream.is_null() {
            return 0;
        }
        let vtable = self.stream_vtable();
        // SAFETY: this fixture retains one live stream reference for its lifetime;
        // the temporary AddRef/Release pair measures the current native count.
        unsafe {
            (vtable.add_ref)(self.stream);
            (vtable.release)(self.stream)
        }
    }

    pub fn stream_bytes(&self) -> io::Result<Vec<u8>> {
        if self.stream.is_null() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "stream not created",
            ));
        }
        let vtable = self.stream_vtable();
        let mut end = 0u64;
        // SAFETY: the retained stream has a live vtable and the output position is valid.
        let hr = unsafe { (vtable.seek)(self.stream, 0, 2, &mut end) };
        if hr != S_OK {
            return Err(io::Error::from_raw_os_error(hr));
        }
        let mut position = 0u64;
        // SAFETY: the retained stream has a live vtable and the output position is valid.
        let hr = unsafe { (vtable.seek)(self.stream, 0, 0, &mut position) };
        if hr != S_OK {
            return Err(io::Error::from_raw_os_error(hr));
        }
        let mut bytes = vec![0u8; end as usize];
        let mut read = 0u32;
        // SAFETY: the output buffer is valid for its bounded length and `read` is a live ULONG.
        let hr = unsafe {
            (vtable.read)(
                self.stream,
                bytes.as_mut_ptr().cast(),
                bytes.len() as u32,
                &mut read,
            )
        };
        if hr != S_OK || read as usize != bytes.len() {
            return Err(io::Error::from_raw_os_error(hr));
        }
        Ok(bytes)
    }

    fn stream_vtable(&self) -> &StreamVtable {
        // SAFETY: stream is retained from CreateStreamOnHGlobal until Drop.
        unsafe { &**self.stream.cast::<*const StreamVtable>() }
    }

    fn ensure_stream(&mut self) -> HResult {
        if !self.stream.is_null() {
            return S_OK;
        }
        let mut stream = ptr::null_mut();
        // SAFETY: COM is initialized by the test before a Stream plan is used; output is valid.
        let hr = unsafe { CreateStreamOnHGlobal(ptr::null_mut(), 1, &mut stream) };
        if hr != S_OK {
            return hr;
        }
        self.stream = stream;
        S_OK
    }
}

impl Default for FakeTransferCallback {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for FakeTransferCallback {
    fn drop(&mut self) {
        if !self.stream.is_null() {
            let stream = self.stream;
            self.stream = ptr::null_mut();
            // SAFETY: this fixture retains exactly one stream reference until Drop.
            unsafe {
                let vtable = &**stream.cast::<*const StreamVtable>();
                (vtable.release)(stream);
            }
        }
    }
}

unsafe extern "system" fn query_interface(
    raw: *mut c_void,
    iid: *const Guid,
    output: *mut *mut c_void,
) -> HResult {
    if output.is_null() {
        return 0x8000_4003u32 as HResult;
    }
    // SAFETY: fake calls use a live fixture and valid IID/output pointers.
    unsafe {
        *output = ptr::null_mut();
        let fake = &mut *raw.cast::<FakeTransferCallback>();
        fake.call_hook();
        if fake.query_result != S_OK {
            return fake.query_result;
        }
        if iid.is_null() || *iid != IID_IWIA_MINI_DRV_TRANSFER_CALLBACK {
            return E_NOINTERFACE;
        }
        fake.reference_count += 1;
        *output = raw;
        S_OK
    }
}

unsafe extern "system" fn add_ref(raw: *mut c_void) -> u32 {
    // SAFETY: COM caller supplies the live fixture pointer.
    unsafe {
        let fake = &mut *raw.cast::<FakeTransferCallback>();
        fake.reference_count += 1;
        fake.reference_count
    }
}

unsafe extern "system" fn release(raw: *mut c_void) -> u32 {
    // SAFETY: COM caller supplies the live fixture pointer; Box owns its storage.
    unsafe {
        let fake = &mut *raw.cast::<FakeTransferCallback>();
        fake.call_hook();
        fake.release_calls += 1;
        fake.reference_count = fake.reference_count.saturating_sub(1);
        fake.reference_count
    }
}

unsafe extern "system" fn get_next_stream(
    raw: *mut c_void,
    flags: i32,
    item_name: BStr,
    full_item_name: BStr,
    output: *mut *mut c_void,
) -> HResult {
    if output.is_null() {
        return 0x8000_4003u32 as HResult;
    }
    // SAFETY: BSTR arguments are valid for the duration of this synchronous callback.
    unsafe {
        *output = ptr::null_mut();
        let fake = &mut *raw.cast::<FakeTransferCallback>();
        fake.call_hook();
        fake.next_stream_calls += 1;
        fake.last_item_name = Some(read_bstr(item_name));
        fake.last_full_item_name = Some(read_bstr(full_item_name));
        assert_eq!(flags, 0);
        match fake.next_plan {
            NextStreamPlan::Stream => {
                let hr = fake.ensure_stream();
                if hr != S_OK {
                    return hr;
                }
                // SAFETY: ensure_stream retained a live native stream reference; this AddRef
                // transfers a second reference to the adapter through the out parameter.
                (fake.stream_vtable().add_ref)(fake.stream);
                *output = fake.stream;
                S_OK
            }
            NextStreamPlan::Cancelled => S_FALSE,
            NextStreamPlan::Skipped => WIA_STATUS_SKIP_ITEM,
            NextStreamPlan::NullSuccess => S_OK,
            NextStreamPlan::Error(hr) => hr,
        }
    }
}

unsafe extern "system" fn send_message(
    raw: *mut c_void,
    flags: i32,
    params: *const WiaTransferParams,
) -> HResult {
    if params.is_null() {
        return 0x8000_4003u32 as HResult;
    }
    // SAFETY: callback supplies a live fixture and a valid transfer parameter block.
    unsafe {
        let fake = &mut *raw.cast::<FakeTransferCallback>();
        fake.call_hook();
        fake.send_message_calls += 1;
        let params = *params;
        fake.messages.push(Message {
            flags,
            message: params.message,
            percent: params.percent,
            bytes: params.bytes,
            error_status: params.error_status,
        });
        match fake.send_plan {
            SendMessagePlan::Continue => S_OK,
            SendMessagePlan::CancelAt(call) if call == fake.send_message_calls => S_FALSE,
            SendMessagePlan::ErrorAt(call, hr) if call == fake.send_message_calls => hr,
            SendMessagePlan::CancelAt(_) | SendMessagePlan::ErrorAt(_, _) => S_OK,
        }
    }
}

static VTABLE: TransferVtable = TransferVtable {
    query_interface,
    add_ref,
    release,
    get_next_stream,
    send_message,
};

fn read_bstr(value: BStr) -> String {
    if value.is_null() {
        return String::new();
    }
    // SAFETY: SysStringLen reads the allocator-owned BSTR length; caller keeps it live.
    let length = unsafe { SysStringLen(value) } as usize;
    // SAFETY: BSTR covers exactly SysStringLen UTF-16 code units.
    let raw = unsafe { slice::from_raw_parts(value, length) };
    String::from_utf16(raw).expect("fixture received valid UTF-16 BSTR")
}

pub struct ComApartment;

impl ComApartment {
    pub fn new() -> Self {
        // SAFETY: null reserved pointer and COINIT_MULTITHREADED initialize only this test thread.
        let hr = unsafe { CoInitializeEx(ptr::null_mut(), 0) };
        assert!(hr == 0 || hr == 1, "CoInitializeEx failed: 0x{hr:08X}");
        Self
    }
}

impl Drop for ComApartment {
    fn drop(&mut self) {
        // SAFETY: balances this fixture thread's successful CoInitializeEx call.
        unsafe { CoUninitialize() }
    }
}

#[link(name = "Ole32")]
unsafe extern "system" {
    fn CoInitializeEx(reserved: *mut c_void, coinit: u32) -> HResult;
    fn CoUninitialize();
    fn CreateStreamOnHGlobal(
        global: *mut c_void,
        delete_on_release: i32,
        stream: *mut *mut c_void,
    ) -> HResult;
}

#[link(name = "OleAut32")]
unsafe extern "system" {
    fn SysStringLen(value: BStr) -> u32;
}
