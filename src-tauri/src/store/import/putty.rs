//! PuTTY registry importer (M4, step 3b). Windows only: PuTTY stores its
//! sessions under HKCU\Software\SimonTatham\PuTTY\Sessions.
//!
//! Saved PuTTY passwords are encrypted with the Windows user account and are
//! deliberately NOT imported. The session is still created, with a warning in
//! the preview so the user can set its password afterwards.

use winreg::enums::HKEY_CURRENT_USER;
use winreg::RegKey;

use crate::vault::Auth;

use super::{default_user, ImportCandidate, ImportPreview};

const SESSIONS_KEY: &str = r"Software\SimonTatham\PuTTY\Sessions";

pub fn parse() -> Result<ImportPreview, &'static str> {
    let mut out = ImportPreview::new("putty");
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let sessions = match hkcu.open_subkey(SESSIONS_KEY) {
        Ok(key) => key,
        Err(_) => {
            out.skip("PuTTY sessions", "putty_not_found");
            return Ok(out);
        }
    };

    for encoded in sessions.enum_keys().filter_map(Result::ok) {
        let name = decode_percent(&encoded);
        let key = match sessions.open_subkey(&encoded) {
            Ok(key) => key,
            Err(_) => continue,
        };
        let host: String = key.get_value("HostName").unwrap_or_default();
        if host.trim().is_empty() {
            out.skip(name, "missing_host");
            continue;
        }
        let protocol: String = key.get_value("Protocol").unwrap_or_else(|_| "ssh".into());
        if !protocol.eq_ignore_ascii_case("ssh") {
            out.skip(name, "not_ssh");
            continue;
        }
        let port: u32 = key.get_value("PortNumber").unwrap_or(22);
        let port = u16::try_from(port).unwrap_or(22);
        let user: String = key.get_value("UserName").unwrap_or_default();
        let user = if user.trim().is_empty() { default_user() } else { user };
        let key_file: String = key.get_value("PublicKeyFile").unwrap_or_default();

        let mut warnings = Vec::new();
        let saved_password: String = key.get_value("Password").unwrap_or_default();
        if !saved_password.is_empty() {
            warnings.push("putty_password_not_imported".into());
        }
        let auth = if key_file.trim().is_empty() {
            Auth::Agent
        } else {
            Auth::Key { path: key_file, passphrase: None }
        };
        out.candidates.push(ImportCandidate {
            label: name.clone(),
            name,
            host,
            port,
            user,
            auth,
            folder_path: Vec::new(),
            warnings,
        });
    }
    Ok(out)
}

/// PuTTY percent-encodes session names in the registry (spaces become %20).
pub fn decode_percent(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (hex_value(bytes[i + 1]), hex_value(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_names_are_percent_decoded() {
        assert_eq!(decode_percent("prod%20db%2D01"), "prod db-01");
        assert_eq!(decode_percent("plain%ZZ"), "plain%ZZ");
    }
}
