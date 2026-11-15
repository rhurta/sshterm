//! `~/.ssh/config` importer (M4, step 3a).
//!
//! Only the keywords the app actually uses are read: Host, HostName, Port,
//! User and IdentityFile. Wildcard-only hosts and `Match` blocks are reported
//! as skipped with a reason, because their effective values depend on the
//! destination and cannot be represented as a single saved connection.

use std::path::{Path, PathBuf};

use crate::vault::Auth;

use super::{expand_path, default_user, home_dir, ImportCandidate, ImportPreview};

struct Block {
    label: String,
    alias: String,
    host: String,
    port: u16,
    user: String,
    key_file: Option<String>,
    problem: Option<&'static str>,
}

pub fn parse(path: Option<&str>) -> Result<ImportPreview, &'static str> {
    let file = match path {
        Some(p) if !p.trim().is_empty() => PathBuf::from(p),
        _ => home_dir()?.join(".ssh").join("config"),
    };
    let text = std::fs::read_to_string(&file).map_err(|_| "import_read_failed")?;
    Ok(parse_text(&text, &file))
}

fn split_kv(line: &str) -> (&str, String) {
    let mut parts = line.trim().splitn(2, |c: char| c.is_whitespace() || c == '=');
    let key = parts.next().unwrap_or("").trim();
    let value = parts.next().unwrap_or("").trim().trim_matches('"').to_string();
    (key, value)
}

pub fn parse_text(text: &str, file: &Path) -> ImportPreview {
    let base = file.parent().unwrap_or_else(|| Path::new("."));
    let mut out = ImportPreview::new("ssh_config");
    let mut blocks: Vec<Block> = Vec::new();
    let mut current: Option<Block> = None;
    let mut in_match = false;

    for (index, raw) in text.lines().enumerate() {
        let line_no = index + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = split_kv(line);
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                if let Some(block) = current.take() {
                    blocks.push(block);
                }
                in_match = false;
                let alias = value.split_whitespace().next().unwrap_or("").to_string();
                let mut problem = None;
                if alias.is_empty() {
                    problem = Some("missing_host");
                } else if value.split_whitespace().any(|p| p.contains('*') || p.contains('?')) {
                    problem = Some("wildcard_host");
                }
                current = Some(Block {
                    label: format!("Host {alias} (line {line_no})"),
                    alias,
                    host: String::new(),
                    port: 22,
                    user: String::new(),
                    key_file: None,
                    problem,
                });
            }
            "match" => {
                if let Some(block) = current.take() {
                    blocks.push(block);
                }
                in_match = true;
                out.skip(format!("Match (line {line_no})"), "unsupported_match");
            }
            "include" => out.notices.push("include_ignored".to_string()),
            _ => {
                if in_match {
                    continue;
                }
                if let Some(block) = current.as_mut() {
                    match key.to_ascii_lowercase().as_str() {
                        "hostname" => block.host = value.clone(),
                        "port" => match value.parse::<u16>() {
                            Ok(p) if p > 0 => block.port = p,
                            _ => block.problem = Some("bad_port"),
                        },
                        "user" => block.user = value.clone(),
                        "identityfile" => block.key_file = Some(value.clone()),
                        _ => {}
                    }
                }
            }
        }
    }
    if let Some(block) = current.take() {
        blocks.push(block);
    }

    for block in blocks {
        if let Some(reason) = block.problem {
            out.skip(block.label, reason);
            continue;
        }
        let host = if block.host.is_empty() { block.alias.clone() } else { block.host };
        if host.is_empty() {
            out.skip(block.label, "missing_host");
            continue;
        }
        let user = if block.user.is_empty() { default_user() } else { block.user };
        let auth = match block.key_file {
            Some(path) => Auth::Key { path: expand_path(&path, base), passphrase: None },
            None => Auth::Agent,
        };
        out.candidates.push(ImportCandidate {
            name: block.alias,
            host,
            port: block.port,
            user,
            auth,
            label: block.label,
            folder_path: Vec::new(),
            warnings: Vec::new(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"
# sample ssh config
Host *
    ServerAliveInterval 30

Host pgw-01
    HostName 10.0.0.5
    User admin
    Port 2222

Host prod-db-01
    HostName db.internal
    User dba
    IdentityFile ~/.ssh/id_ed25519

Host legacy
    HostName 10.0.0.9

Host bad-port
    HostName 10.0.0.10
    Port nope

Match host *.example.com
    ProxyJump bastion
"#;

    #[test]
    fn parses_hosts_and_reports_skips() {
        let file = Path::new("C:/Users/me/.ssh/config");
        let preview = parse_text(FIXTURE, file);
        assert_eq!(preview.candidates.len(), 3);
        assert_eq!(preview.candidates[0].name, "pgw-01");
        assert_eq!(preview.candidates[0].host, "10.0.0.5");
        assert_eq!(preview.candidates[0].port, 2222);
        assert_eq!(preview.candidates[0].user, "admin");
        assert!(matches!(preview.candidates[1].auth, Auth::Key { .. }));
        assert_eq!(preview.candidates[2].user, super::super::default_user());
        assert!(preview.skipped.iter().any(|s| s.reason == "wildcard_host"));
        assert!(preview.skipped.iter().any(|s| s.reason == "bad_port"));
        assert!(preview.skipped.iter().any(|s| s.reason == "unsupported_match"));
    }

    #[test]
    fn identity_file_paths_are_expanded() {
        let file = Path::new("C:/Users/me/.ssh/config");
        let preview = parse_text(FIXTURE, file);
        match &preview.candidates[1].auth {
            Auth::Key { path, .. } => assert!(path.ends_with("id_ed25519")),
            _ => panic!("expected key auth"),
        }
    }
}
