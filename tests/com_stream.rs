#![cfg(windows)]
//! Synthetic COM boundaries plus a real Windows OLE memory stream; no WIA registration.
use std::{
    ffi::c_void,
    io::{self, Cursor, Seek, SeekFrom, Write},
    ptr,
};
use workcentre_3119::{
    bitmap::BmpEncoder,
    com_stream::{ComOutputStream, StreamError},
    scan::{ColorMode, ImageBand, ScanSummary},
};

#[repr(C)]
struct Vtable {
    query: unsafe extern "system" fn(*mut Fake, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut Fake) -> u32,
    release: unsafe extern "system" fn(*mut Fake) -> u32,
    read: unsafe extern "system" fn(*mut Fake, *mut c_void, u32, *mut u32) -> i32,
    write: unsafe extern "system" fn(*mut Fake, *const c_void, u32, *mut u32) -> i32,
    seek: unsafe extern "system" fn(*mut Fake, i64, u32, *mut u64) -> i32,
}

#[repr(C)]
struct Fake {
    vtable: *const Vtable,
    data: Cursor<Vec<u8>>,
    released: u32,
    writes: u32,
    max_write: u32,
    write_hr: i32,
    seek_hr: i32,
    overreport: bool,
    last_seek: (i64, u32),
}

unsafe extern "system" fn query(_: *mut Fake, _: *const c_void, _: *mut *mut c_void) -> i32 {
    0x80004002u32 as i32
}
unsafe extern "system" fn add_ref(_: *mut Fake) -> u32 {
    2
}
unsafe extern "system" fn release(raw: *mut Fake) -> u32 {
    // SAFETY: tests retain the boxed fake until the adapter is dropped.
    unsafe {
        (*raw).released += 1;
    }
    0
}
unsafe extern "system" fn read(_: *mut Fake, _: *mut c_void, _: u32, _: *mut u32) -> i32 {
    0x80004001u32 as i32
}
unsafe extern "system" fn write(
    raw: *mut Fake,
    data: *const c_void,
    length: u32,
    written: *mut u32,
) -> i32 {
    // SAFETY: adapter supplies this live fake, readable length-byte slice and ULONG output.
    unsafe {
        let fake = &mut *raw;
        fake.writes += 1;
        let count = length.min(fake.max_write);
        let bytes = std::slice::from_raw_parts(data.cast::<u8>(), count as usize);
        if fake.data.write_all(bytes).is_err() {
            return 0x8003001Du32 as i32;
        }
        *written = if fake.overreport { length + 1 } else { count };
        fake.write_hr
    }
}
unsafe extern "system" fn seek(
    raw: *mut Fake,
    offset: i64,
    origin: u32,
    position: *mut u64,
) -> i32 {
    // SAFETY: adapter supplies this live fake and a valid 64-bit position output.
    unsafe {
        let fake = &mut *raw;
        fake.last_seek = (offset, origin);
        if fake.seek_hr != 0 {
            return fake.seek_hr;
        }
        let from = match origin {
            0 => SeekFrom::Start(offset as u64),
            1 => SeekFrom::Current(offset),
            2 => SeekFrom::End(offset),
            _ => return 0x80030001u32 as i32,
        };
        match fake.data.seek(from) {
            Ok(value) => {
                *position = value;
                0
            }
            Err(_) => 0x80030001u32 as i32,
        }
    }
}
static VTABLE: Vtable = Vtable {
    query,
    add_ref,
    release,
    read,
    write,
    seek,
};

fn fake() -> Box<Fake> {
    Box::new(Fake {
        vtable: &VTABLE,
        data: Cursor::new(Vec::new()),
        released: 0,
        writes: 0,
        max_write: u32::MAX,
        write_hr: 0,
        seek_hr: 0,
        overreport: false,
        last_seek: (0, 0),
    })
}
fn owned(fake: &mut Fake) -> ComOutputStream {
    // SAFETY: fake outlives adapter on this thread; its single synthetic COM reference
    // is transferred to the adapter, which only calls the initialized vtable prefix.
    unsafe { ComOutputStream::from_raw_owned((fake as *mut Fake).cast()).unwrap() }
}

#[test]
fn short_writes_preserve_bytes_and_release_exactly_once() {
    let mut fake = fake();
    fake.max_write = 2;
    {
        let mut stream = owned(&mut fake);
        stream.write_all(&[10, 20, 30, 40, 50]).unwrap();
        assert_eq!(stream.seek(SeekFrom::End(-2)).unwrap(), 3);
        assert_eq!(stream.seek(SeekFrom::Current(-1)).unwrap(), 2);
        assert_eq!(stream.seek(SeekFrom::Start(u64::MAX)).unwrap(), u64::MAX);
        assert_eq!(fake.last_seek, (-1, 0));
        assert_eq!(
            stream.flush().unwrap_err().kind(),
            io::ErrorKind::Unsupported
        );
    }
    assert_eq!(fake.released, 1);
    assert_eq!(fake.writes, 3);
    assert_eq!(fake.data.get_ref(), &[10, 20, 30, 40, 50]);
}

#[test]
fn failed_hresult_with_partial_count_is_not_success_or_retried() {
    for (hr, kind) in [
        (0x80004004u32, io::ErrorKind::Interrupted),
        (0x80030070, io::ErrorKind::StorageFull),
        (0x80030005, io::ErrorKind::PermissionDenied),
        (0x81234567, io::ErrorKind::Other),
        (1, io::ErrorKind::Other),
    ] {
        let mut fake = fake();
        fake.max_write = 1;
        fake.write_hr = hr as i32;
        let error = owned(&mut fake).write_all(&[1, 2, 3]).unwrap_err();
        assert_eq!(error.kind(), kind);
        assert_eq!(
            error
                .get_ref()
                .unwrap()
                .downcast_ref::<StreamError>()
                .unwrap()
                .hresult(),
            hr as i32
        );
        assert_eq!(fake.writes, 1);
        assert_eq!(fake.data.get_ref(), &[1]);
        assert_eq!(fake.released, 1);
    }
}

#[test]
fn zero_or_excess_count_and_seek_error_are_reported() {
    let mut zero = fake();
    zero.max_write = 0;
    assert_eq!(
        owned(&mut zero).write_all(&[1]).unwrap_err().kind(),
        io::ErrorKind::WriteZero
    );
    assert_eq!(zero.writes, 1);
    let mut excessive = fake();
    excessive.overreport = true;
    assert_eq!(
        owned(&mut excessive).write(&[1]).unwrap_err().kind(),
        io::ErrorKind::InvalidData
    );
    let mut failed_seek = fake();
    failed_seek.seek_hr = 0x80030001u32 as i32;
    let error = owned(&mut failed_seek)
        .seek(SeekFrom::Start(0))
        .unwrap_err();
    assert_eq!(
        error
            .get_ref()
            .unwrap()
            .downcast_ref::<StreamError>()
            .unwrap()
            .hresult(),
        failed_seek.seek_hr
    );
}

#[test]
fn null_owned_pointer_is_rejected_without_dereferencing() {
    // SAFETY: null is explicitly rejected by the constructor before dereferencing.
    let result = unsafe { ComOutputStream::from_raw_owned(ptr::null_mut()) };
    assert!(result.is_err());
}

#[link(name = "Ole32")]
unsafe extern "system" {
    fn CreateStreamOnHGlobal(global: *mut c_void, delete: i32, stream: *mut *mut c_void) -> i32;
    fn CoInitializeEx(reserved: *mut c_void, concurrency: u32) -> i32;
    fn CoUninitialize();
}

#[test]
fn real_windows_istream_encodes_and_reads_back_bmp() {
    struct Apartment;
    impl Drop for Apartment {
        fn drop(&mut self) {
            // SAFETY: balances this test thread's successful CoInitializeEx call.
            unsafe {
                CoUninitialize();
            }
        }
    }
    // SAFETY: null reserved pointer and COINIT_MULTITHREADED initialize only this thread.
    let initialized = unsafe { CoInitializeEx(ptr::null_mut(), 0) };
    assert!(initialized == 0 || initialized == 1);
    let _apartment = Apartment;
    let mut raw = ptr::null_mut();
    // SAFETY: null requests an empty allocation, output is valid; TRUE frees it on Release.
    let created = unsafe { CreateStreamOnHGlobal(ptr::null_mut(), 1, &mut raw) };
    assert_eq!(created, 0);
    // SAFETY: Windows returned one owned IStream reference, used and released on this thread.
    let mut stream = unsafe { ComOutputStream::from_raw_owned(raw).unwrap() };
    let mut encoder = BmpEncoder::new(&mut stream, 300, ColorMode::Rgb).unwrap();
    encoder
        .push(&ImageBand {
            width: 2,
            rows: 1,
            mode: ColorMode::Rgb,
            pixels: vec![1, 2, 3, 100, 110, 120],
            wire_data: vec![],
        })
        .unwrap();
    encoder
        .finish(&ScanSummary {
            width: 2,
            height: 1,
            bands: 1,
            bytes: 6,
        })
        .unwrap();
    assert_eq!(stream.stream_position().unwrap(), 2);
    assert_eq!(stream.seek(SeekFrom::End(0)).unwrap(), 62);
    stream.seek(SeekFrom::Start(0)).unwrap();
    let mut bytes = [0u8; 62];
    let mut count = 0u32;
    // SAFETY: raw remains owned by stream. SDK IStream prefix has Read at slot 3;
    // the output array has 62 bytes and count is a live ULONG. Read is test-only.
    let hr = unsafe {
        let vtable = *(raw as *mut *const Vtable);
        ((*vtable).read)(
            raw.cast(),
            bytes.as_mut_ptr().cast(),
            bytes.len() as u32,
            &mut count,
        )
    };
    assert_eq!((hr, count), (0, 62));
    assert_eq!(&bytes[..2], b"BM");
    assert_eq!(&bytes[54..], &[3, 2, 1, 120, 110, 100, 0, 0]);
    assert_eq!(i32::from_le_bytes(bytes[22..26].try_into().unwrap()), -1);
}
