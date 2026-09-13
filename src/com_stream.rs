//! Owned, thread-bound Windows IStream output for the scanning image encoder.
//! This module does not initialize COM, register a driver, or discover WIA devices.

use std::{
    ffi::c_void,
    fmt,
    io::{self, Seek, SeekFrom, Write},
    marker::PhantomData,
    ptr::NonNull,
    rc::Rc,
};

// IUnknown + ISequentialStream + the IStream Seek slot, in SDK objidlbase.h order.
// Only this prefix is accessed; no dependence on Read, Stat, Commit or SetSize.
#[repr(C)]
struct StreamVtable {
    query_interface: unsafe extern "system" fn(*mut c_void, *const c_void, *mut *mut c_void) -> i32,
    add_ref: unsafe extern "system" fn(*mut c_void) -> u32,
    release: unsafe extern "system" fn(*mut c_void) -> u32,
    read: unsafe extern "system" fn(*mut c_void, *mut c_void, u32, *mut u32) -> i32,
    write: unsafe extern "system" fn(*mut c_void, *const c_void, u32, *mut u32) -> i32,
    seek: unsafe extern "system" fn(*mut c_void, i64, u32, *mut u64) -> i32,
}

/// Original COM result retained inside an [`io::Error`] for callers to inspect.
#[derive(Debug)]
pub struct StreamError {
    operation: &'static str,
    hresult: i32,
}

impl StreamError {
    /// Returns the unchanged HRESULT, including unexpected positive statuses.
    pub fn hresult(&self) -> i32 {
        self.hresult
    }
}

impl fmt::Display for StreamError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "IStream::{} returned HRESULT 0x{:08X}",
            self.operation, self.hresult as u32
        )
    }
}

impl std::error::Error for StreamError {}

fn check_result(operation: &'static str, hresult: i32) -> io::Result<()> {
    // Write and Seek document S_OK. Do not silently treat an unexpected positive
    // status (including S_FALSE) as a completed operation or infer WIA cancellation.
    if hresult == 0 {
        return Ok(());
    }
    let kind = match hresult as u32 {
        0x8000_4004 => io::ErrorKind::Interrupted,      // E_ABORT
        0x8003_0070 => io::ErrorKind::StorageFull,      // STG_E_MEDIUMFULL
        0x8003_0005 => io::ErrorKind::PermissionDenied, // STG_E_ACCESSDENIED
        0x8003_0001 => io::ErrorKind::InvalidInput,     // STG_E_INVALIDFUNCTION
        0x8007_04C7 => io::ErrorKind::Interrupted,      // HRESULT_FROM_WIN32(ERROR_CANCELLED)
        value if value & 0xFFFF_0000 == 0x8007_0000 => {
            io::Error::from_raw_os_error((value & 0xFFFF) as i32).kind()
        }
        _ => io::ErrorKind::Other,
    };
    Err(io::Error::new(kind, StreamError { operation, hresult }))
}

/// Owns one IStream reference and exposes only image output and positioning.
///
/// Obtain the pointer from a COM operation returning an owned IStream reference,
/// then pass this adapter to [`crate::bitmap::BmpEncoder`]. The caller owns COM
/// apartment initialization and must keep it initialized until this value drops.
/// No truncation or publication is performed; the encoder requires empty output.
/// Any I/O error invalidates the image, because COM can report failure after
/// consuming bytes. `write_all` stops on every error, including `Interrupted`.
/// `flush` returns `Unsupported`: WIA does not guarantee IStream::Commit.
///
/// Apartment-bound references cannot be moved or shared across threads:
/// ```compile_fail
/// use workcentre_3119::com_stream::ComOutputStream;
/// fn requires_send<T: Send>() {}
/// requires_send::<ComOutputStream>();
/// ```
/// ```compile_fail
/// use workcentre_3119::com_stream::ComOutputStream;
/// fn requires_sync<T: Sync>() {}
/// requires_sync::<ComOutputStream>();
/// ```
pub struct ComOutputStream {
    raw: NonNull<c_void>,
    apartment: PhantomData<Rc<()>>,
}

impl ComOutputStream {
    /// Takes ownership of exactly one reference; does not call AddRef.
    /// Null is rejected without access. A successful constructor calls Release
    /// exactly once on drop. This does not query or change the stream contents.
    ///
    /// # Safety
    /// A non-null pointer must be a valid IStream interface with a live vtable,
    /// usable in the current COM apartment. The caller transfers one owned
    /// reference and must not release it separately. The object must remain
    /// callable through Drop, with no concurrent changes to its seek position or
    /// contents during encoding. The COM apartment must outlive this adapter.
    pub unsafe fn from_raw_owned(raw: *mut c_void) -> io::Result<Self> {
        let raw = NonNull::new(raw).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "IStream pointer is null")
        })?;
        Ok(Self {
            raw,
            apartment: PhantomData,
        })
    }

    fn vtable(&self) -> &StreamVtable {
        // SAFETY: from_raw_owned's contract guarantees the live IStream layout
        // and vtable until Drop, and this thread-bound wrapper owns its reference.
        unsafe { &**self.raw.as_ptr().cast::<*const StreamVtable>() }
    }
}

impl Write for ComOutputStream {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        let count = bytes.len().min(u32::MAX as usize) as u32;
        let mut written = 0u32;
        // SAFETY: the owned reference is live on this thread; the slice covers
        // count bytes and written is a valid ULONG output for this synchronous call.
        let hresult = unsafe {
            (self.vtable().write)(
                self.raw.as_ptr(),
                bytes.as_ptr().cast(),
                count,
                &mut written,
            )
        };
        // A failed HRESULT wins even when pcbWritten reports partial or full progress.
        check_result("Write", hresult)?;
        if written > count {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "IStream wrote more bytes than provided",
            ));
        }
        if written == 0 {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "IStream writer made no progress",
            ));
        }
        Ok(written as usize)
    }

    fn write_all(&mut self, mut bytes: &[u8]) -> io::Result<()> {
        // std's default retries Interrupted; COM cancellation must stop immediately.
        while !bytes.is_empty() {
            let written = self.write(bytes)?;
            bytes = &bytes[written..];
        }
        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "WIA output does not guarantee IStream::Commit",
        ))
    }
}

impl Seek for ComOutputStream {
    fn seek(&mut self, from: SeekFrom) -> io::Result<u64> {
        let (offset, origin) = match from {
            // COM interprets LARGE_INTEGER as unsigned for STREAM_SEEK_SET.
            SeekFrom::Start(value) => (value as i64, 0),
            SeekFrom::Current(value) => (value, 1),
            SeekFrom::End(value) => (value, 2),
        };
        let mut position = 0u64;
        // SAFETY: live owned IStream, SDK seek origin and a valid ULARGE_INTEGER
        // output. On Windows x64 LARGE_INTEGER is passed as one 64-bit value.
        let hresult =
            unsafe { (self.vtable().seek)(self.raw.as_ptr(), offset, origin, &mut position) };
        check_result("Seek", hresult)?;
        Ok(position)
    }
}

impl Drop for ComOutputStream {
    fn drop(&mut self) {
        // SAFETY: exactly one owned reference is released, on the originating thread,
        // before the caller tears down its COM apartment; raw is not used afterward.
        unsafe {
            (self.vtable().release)(self.raw.as_ptr());
        }
    }
}
