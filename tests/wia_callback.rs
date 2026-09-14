#![cfg(windows)]

#[path = "support/wia_callback.rs"]
mod support;

use std::{
    cell::Cell,
    io::{self, Write},
    ptr,
    rc::Rc,
};

use support::{FakeTransferCallback, NextStreamPlan, SendMessagePlan};
use workcentre_3119::wia_callback::{CallbackError, CallbackStatus, NextStream, TransferCallback};

const E_POINTER: i32 = 0x8000_4003u32 as i32;
const E_ACCESSDENIED: i32 = 0x8007_0005u32 as i32;
const E_INVALIDARG: i32 = 0x8007_0057u32 as i32;
const WIA_STATUS_SKIP_ITEM: i32 = 0x0021_0009;

fn callback(fake: &mut FakeTransferCallback) -> TransferCallback {
    // SAFETY: the fixture owns a live callback object and transfers only the QI reference
    // acquired by the adapter; the borrowed fixture remains alive until adapter drop.
    unsafe { TransferCallback::query_from_borrowed(fake.as_raw()).unwrap() }
}

fn callback_hresult(error: &io::Error) -> i32 {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<CallbackError>())
        .expect("callback HRESULT error")
        .hresult()
}

#[test]
fn null_borrowed_pointer_is_rejected_without_dereference() {
    // SAFETY: null is explicitly rejected before reading a COM vtable.
    let error = unsafe { TransferCallback::query_from_borrowed(ptr::null_mut()) }.unwrap_err();
    assert_eq!(callback_hresult(&error), E_POINTER);
}

#[test]
fn qi_owns_and_releases_exactly_one_reference() {
    let mut fake = FakeTransferCallback::new();
    assert_eq!(fake.reference_count(), 1);
    let callback = callback(&mut fake);
    assert_eq!(fake.reference_count(), 2);
    drop(callback);
    assert_eq!(fake.reference_count(), 1);
    assert_eq!(fake.release_calls(), 1);
}

#[test]
fn fixture_hook_runs_at_each_external_callback_entry() {
    let mut fake = FakeTransferCallback::new();
    let entries = Rc::new(Cell::new(0usize));
    let observed = Rc::clone(&entries);
    fake.set_hook(Box::new(move || observed.set(observed.get() + 1)));
    fake.set_next_stream(NextStreamPlan::Cancelled);
    let mut callback = callback(&mut fake);
    assert!(matches!(
        callback.next_stream("Flatbed", "\\Root\\Flatbed").unwrap(),
        NextStream::Cancelled
    ));
    assert_eq!(callback.status(1, 2).unwrap(), CallbackStatus::Continue);
    drop(callback);
    assert_eq!(entries.get(), 4);
}

#[test]
fn qi_failure_preserves_hresult_and_does_not_release_borrowed_reference() {
    let mut fake = FakeTransferCallback::new();
    fake.set_query_result(E_ACCESSDENIED);
    // SAFETY: the fixture remains alive and exposes a valid IUnknown vtable.
    let error = unsafe { TransferCallback::query_from_borrowed(fake.as_raw()) }.unwrap_err();
    assert_eq!(callback_hresult(&error), E_ACCESSDENIED);
    assert_eq!(fake.reference_count(), 1);
    assert_eq!(fake.release_calls(), 0);
}

#[test]
fn unexpected_positive_qi_status_is_an_error() {
    let mut fake = FakeTransferCallback::new();
    fake.set_query_result(2);
    // SAFETY: the fixture remains alive and exposes a valid IUnknown vtable.
    let error = unsafe { TransferCallback::query_from_borrowed(fake.as_raw()) }.unwrap_err();
    assert_eq!(callback_hresult(&error), 2);
    assert_eq!(fake.reference_count(), 1);
}

#[test]
fn next_stream_writes_real_hglobal_stream_and_passes_bstr_names() {
    let _apartment = support::ComApartment::new();
    let mut fake = FakeTransferCallback::new();
    fake.set_next_stream(NextStreamPlan::Stream);
    let mut callback = callback(&mut fake);

    let mut stream = match callback.next_stream("Flatbed", "\\Root\\Flatbed").unwrap() {
        NextStream::Stream(stream) => stream,
        other => panic!("expected stream, got {other:?}"),
    };
    stream.write_all(b"wia-callback").unwrap();
    drop(stream);

    assert_eq!(fake.last_item_name(), Some("Flatbed"));
    assert_eq!(fake.last_full_item_name(), Some("\\Root\\Flatbed"));
    assert_eq!(fake.stream_bytes().unwrap(), b"wia-callback");
    assert_eq!(fake.next_stream_calls(), 1);
    assert_eq!(fake.stream_reference_count(), 1);
}

#[test]
fn next_stream_s_false_is_cancelled() {
    let mut fake = FakeTransferCallback::new();
    fake.set_next_stream(NextStreamPlan::Cancelled);
    let mut callback = callback(&mut fake);
    assert!(matches!(
        callback.next_stream("Flatbed", "\\Root\\Flatbed").unwrap(),
        NextStream::Cancelled
    ));
    assert_eq!(fake.next_stream_calls(), 1);
}

#[test]
fn next_stream_skip_is_distinct_from_cancellation() {
    let mut fake = FakeTransferCallback::new();
    fake.set_next_stream(NextStreamPlan::Skipped);
    let mut callback = callback(&mut fake);
    assert!(matches!(
        callback.next_stream("Flatbed", "\\Root\\Flatbed").unwrap(),
        NextStream::Skipped
    ));
}

#[test]
fn next_stream_null_success_is_rejected() {
    let mut fake = FakeTransferCallback::new();
    fake.set_next_stream(NextStreamPlan::NullSuccess);
    let mut callback = callback(&mut fake);
    let error = callback
        .next_stream("Flatbed", "\\Root\\Flatbed")
        .unwrap_err();
    assert_eq!(callback_hresult(&error), E_POINTER);
}

#[test]
fn next_stream_error_preserves_hresult() {
    let mut fake = FakeTransferCallback::new();
    fake.set_next_stream(NextStreamPlan::Error(E_ACCESSDENIED));
    let mut callback = callback(&mut fake);
    let error = callback
        .next_stream("Flatbed", "\\Root\\Flatbed")
        .unwrap_err();
    assert_eq!(callback_hresult(&error), E_ACCESSDENIED);
}

#[test]
fn status_sends_progress_message() {
    let mut fake = FakeTransferCallback::new();
    fake.set_send_plan(SendMessagePlan::Continue);
    let mut callback = callback(&mut fake);

    assert_eq!(callback.status(42, 100).unwrap(), CallbackStatus::Continue);

    let messages = fake.messages();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].message, 1);
    assert_eq!(messages[0].percent, 42);
    assert_eq!(messages[0].bytes, 100);
    assert!(messages.iter().all(|message| message.flags == 0));
}

#[test]
fn send_s_false_is_cancelled() {
    let mut fake = FakeTransferCallback::new();
    fake.set_send_plan(SendMessagePlan::CancelAt(1));
    let mut callback = callback(&mut fake);
    assert_eq!(callback.status(1, 2).unwrap(), CallbackStatus::Cancelled);
}

#[test]
fn skip_status_from_send_message_is_not_cancellation() {
    let mut fake = FakeTransferCallback::new();
    fake.set_send_plan(SendMessagePlan::ErrorAt(1, WIA_STATUS_SKIP_ITEM));
    let mut callback = callback(&mut fake);
    let error = callback.status(50, 10).unwrap_err();
    assert_eq!(callback_hresult(&error), WIA_STATUS_SKIP_ITEM);
}

#[test]
fn invalid_percent_is_rejected_before_sending() {
    let mut fake = FakeTransferCallback::new();
    let mut callback = callback(&mut fake);
    let error = callback.status(101, 0).unwrap_err();
    assert_eq!(callback_hresult(&error), E_INVALIDARG);
    assert_eq!(fake.send_message_calls(), 0);
}

#[test]
fn bstr_length_is_bounded_before_allocation() {
    let mut fake = FakeTransferCallback::new();
    let mut callback = callback(&mut fake);
    let too_long = "x".repeat(16 * 1024 + 1);
    let error = callback.next_stream(&too_long, "full").unwrap_err();
    assert_eq!(callback_hresult(&error), E_INVALIDARG);
    assert_eq!(fake.next_stream_calls(), 0);
}
