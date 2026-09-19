//! Minimal failure trace for the in-service driver.
//!
//! The WIA service hosts this DLL as LocalService with no console, so a Rust
//! panic contained by `catch_hresult` would otherwise leave only an opaque
//! E_UNEXPECTED in wiatrace.log. The hook appends one line per panic to
//! `%SystemRoot%\debug\WIA\wc3119-driver.log`, the directory the WIA service
//! itself writes to. Every failure to write is ignored: tracing must never
//! change the COM result or raise a second panic.

use std::{
    io::Write,
    panic::{self, PanicHookInfo},
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};

static HOOK: OnceLock<()> = OnceLock::new();

/// Install the panic hook once per process, chaining to the previous hook so
/// test harnesses keep their own panic output.
pub(super) fn install_panic_hook() {
    HOOK.get_or_init(|| {
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |info: &PanicHookInfo<'_>| {
            record_panic(info);
            previous(info);
        }));
    });
}

fn record_panic(info: &PanicHookInfo<'_>) {
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|text| (*text).to_owned())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned());
    let location = info
        .location()
        .map(|location| format!("{}:{}", location.file(), location.line()))
        .unwrap_or_else(|| "unknown location".to_owned());
    append(&format!("panic at {location}: {message}"));
}

/// Append one line with a coarse timestamp; silently does nothing on error.
pub(super) fn append(text: &str) {
    // Test processes launched by cargo contain many deliberate synthetic
    // panics; keep them out of the machine-wide service log.
    if std::env::var_os("CARGO_MANIFEST_DIR").is_some() {
        return;
    }
    let Some(root) = std::env::var_os("SystemRoot") else {
        return;
    };
    let path = std::path::PathBuf::from(root).join(r"debug\WIA\wc3119-driver.log");
    let Ok(mut file) = std::fs::OpenOptions::new()
        .append(true)
        .create(true)
        .open(path)
    else {
        return;
    };
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    let pid = std::process::id();
    let _ = writeln!(file, "{seconds} pid={pid} {text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_installs_once_and_keeps_the_previous_hook() {
        install_panic_hook();
        install_panic_hook();
        let seen = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = seen.clone();
        let previous = panic::take_hook();
        panic::set_hook(Box::new(move |_| {
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        }));
        let result = panic::catch_unwind(|| panic!("synthetic panic for trace test"));
        panic::set_hook(previous);
        assert!(result.is_err());
        assert!(seen.load(std::sync::atomic::Ordering::SeqCst));
    }
}
