//! Per-session logging (M3).
//!
//! One file per session: `logs/<connection>_<YYYYMMDD-HHMMSS>.log` next to the exe.
//!
//! The default log is plain text built in the frontend from what the terminal
//! DISPLAYS (`src/lib/logCapture.ts`), not from the bytes of the PTY. ConPTY keeps
//! its own copy of the screen and re-sends it (full repaints on resize and scroll,
//! partial-line updates with cursor jumps). A log made by stripping the escape
//! codes from that stream repeats the same screen many times and leaves text
//! fragments, and it cannot be repaired afterwards because the cursor movements
//! that explain the redraws are gone.
//!
//! `LogMode::Raw` keeps the old behaviour (bytes as received) for diagnostics.
//! Typed input is never logged: commands already appear in the screen log next to
//! the prompt, and a password typed at a prompt is not echoed, so it never shows.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};

use crate::config::{Config, LogMode};
use crate::paths;

/// Replaces characters that are illegal or awkward in Windows file names.
fn sanitize(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    // Windows does not allow names ending in a dot or a space.
    let trimmed = cleaned.trim().trim_end_matches('.').chars().take(80).collect::<String>();
    if trimmed.is_empty() { "session".to_string() } else { trimmed }
}

fn stamp(now: DateTime<Local>) -> String {
    now.format("%Y%m%d-%H%M%S").to_string()
}

pub struct SessionLogger {
    file: Option<File>,
    mode: LogMode,
}

impl SessionLogger {
    /// Opens the per-session log file. Any failure is swallowed: a logging problem
    /// must never break a session.
    pub fn open(connection_name: &str, host: &str, user: &str, cfg: &Config) -> Self {
        Self::open_in(&paths::logs_dir(), connection_name, host, user, cfg)
    }

    pub fn open_in(dir: &Path, connection_name: &str, host: &str, user: &str, cfg: &Config) -> Self {
        let mut logger = SessionLogger { file: None, mode: cfg.log_mode };
        if cfg.log_output {
            logger.file = create_file(dir, connection_name, host, user);
        }
        logger
    }

    /// Plain text produced from the terminal screen (`LogMode::Screen`).
    pub fn write_text(&mut self, text: &str) {
        if self.mode != LogMode::Screen {
            return;
        }
        self.write(text.as_bytes());
    }

    /// PTY bytes exactly as received (`LogMode::Raw`).
    pub fn write_raw(&mut self, bytes: &[u8]) {
        if self.mode != LogMode::Raw {
            return;
        }
        self.write(bytes);
    }

    /// Writes the closing line and releases the file.
    pub fn finish(&mut self) {
        if self.file.is_some() {
            let line = format!("---- ended: {}\n", Local::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, false));
            self.write(line.as_bytes());
        }
        self.file = None;
    }

    fn write(&mut self, bytes: &[u8]) {
        if let Some(f) = self.file.as_mut() {
            let _ = f.write_all(bytes);
            let _ = f.flush();
        }
    }
}

/// Creates a new file. Two sessions of the same connection that start within the
/// same second get `_2`, `_3`... instead of sharing (and mixing) one file.
fn create_file(dir: &Path, connection_name: &str, host: &str, user: &str) -> Option<File> {
    std::fs::create_dir_all(dir).ok()?;
    let now = Local::now();
    let base = format!("{}_{}", sanitize(connection_name), stamp(now));
    let mut path: PathBuf = dir.join(format!("{base}.log"));
    for n in 2..100 {
        match OpenOptions::new().create_new(true).append(true).open(&path) {
            Ok(mut f) => {
                let _ = write!(
                    f,
                    "sshterm session log\nconnection: {connection_name}\nuser: {user}\nhost: {host}\nstarted: {}\n----\n",
                    now.to_rfc3339_opts(chrono::SecondsFormat::Secs, false)
                );
                return Some(f);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                path = dir.join(format!("{base}_{n}.log"));
            }
            Err(_) => return None,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    fn cfg(mode: LogMode) -> Config {
        Config { log_mode: mode, ..Config::default() }
    }

    fn only_log(dir: &Path) -> String {
        let mut files: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
        files.sort();
        assert_eq!(files.len(), 1, "expected exactly one log file, got {files:?}");
        fs::read_to_string(&files[0]).unwrap()
    }

    #[test]
    fn sanitize_replaces_illegal_characters() {
        assert_eq!(sanitize("a/b:c*d?e\"f<g>h|i\\j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize("normal-name"), "normal-name");
        assert_eq!(sanitize("  name. "), "name");
        assert_eq!(sanitize("..."), "session");
        assert_eq!(sanitize("").as_str(), "session");
        assert_eq!(sanitize(&"x".repeat(200)).len(), 80);
    }

    #[test]
    fn file_name_has_a_readable_timestamp() {
        let dir = tempdir().unwrap();
        let _ = SessionLogger::open_in(dir.path(), "server", "h", "u", &cfg(LogMode::Screen));
        let name = fs::read_dir(dir.path()).unwrap().next().unwrap().unwrap().file_name();
        let name = name.to_string_lossy().into_owned();
        // server_YYYYMMDD-HHMMSS.log
        let re_ok = name.starts_with("server_")
            && name.ends_with(".log")
            && name.len() == "server_20260101-120000.log".len()
            && name.as_bytes()["server_".len() + 8] == b'-';
        assert!(re_ok, "unexpected file name {name}");
    }

    #[test]
    fn screen_mode_writes_text_and_ignores_raw_bytes() {
        let dir = tempdir().unwrap();
        let mut l = SessionLogger::open_in(dir.path(), "srv", "10.0.0.5", "olfo", &cfg(LogMode::Screen));
        l.write_raw(b"\x1b[2J garbage that must not appear");
        l.write_text("line one\nline two\n");
        l.finish();
        let log = only_log(dir.path());
        assert!(log.starts_with("sshterm session log\nconnection: srv\nuser: olfo\nhost: 10.0.0.5\nstarted: "));
        assert!(log.contains("----\nline one\nline two\n---- ended: "));
        assert!(!log.contains("garbage"));
    }

    #[test]
    fn raw_mode_writes_bytes_and_ignores_screen_text() {
        let dir = tempdir().unwrap();
        let mut l = SessionLogger::open_in(dir.path(), "srv", "u", "h", &cfg(LogMode::Raw));
        l.write_text("screen text must not appear");
        l.write_raw(b"raw bytes");
        let log = only_log(dir.path());
        assert!(log.contains("raw bytes"));
        assert!(!log.contains("screen text"));
    }

    #[test]
    fn log_output_false_creates_no_file() {
        let dir = tempdir().unwrap();
        let mut c = cfg(LogMode::Screen);
        c.log_output = false;
        let mut l = SessionLogger::open_in(dir.path(), "srv", "u", "h", &c);
        l.write_text("x");
        l.finish();
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn two_sessions_in_the_same_second_do_not_share_a_file() {
        let dir = tempdir().unwrap();
        let mut a = SessionLogger::open_in(dir.path(), "srv", "u", "h", &cfg(LogMode::Screen));
        let mut b = SessionLogger::open_in(dir.path(), "srv", "u", "h", &cfg(LogMode::Screen));
        a.write_text("from a\n");
        b.write_text("from b\n");
        let mut contents: Vec<String> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        contents.sort();
        assert_eq!(contents.len(), 2);
        assert!(contents.iter().any(|c| c.contains("from a") && !c.contains("from b")));
        assert!(contents.iter().any(|c| c.contains("from b") && !c.contains("from a")));
    }
}
