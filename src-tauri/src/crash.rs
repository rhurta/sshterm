//! Crash log for release builds.
//!
//! The release profile uses `panic = "abort"` and the exe is a GUI-subsystem
//! binary, so a panic in any thread closes the app with no message. This hook
//! appends the panic message, thread, location and a backtrace to
//! `logs/crash.log` next to the exe before the process aborts.
//!
//! It cannot capture native faults (for example inside WebView2).

use std::backtrace::Backtrace;
use std::io::Write;
use std::panic::PanicHookInfo;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Installs the panic hook. Call once, as early as possible in `run()`.
pub fn install() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let backtrace = Backtrace::force_capture();
        let report = format_report(info, &backtrace.to_string());
        let dir = crate::paths::logs_dir();
        let _ = std::fs::create_dir_all(&dir);
        append_report(&dir.join("crash.log"), &report);
        previous(info);
    }));
}

fn format_report(info: &PanicHookInfo<'_>, backtrace: &str) -> String {
    let payload = info.payload();
    let message = payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "<non-string panic payload>".to_string());
    let location = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "<unknown location>".to_string());
    let thread = std::thread::current();
    build_report(
        unix_seconds(),
        thread.name().unwrap_or("<unnamed>"),
        &location,
        &message,
        backtrace,
    )
}

fn build_report(unix_secs: u64, thread: &str, location: &str, message: &str, backtrace: &str) -> String {
    format!(
        "=== panic (unix time {unix_secs}, sshterm {}) ===\nthread:   {thread}\nlocation: {location}\nmessage:  {message}\nbacktrace:\n{backtrace}\n\n",
        env!("CARGO_PKG_VERSION"),
    )
}

fn unix_seconds() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn append_report(path: &Path, report: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = file.write_all(report.as_bytes());
        let _ = file.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_contains_thread_location_and_message() {
        let report = build_report(42, "tunnel-monitor", "src/x.rs:1:2", "boom", "frame 0");
        assert!(report.contains("thread:   tunnel-monitor"));
        assert!(report.contains("location: src/x.rs:1:2"));
        assert!(report.contains("message:  boom"));
        assert!(report.contains("frame 0"));
    }

    #[test]
    fn reports_are_appended_not_overwritten() {
        let dir = std::env::temp_dir().join(format!("sshterm-crash-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("crash.log");
        append_report(&path, "first\n");
        append_report(&path, "second\n");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "first\nsecond\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hook_records_a_real_panic() {
        // Run in a separate thread so the panic is caught by join().
        let result = std::thread::Builder::new()
            .name("crash-test".into())
            .spawn(|| {
                let captured = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
                let sink = captured.clone();
                let prev = std::panic::take_hook();
                std::panic::set_hook(Box::new(move |info| {
                    *sink.lock().unwrap() = format_report(info, "bt");
                }));
                let _ = std::panic::catch_unwind(|| panic!("test panic"));
                std::panic::set_hook(prev);
                let text = captured.lock().unwrap().clone();
                text
            })
            .unwrap()
            .join()
            .unwrap();
        assert!(result.contains("message:  test panic"));
        assert!(result.contains("thread:   crash-test"));
    }
}
