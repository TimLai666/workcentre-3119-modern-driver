//! Single-image native WIA callback transfer using the existing scan session.
//! IWiaMiniDrv dispatch is in com_server::minidrv; service integration is unfinished.

use crate::{
    bitmap::BmpEncoder,
    scan::{ImageBand, ScanRequest, ScanSummary, SessionHealth},
    wia_callback::{CallbackStatus, NextStream, TransferCallback},
};
use std::{
    ffi::c_void,
    fmt, io,
    sync::atomic::{AtomicBool, Ordering},
};

/// Only `Completed` permits publication of the output image.
#[derive(Debug)]
pub enum TransferOutcome {
    Completed(ScanSummary),
    Cancelled,
    Skipped,
}

/// Retains the original callback/stream error and scan cleanup diagnostics.
/// In particular, an IStream `Interrupted` error is not WIA cancellation.
#[derive(Debug)]
pub struct TransferError {
    original: io::Error,
    scan_diagnostic: String,
}

impl TransferError {
    pub fn original(&self) -> &io::Error {
        &self.original
    }
}
impl fmt::Display for TransferError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}; scan: {}", self.original, self.scan_diagnostic)
    }
}
impl std::error::Error for TransferError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.original)
    }
}

pub(crate) unsafe fn transfer_in_session(
    usb: &mut crate::usb::UsbSession,
    request: ScanRequest,
    expected_rows: u32,
    cancel: &AtomicBool,
    raw_callback: *mut c_void,
    item: &str,
    full_item: &str,
) -> (io::Result<TransferOutcome>, SessionHealth) {
    // SAFETY: the enclosing driver call retains the borrowed COM reference and
    // apartment; its session lease prevents any reentrant USB operation.
    let mut callback = match unsafe { TransferCallback::query_from_borrowed(raw_callback) } {
        Ok(callback) => callback,
        Err(error) => return (Err(error), SessionHealth::Ready),
    };
    transfer(
        &mut callback,
        request,
        expected_rows,
        cancel,
        item,
        full_item,
        |sink| crate::scan::scan_in_session(usb, request, cancel, sink),
    )
}

fn transfer(
    callback: &mut TransferCallback,
    request: ScanRequest,
    expected_rows: u32,
    cancel: &AtomicBool,
    item: &str,
    full_item: &str,
    run: impl FnOnce(
        &mut dyn FnMut(&ImageBand) -> io::Result<()>,
    ) -> (io::Result<ScanSummary>, SessionHealth),
) -> (io::Result<TransferOutcome>, SessionHealth) {
    let mut health = SessionHealth::Ready;
    let result = (|| {
        if expected_rows == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Expected image height is zero",
            ));
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(TransferOutcome::Cancelled);
        }
        let mut stream = match callback.next_stream(item, full_item)? {
            NextStream::Stream(stream) => stream,
            NextStream::Cancelled => return Ok(TransferOutcome::Cancelled),
            // One flatbed item, with no RESERVE or scan started yet. There is
            // no current page to drain and no next item in this operation.
            NextStream::Skipped => return Ok(TransferOutcome::Skipped),
        };
        if callback.status(0, 0)? == CallbackStatus::Cancelled || cancel.load(Ordering::Relaxed) {
            return Ok(TransferOutcome::Cancelled);
        }
        let mut encoder = BmpEncoder::new(&mut stream, request.dpi, request.mode)?;
        let mut callback_cancelled = false;
        let mut output_failure = None;
        let (summary, current_health) = run(&mut |band| {
            let result = (|| {
                encoder.push(band)?;
                let (rows, bytes) = encoder.progress();
                // Selected height is an estimate until READ finishes. Never
                // advertise completion before device cleanup and BMP finalization.
                let percent = (u64::from(rows) * 100 / u64::from(expected_rows)).min(99) as u32;
                if callback.status(percent, bytes)? == CallbackStatus::Cancelled {
                    callback_cancelled = true;
                    return Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        "WIA callback cancelled transfer",
                    ));
                }
                Ok(())
            })();
            if let Err(error) = result {
                let signal = io::Error::new(error.kind(), error.to_string());
                output_failure = Some(error);
                return Err(signal);
            }
            Ok(())
        });
        health = current_health;
        let summary = match summary {
            Ok(summary) => summary,
            Err(error) => {
                if callback_cancelled && health == SessionHealth::Ready {
                    return Ok(TransferOutcome::Cancelled);
                }
                return Err(match output_failure {
                    Some(original) => io::Error::new(
                        original.kind(),
                        TransferError {
                            original,
                            scan_diagnostic: error.to_string(),
                        },
                    ),
                    None => error,
                });
            }
        };
        // A successful core must not conceal a failed output callback.
        if let Some(error) = output_failure {
            return Err(error);
        }
        if cancel.load(Ordering::Relaxed) {
            return Ok(TransferOutcome::Cancelled);
        }
        let (_, bytes) = encoder.progress();
        encoder.finish(&summary)?;
        drop(stream);
        if callback.status(100, bytes)? == CallbackStatus::Cancelled {
            return Ok(TransferOutcome::Cancelled);
        }
        // END_OF_STREAM / END_OF_TRANSFER are emitted by the WIA service,
        // never manually by a minidriver (Microsoft WIA transfer constants).
        Ok(TransferOutcome::Completed(summary))
    })();
    (result, health)
}

#[cfg(test)]
#[allow(dead_code)]
#[path = "../tests/support/wia_callback.rs"]
pub(crate) mod fixture;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::ColorMode;
    use fixture::{ComApartment, FakeTransferCallback, NextStreamPlan, SendMessagePlan};

    fn request() -> ScanRequest {
        ScanRequest {
            dpi: 75,
            mode: ColorMode::Rgb,
            x_units: 0,
            y_units: 0,
            width_units: 48,
            height_units: 32,
        }
    }
    fn band() -> ImageBand {
        // Synthetic RGB samples, deliberately including midtones and padding.
        ImageBand {
            width: 3,
            rows: 1,
            mode: ColorMode::Rgb,
            pixels: vec![1, 40, 255, 7, 128, 0, 9, 16, 32],
            wire_data: vec![],
        }
    }
    fn summary() -> ScanSummary {
        ScanSummary {
            width: 3,
            height: 2,
            bands: 2,
            bytes: 18,
        }
    }
    fn callback(fake: &mut FakeTransferCallback) -> TransferCallback {
        // SAFETY: fixture storage and initialized apartment outlive this adapter.
        unsafe { TransferCallback::query_from_borrowed(fake.as_raw()).unwrap() }
    }

    #[test]
    fn actual_com_stream_preserves_pixels_and_only_reports_status() {
        let _com = ComApartment::new();
        let mut fake = FakeTransferCallback::new();
        let mut cb = callback(&mut fake);
        let (result, health) = transfer(
            &mut cb,
            request(),
            2,
            &AtomicBool::new(false),
            "Flatbed",
            "Root\\Flatbed",
            |sink| {
                sink(&band()).unwrap();
                sink(&band()).unwrap();
                // No completion before the simulated successful cleanup returns.
                assert!(fake.messages().iter().all(|m| m.percent < 100));
                (Ok(summary()), SessionHealth::Ready)
            },
        );
        assert!(matches!(result.unwrap(), TransferOutcome::Completed(_)));
        assert_eq!(health, SessionHealth::Ready);
        let bytes = fake.stream_bytes().unwrap();
        assert_eq!(&bytes[..2], b"BM");
        assert_eq!(bytes.len(), 78);
        assert_eq!(&bytes[54..66], &[255, 40, 1, 0, 128, 7, 32, 16, 9, 0, 0, 0]);
        assert_eq!(&bytes[54..66], &bytes[66..78]);
        let messages = fake.messages();
        assert_eq!(
            messages
                .iter()
                .map(|m| (m.percent, m.bytes))
                .collect::<Vec<_>>(),
            [(0, 0), (50, 66), (99, 78), (100, 78)]
        );
        assert!(
            messages
                .iter()
                .all(|m| m.message == 1 && m.flags == 0 && m.error_status == 0)
        );
        drop(cb);
        assert_eq!(fake.reference_count(), 1);
        assert_eq!(fake.release_calls(), 1);
    }

    #[test]
    fn skip_or_cancel_before_scanning_never_calls_core() {
        let _com = ComApartment::new();
        for plan in [NextStreamPlan::Skipped, NextStreamPlan::Cancelled] {
            let mut fake = FakeTransferCallback::new();
            fake.set_next_stream(plan);
            let mut cb = callback(&mut fake);
            let (result, health) = transfer(
                &mut cb,
                request(),
                2,
                &AtomicBool::new(false),
                "Flatbed",
                "Root\\Flatbed",
                |_| panic!("must not start scan"),
            );
            match plan {
                NextStreamPlan::Cancelled => {
                    assert!(matches!(result.unwrap(), TransferOutcome::Cancelled))
                }
                NextStreamPlan::Skipped => {
                    assert!(matches!(result.unwrap(), TransferOutcome::Skipped))
                }
                _ => unreachable!(),
            }
            assert_eq!(health, SessionHealth::Ready);
            assert!(fake.messages().is_empty());
        }
    }

    #[test]
    fn initial_status_cancel_and_nonempty_stream_do_not_start_scan() {
        use std::io::Write;
        let _com = ComApartment::new();
        for nonempty in [false, true] {
            let mut fake = FakeTransferCallback::new();
            if !nonempty {
                fake.set_send_plan(SendMessagePlan::CancelAt(1));
            }
            let mut cb = callback(&mut fake);
            if nonempty {
                let NextStream::Stream(mut stream) =
                    cb.next_stream("Flatbed", "Root\\Flatbed").unwrap()
                else {
                    panic!("expected stream")
                };
                stream.write_all(b"keep").unwrap();
            }
            let (result, health) = transfer(
                &mut cb,
                request(),
                2,
                &AtomicBool::new(false),
                "Flatbed",
                "Root\\Flatbed",
                |_| panic!("must not start scan"),
            );
            assert_eq!(health, SessionHealth::Ready);
            if nonempty {
                assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
                assert_eq!(fake.stream_bytes().unwrap(), b"keep");
            } else {
                assert!(matches!(result.unwrap(), TransferOutcome::Cancelled));
                assert!(fake.stream_bytes().unwrap().is_empty());
            }
            assert_eq!(fake.stream_reference_count(), 1);
        }
    }

    #[test]
    fn callback_cancel_requires_confirmed_cleanup_and_never_finishes_bmp() {
        let _com = ComApartment::new();
        for health in [SessionHealth::Ready, SessionHealth::NeedsReconnect] {
            let mut fake = FakeTransferCallback::new();
            fake.set_send_plan(SendMessagePlan::CancelAt(2));
            let mut cb = callback(&mut fake);
            let (result, actual_health) = transfer(
                &mut cb,
                request(),
                2,
                &AtomicBool::new(false),
                "Flatbed",
                "Root\\Flatbed",
                |sink| {
                    assert_eq!(
                        sink(&band()).unwrap_err().kind(),
                        io::ErrorKind::Interrupted
                    );
                    (Err(io::Error::other("synthetic cleanup result")), health)
                },
            );
            assert_eq!(actual_health, health);
            if health == SessionHealth::Ready {
                assert!(matches!(result.unwrap(), TransferOutcome::Cancelled));
            } else {
                assert!(
                    result
                        .unwrap_err()
                        .to_string()
                        .contains("synthetic cleanup result")
                );
            }
            assert_ne!(&fake.stream_bytes().unwrap()[..2], b"BM");
            assert!(fake.messages().iter().all(|m| m.percent < 100));
        }
    }

    #[test]
    fn callback_failure_preserves_hresult_through_core_diagnostics() {
        let _com = ComApartment::new();
        let mut fake = FakeTransferCallback::new();
        let hresult = 0x80070005u32 as i32;
        fake.set_send_plan(SendMessagePlan::ErrorAt(2, hresult));
        let mut cb = callback(&mut fake);
        let (result, health) = transfer(
            &mut cb,
            request(),
            2,
            &AtomicBool::new(false),
            "Flatbed",
            "Root\\Flatbed",
            |sink| {
                let error = sink(&band()).unwrap_err();
                (
                    Err(io::Error::other(format!("core wrapped: {error}"))),
                    SessionHealth::Ready,
                )
            },
        );
        assert_eq!(health, SessionHealth::Ready);
        let error = result.unwrap_err();
        let detail = error
            .get_ref()
            .unwrap()
            .downcast_ref::<TransferError>()
            .unwrap();
        assert_eq!(
            detail
                .original()
                .get_ref()
                .unwrap()
                .downcast_ref::<crate::wia_callback::CallbackError>()
                .unwrap()
                .hresult(),
            hresult
        );
        assert!(detail.to_string().contains("core wrapped"));
        assert_ne!(&fake.stream_bytes().unwrap()[..2], b"BM");
    }

    #[test]
    fn cleanup_failure_suppresses_signature_and_completion() {
        let _com = ComApartment::new();
        let mut fake = FakeTransferCallback::new();
        let mut cb = callback(&mut fake);
        let (result, health) = transfer(
            &mut cb,
            request(),
            2,
            &AtomicBool::new(false),
            "Flatbed",
            "Root\\Flatbed",
            |sink| {
                sink(&band()).unwrap();
                (
                    Err(io::Error::other("synthetic RELEASE failure")),
                    SessionHealth::NeedsReconnect,
                )
            },
        );
        assert_eq!(health, SessionHealth::NeedsReconnect);
        assert!(result.unwrap_err().to_string().contains("RELEASE"));
        assert_ne!(&fake.stream_bytes().unwrap()[..2], b"BM");
        assert!(fake.messages().iter().all(|m| m.percent < 100));
    }

    #[test]
    fn last_status_cancel_does_not_publish_even_a_complete_bitmap() {
        let _com = ComApartment::new();
        let mut fake = FakeTransferCallback::new();
        fake.set_send_plan(SendMessagePlan::CancelAt(4));
        let mut cb = callback(&mut fake);
        let (result, health) = transfer(
            &mut cb,
            request(),
            2,
            &AtomicBool::new(false),
            "Flatbed",
            "Root\\Flatbed",
            |sink| {
                sink(&band()).unwrap();
                sink(&band()).unwrap();
                (Ok(summary()), SessionHealth::Ready)
            },
        );
        assert_eq!(health, SessionHealth::Ready);
        assert!(matches!(result.unwrap(), TransferOutcome::Cancelled));
        assert_eq!(&fake.stream_bytes().unwrap()[..2], b"BM");
    }
}
