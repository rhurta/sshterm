//! Per-session terminal logging (M3).
//!
//! One file per session, named `logs/<connection>_<timestamp>.log` next to the
//! exe. Only terminal bytes are written: the header plus the bytes that flow
//! through the PTY. The metadata diagnostics in `session/mod.rs::log_line`
//! (which never logs secrets or terminal contents) stay in `logs/sshterm.log`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::Config;
use crate::paths;

/// Replaces characters that are illegal or awkward in Windows file names.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect()
}

/// Minimal ANSI/escape stripper for log files. It removes CSI (`ESC [` ...),
/// OSC (`ESC ]` ... BEL) and other two-byte escapes. It is not a full terminal
/// emulator, only enough to keep the log readable.
fn strip_ansi_sequences(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    let mut i = 0;
    while i < input.len() {
        let b = input[i];
        if b == 0x1b {
            // An escape sequence starts here.
            if i + 1 >= input.len() {
                break;
            }
            let next = input[i + 1];
            if next == b'[' {
                // CSI: ESC [ params... final byte (0x40..=0x7e)
                i += 2;
                while i < input.len() && !(0x40..=0x7e).contains(&input[i]) {
                    i += 1;
                }
                if i < input.len() {
                    i += 1; // skip the final byte
                }
                continue;
            } else if next == b']' {
                // OSC: ESC ] ... BEL (0x07) or ST (ESC \)
                i += 2;
                while i < input.len() && input[i] != 0x07 {
                    i += 1;
                }
                if i < input.len() {
                    i += 1; // skip the BEL
                }
                continue;
            } else {
                // Other escapes (e.g. ESC ( B): consume ESC and the next byte.
                i += 2;
                continue;
            }
        }
        out.push(b);
        i += 1;
    }
    out
}

/// Removes stray control characters that make a plain-text log hard to read,
/// while keeping the text itself. It drops carriage returns, bell, backspace
/// and the rest of the C0 controls, plus DEL (0x7f). The line feed (0x0a) and
/// horizontal tab (0x09) are kept so the log keeps its line breaks and
/// indentation. Printable ASCII and UTF-8 (including multibyte) are preserved.
fn strip_control_chars(input: &[u8]) -> Vec<u8> {
    input
        .iter()
        .copied()
        .filter(|&b| b == b'\n' || b == b'\t' || (b >= 0x20 && b != 0x7f))
        .collect()
}

/// Collapses runs of consecutive blank lines into a single blank line so a log
/// the remote host padded with many newlines (e.g. a centered banner) stays
/// readable. A run of N newlines becomes at most two (one blank line).
fn collapse_blank_lines(input: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(input.len());
    for &b in input {
        if b == b'\n' {
            // Keep at most one blank line: allow a run of two newlines, drop the rest.
            let len = out.len();
            if len >= 2 && out[len - 1] == b'\n' && out[len - 2] == b'\n' {
                continue;
            }
        }
        out.push(b);
    }
    out
}

/// Cleans terminal bytes for the log file: first strips ANSI/escape sequences,
/// then removes other control characters, then collapses runs of blank lines,
/// leaving readable text.
fn clean_for_log(input: &[u8]) -> Vec<u8> {
    let no_ansi = strip_ansi_sequences(input);
    let no_control = strip_control_chars(&no_ansi);
    collapse_blank_lines(&no_control)
}

pub struct SessionLogger {
    file: Option<File>,
    log_input: bool,
    strip_special: bool,
}

impl SessionLogger {
    /// Opens the per-session log file. Any failure is swallowed: a logging
    /// problem must never break a session.
    pub fn open(connection_name: &str, host: &str, user: &str, cfg: &Config) -> Self {
        let mut logger = SessionLogger {
            file: None,
            log_input: cfg.log_input,
            strip_special: cfg.strip_special,
        };
        logger.try_open(connection_name, host, user);
        logger
    }

    fn try_open(&mut self, connection_name: &str, host: &str, user: &str) {
        let dir = paths::logs_dir();
        if fs::create_dir_all(&dir).is_err() {
            return;
        }
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let name = format!("{}_{}.log", sanitize(connection_name), secs);
        let path = dir.join(&name);
        match OpenOptions::new().create(true).append(true).open(&path) {
            Ok(mut f) => {
                let _ = writeln!(f, "sshterm session log");
                let _ = writeln!(f, "connection: {connection_name}");
                let _ = writeln!(f, "user: {user}");
                let _ = writeln!(f, "host: {host}");
                let _ = writeln!(f, "started: {secs}");
                let _ = writeln!(f, "----");
                self.file = Some(f);
            }
            Err(_) => {}
        }
    }

    /// Writes terminal output to the log file (unless `log_output` was off, which
    /// it never is here — the caller only builds a logger when logging is on).
    pub fn log_output(&mut self, bytes: &[u8]) {
        if self.file.is_none() {
            return;
        }
        let data: Vec<u8> = if self.strip_special {
            clean_for_log(bytes)
        } else {
            bytes.to_vec()
        };
        if let Some(f) = self.file.as_mut() {
            let _ = f.write_all(&data);
            let _ = f.flush();
        }
    }

    /// Writes typed input, marked so the log stays readable. Respects the
    /// `log_input` setting (off by default to keep secrets out).
    pub fn log_input(&mut self, bytes: &[u8]) {
        if !self.log_input || self.file.is_none() {
            return;
        }
        if let Some(f) = self.file.as_mut() {
            let _ = write!(f, "\x1b[2m← {}\x1b[0m", String::from_utf8_lossy(bytes));
            let _ = f.flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_replaces_illegal_characters() {
        assert_eq!(sanitize("a/b:c*d?e\"f<g>h|i\\j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize("normal-name"), "normal-name");
    }

    #[test]
    fn ansi_sequences_are_stripped() {
        // ESC[32m is a green color; ESC[0m resets it.
        let input = b"\x1b[32mgreen\x1b[0m text";
        let out = strip_ansi_sequences(input);
        assert_eq!(out, b"green text");
    }

    #[test]
    fn osc_title_sequences_are_stripped() {
        // ESC]0;title BEL
        let input = b"\x1b]0;my title\x07prompt$ ";
        let out = strip_ansi_sequences(input);
        assert_eq!(out, b"prompt$ ");
    }

    #[test]
    fn plain_text_is_untouched() {
        let input = b"hello world\r\n";
        assert_eq!(strip_ansi_sequences(input), input);
    }

    #[test]
    fn control_characters_are_stripped() {
        // CR (0x0d) and BEL (0x07) are removed; LF (0x0a) and printable kept.
        let input = b"line1\r\nline2\x07done";
        assert_eq!(clean_for_log(input), b"line1\nline2done");
    }

    #[test]
    fn special_characters_cleaned_keeps_text() {
        // ANSI color plus a stray CR: both gone, readable text remains.
        let input = b"\x1b[32mok\x1b[0m\r\n";
        assert_eq!(clean_for_log(input), b"ok\n");
    }

    #[test]
    fn blank_lines_collapse_to_one() {
        // Three newlines (two blank lines) become two newlines (one blank line).
        let input = b"line1\n\n\nline2";
        assert_eq!(collapse_blank_lines(input), b"line1\n\nline2");
    }

    #[test]
    fn many_blank_lines_collapse_to_one() {
        // A run of seven newlines keeps a single blank line between the blocks.
        let input = b"head\n\n\n\n\n\n\n\ntail";
        assert_eq!(collapse_blank_lines(input), b"head\n\ntail");
    }
}
