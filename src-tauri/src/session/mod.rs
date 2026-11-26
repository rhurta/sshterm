//! Open SSH sessions (M2).
//!
//! Each session owns a `Transport`. PTY bytes are published as `session:data`
//! events (base64, to avoid splitting UTF-8 sequences across the JSON bridge);
//! when the PTY reaches EOF, `session:closed` is emitted.

use std::collections::HashMap;
use std::sync::Arc;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use rand::{rngs::OsRng, RngCore};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::Mutex;

use crate::askpass::{AskpassCredential, AskpassServer};
use crate::commands::VaultState;
use crate::config::Config;
use crate::logger::SessionLogger;
use crate::paths;
use crate::store;
use crate::transport::{openssh::OpenSshTransport, JumpHost, LocalShellConfig, PtySize, SshConfig, Transport};

#[derive(Default)]
pub struct SessionState {
    pub transports: Mutex<HashMap<String, Box<dyn Transport>>>,
    pub loggers: Mutex<HashMap<String, Arc<Mutex<SessionLogger>>>>,
}

impl Drop for SessionState {
    fn drop(&mut self) {
        // On app exit, each Transport kills its ssh.exe in its Drop.
        self.transports.get_mut().clear();
    }
}

#[derive(Clone, Serialize)]
struct DataEvent {
    id: String,
    data: String,
}

#[derive(Clone, Serialize)]
struct ClosedEvent {
    id: String,
}

#[derive(Clone, Serialize)]
struct SessionErrorEvent {
    id: String,
    message: String,
}

/// Returns a useful SSH diagnostic line for connection, jump-host, or
/// forwarding setup failures. PTY output is still sent unchanged to xterm;
/// this second channel makes early failures visible in the workspace as well.
pub(crate) fn ssh_failure_message(line: &str) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let patterns = [
        "stdio forwarding failed",
        "could not request local forwarding",
        "remote port forwarding failed",
        "channel_setup_fwd_listener_tcpip",
        "cannot listen to port",
        "address already in use",
        "open failed: administratively prohibited",
        "connect failed",
        "could not resolve hostname",
        "connection timed out",
        "operation timed out",
        "connection refused",
        "no route to host",
        "permission denied",
        "connection closed by",
        "host key verification failed",
        "remote host identification has changed",
        "failed to add the host to the list of known hosts",
        "could not add host to known hosts",
    ];
    if patterns.iter().any(|pattern| lower.contains(pattern)) {
        let message = line
            .chars()
            .filter(|ch| !ch.is_control())
            .take(500)
            .collect::<String>();
        let message = message.trim().to_string();
        if !message.is_empty() {
            return Some(message);
        }
    }
    None
}

fn new_id() -> String {
    let mut bytes = [0u8; 8];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Normalize the same identity representation used by the askpass prompt
/// matcher, which omits IPv6 brackets and does not include the port.
fn prompt_identity(user: &str, host: &str) -> (String, String) {
    (
        user.to_ascii_lowercase(),
        host.trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase(),
    )
}

/// Appends one diagnostic line to logs/sshterm.log next to the executable.
/// Only metadata and byte counts are recorded, never terminal contents or
/// credentials. Failures to write are ignored: logging must not break a session.
fn log_line(message: &str) {
    use std::io::Write as _;
    let dir = paths::logs_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("sshterm.log"))
    {
        let _ = writeln!(f, "{secs} {message}");
    }
}

/// Opens a connection saved in the vault. The password is never passed to the
/// frontend or shown on the command line: it is delivered via askpass.
#[tauri::command]
pub async fn session_open(
    app: AppHandle,
    state: State<'_, SessionState>,
    vault_state: State<'_, VaultState>,
    conn_id: String,
    cols: u16,
    rows: u16,
) -> Result<String, String> {
    let (cfg, credentials, name, jump_names) = {
        let g = vault_state.0.lock().unwrap_or_else(|e| e.into_inner());
        let vault = g.as_ref().ok_or_else(|| "locked".to_string())?;
        let conn = store::find(vault, &conn_id)
            .ok_or_else(|| "connection_not_found".to_string())?;
        // Connections saved by older versions were not validated on write.
        if !store::valid_address(&conn.user, &conn.host) || conn.port == 0 {
            return Err("bad_address".to_string());
        }
        let hops = store::resolve_jump_chain(&vault.data.connections, &conn_id)
            .map_err(str::to_string)?;
        let forwards = store::resolve_forwards(&conn.tunnels).map_err(str::to_string)?;

        let mut credentials = Vec::new();
        let mut prompt_targets = std::collections::HashSet::new();
        for hop in &hops {
            // Prompts omit the SSH port, so even agent-authenticated hops must
            // have distinct user@host identities from every other route member.
            let key = prompt_identity(&hop.user, &hop.host);
            if !prompt_targets.insert(key) {
                return Err("jump_ambiguous_credentials".to_string());
            }
            // ProxyJump does not accept a separate -i value per hop. Password
            // and agent authentication work; a saved per-hop key path will be
            // supported when the native transport can configure each hop.
            if hop.key_path().is_some() {
                return Err("jump_key_auth_unsupported".to_string());
            }
            if let Some(password) = hop.password() {
                credentials.push(AskpassCredential {
                    user: hop.user.clone(),
                    host: hop.host.clone(),
                    key_path: None,
                    password,
                });
            }
        }
        let target_key = prompt_identity(&conn.user, &conn.host);
        if !prompt_targets.insert(target_key) {
            return Err("jump_ambiguous_credentials".to_string());
        }
        if let Some(password) = conn.password() {
            credentials.push(AskpassCredential {
                user: conn.user.clone(),
                host: conn.host.clone(),
                key_path: conn.key_path(),
                password,
            });
        }

        let program = paths::ssh_program().map_err(|e| e.to_string())?;
        let cfg = SshConfig {
            program,
            askpass_program: paths::askpass_program(),
            known_hosts: paths::known_hosts_path(),
            host: conn.host.clone(),
            port: conn.port,
            user: conn.user.clone(),
            keepalive_secs: conn.keepalive_secs,
            key_path: conn.key_path().map(std::path::PathBuf::from),
            password_auth: conn.uses_password_auth(),
            jump_hosts: hops.iter().map(|hop| JumpHost {
                user: hop.user.clone(),
                host: hop.host.clone(),
                port: hop.port,
                password_auth: hop.uses_password_auth(),
            }).collect(),
            forwards,
        };
        let jump_names = hops.iter().map(|hop| hop.name.clone()).collect::<Vec<_>>();
        (cfg, credentials, conn.name.clone(), jump_names)
    };

    let has_password = !credentials.is_empty();
    let askpass = if credentials.is_empty() {
        None
    } else {
        Some(AskpassServer::start(credentials).map_err(|e| e.to_string())?)
    };

    let id = new_id();

    let mut transport = OpenSshTransport::new(askpass);
    transport
        .open(&cfg, PtySize { cols, rows })
        .await
        .map_err(|e| format!("{}: {}", e.code, e.detail))?;
    let mut rx = transport
        .output()
        .ok_or_else(|| "no_output".to_string())?;

    // Per-session log file (M3), created only once ssh has really started so a
    // failed start leaves no empty file behind. In the default mode the text comes
    // from the frontend (`session_log_append`); the PTY bytes are only written in
    // `raw` mode. Typed input and passwords are never logged.
    let app_cfg = Config::load(&paths::config_path());
    let logger = Arc::new(tokio::sync::Mutex::new(SessionLogger::open(
        &name,
        &cfg.host,
        &cfg.user,
        &app_cfg,
    )));
    state
        .loggers
        .lock()
        .await
        .insert(id.clone(), Arc::clone(&logger));

    state
        .transports
        .lock()
        .await
        .insert(id.clone(), Box::new(transport));

    log_line(&format!(
        "open id={id} host={} port={} user={} via={} program={} askpass={has_password}",
        cfg.host,
        cfg.port,
        cfg.user,
        jump_names.join(","),
        cfg.program.display()
    ));

    // Immediate feedback in the terminal, even before ssh.exe emits anything.
    // The app-wide session bus buffers it until the pane subscribes.
    let route = if jump_names.is_empty() {
        String::new()
    } else {
        format!(" via {}", jump_names.join(" → "))
    };
    let banner = format!(
        "sshterm: connecting{} to {}@{}:{} ...\r\n",
        route, cfg.user, cfg.host, cfg.port
    );
    let _ = app.emit(
        "session:data",
        DataEvent { id: id.clone(), data: STANDARD.encode(banner.as_bytes()) },
    );

    let event_id = id.clone();
    tauri::async_runtime::spawn(async move {
        let mut chunks = 0u64;
        let mut total = 0u64;
        let mut diagnostic_buffer = String::new();
        let mut emitted_errors = std::collections::HashSet::new();
        while let Some(bytes) = rx.recv().await {
            // Empty chunk = EOF sentinel from the transport's child watchdog.
            // ConPTY does not always close the output pipe when ssh.exe exits.
            if bytes.is_empty() {
                if let Some(message) = ssh_failure_message(&diagnostic_buffer) {
                    if emitted_errors.insert(message.to_ascii_lowercase()) {
                        let _ = app.emit(
                            "session:error",
                            SessionErrorEvent { id: event_id.clone(), message },
                        );
                    }
                }
                log_line(&format!("ssh exited id={event_id} chunks={chunks} bytes={total}"));
                break;
            }
            diagnostic_buffer.push_str(&String::from_utf8_lossy(&bytes));
            while let Some(newline) = diagnostic_buffer.find('\n') {
                let line = diagnostic_buffer.drain(..=newline).collect::<String>();
                if let Some(message) = ssh_failure_message(line.trim()) {
                    if emitted_errors.insert(message.to_ascii_lowercase()) {
                        let _ = app.emit(
                            "session:error",
                            SessionErrorEvent { id: event_id.clone(), message },
                        );
                    }
                }
            }
            // Keep partial output bounded while retaining incomplete final lines.
            if diagnostic_buffer.len() > 4096 {
                let keep_from = diagnostic_buffer.len().saturating_sub(2048);
                let boundary = diagnostic_buffer
                    .char_indices()
                    .find(|(index, _)| *index >= keep_from)
                    .map(|(index, _)| index)
                    .unwrap_or(0);
                diagnostic_buffer.drain(..boundary);
            }
            chunks += 1;
            total += bytes.len() as u64;
            if chunks <= 3 {
                log_line(&format!("data id={event_id} chunk={chunks} bytes={}", bytes.len()));
            }
            logger.lock().await.write_raw(&bytes);
            let payload = DataEvent { id: event_id.clone(), data: STANDARD.encode(bytes) };
            if app.emit("session:data", payload).is_err() {
                log_line(&format!("emit failed id={event_id}"));
                break;
            }
        }
        log_line(&format!("reader ended id={event_id} chunks={chunks} bytes={total}"));
        let _ = app.emit("session:closed", ClosedEvent { id: event_id });
    });

    Ok(id)
}

/// Lists installed WSL distributions. WSL may emit UTF-16LE when stdout is piped,
/// so accept both UTF-16LE and UTF-8 output.
#[tauri::command]
pub fn local_terminal_wsl_distros() -> Result<Vec<String>, String> {
    #[cfg(windows)]
    {
        let output = std::process::Command::new("wsl.exe")
            .args(["--list", "--quiet"])
            .output()
            .map_err(|e| format!("wsl_unavailable: {e}"))?;
        if !output.status.success() {
            return Err("wsl_list_failed".into());
        }
        let bytes = output.stdout;
        let text = if bytes.len() >= 2 && bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count() > bytes.len() / 8 {
            let words = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]]));
            String::from_utf16_lossy(&words.collect::<Vec<_>>())
        } else {
            String::from_utf8_lossy(&bytes).into_owned()
        };
        let distros = text.lines().map(|line| line.trim().trim_matches('\0').trim_start_matches('\u{feff}').to_string())
            .filter(|line| !line.is_empty()).collect::<Vec<_>>();
        if distros.is_empty() { return Err("wsl_no_distros".into()); }
        Ok(distros)
    }
    #[cfg(not(windows))]
    {
        Err("local_terminals_windows_only".into())
    }
}

/// Opens PowerShell or WSL in a normal workspace terminal tab.
#[tauri::command]
pub async fn local_terminal_open(
    app: AppHandle,
    state: State<'_, SessionState>,
    shell: String,
    distro: Option<String>,
    cols: u16,
    rows: u16,
) -> Result<String, String> {
    #[cfg(windows)]
    let (config, name) = {
        let (program, args, name) = match shell.as_str() {
            "powershell" => {
                let path_dirs = std::env::var_os("PATH").unwrap_or_default();
                let pwsh = std::env::split_paths(&path_dirs)
                    .map(|dir| dir.join("pwsh.exe"))
                    .find(|path| path.is_file());
                let powershell = std::env::split_paths(&path_dirs)
                    .map(|dir| dir.join("powershell.exe"))
                    .find(|path| path.is_file());
                let program = pwsh.or(powershell).unwrap_or_else(|| std::path::PathBuf::from("powershell.exe"));
                let name = if program.file_stem().and_then(|s| s.to_str()).unwrap_or("").eq_ignore_ascii_case("pwsh") {
                    "PowerShell 7"
                } else { "Windows PowerShell" };
                (program, vec!["-NoLogo".to_string()], name.to_string())
            }
            "wsl" => {
                let mut args = Vec::new();
                let selected = distro.as_deref().map(str::trim).filter(|s| !s.is_empty());
                if let Some(name) = selected {
                    if name.chars().any(|ch| ch.is_control()) { return Err("wsl_bad_distro".into()); }
                    args.push("--distribution".to_string());
                    args.push(name.to_string());
                }
                let label = selected.map(|name| format!("WSL · {name}")).unwrap_or_else(|| "WSL".into());
                (std::path::PathBuf::from("wsl.exe"), args, label)
            }
            _ => return Err("local_shell_unknown".into()),
        };
        (LocalShellConfig { program, args, cwd: None }, name)
    };
    #[cfg(not(windows))]
    let (config, name) = {
        let _ = (shell, distro, cols, rows);
        return Err("local_terminals_windows_only".into());
    };

    let id = new_id();
    let mut transport = OpenSshTransport::new(None);
    transport.open_local(&config, PtySize { cols, rows }).await
        .map_err(|e| format!("{}: {}", e.code, e.detail))?;
    let mut rx = transport.output().ok_or_else(|| "no_output".to_string())?;
    let app_cfg = Config::load(&paths::config_path());
    let logger = Arc::new(tokio::sync::Mutex::new(SessionLogger::open(&name, "local", &name, &app_cfg)));
    state.loggers.lock().await.insert(id.clone(), Arc::clone(&logger));
    state.transports.lock().await.insert(id.clone(), Box::new(transport));

    let banner = format!("sshterm: starting {name} ...\r\n");
    let _ = app.emit("session:data", DataEvent { id: id.clone(), data: STANDARD.encode(banner.as_bytes()) });
    let event_id = id.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if bytes.is_empty() { break; }
            logger.lock().await.write_raw(&bytes);
            if app.emit("session:data", DataEvent { id: event_id.clone(), data: STANDARD.encode(bytes) }).is_err() {
                break;
            }
        }
        let _ = app.emit("session:closed", ClosedEvent { id: event_id });
    });
    Ok(id)
}

#[tauri::command]
pub async fn session_write(
    state: State<'_, SessionState>,
    id: String,
    data: String,
) -> Result<(), String> {
    let bytes = STANDARD.decode(&data).map_err(|_| "bad_base64".to_string())?;
    let mut sessions = state.transports.lock().await;
    let transport = sessions
        .get_mut(&id)
        .ok_or_else(|| "session_not_found".to_string())?;
    transport.write(&bytes).await.map_err(|e| e.code.to_string())
}

#[tauri::command]
pub async fn session_resize(
    state: State<'_, SessionState>,
    id: String,
    cols: u16,
    rows: u16,
) -> Result<(), String> {
    let mut sessions = state.transports.lock().await;
    let transport = sessions
        .get_mut(&id)
        .ok_or_else(|| "session_not_found".to_string())?;
    transport
        .resize(PtySize { cols, rows })
        .await
        .map_err(|e| e.code.to_string())
}

#[tauri::command]
pub async fn session_close(state: State<'_, SessionState>, id: String) -> Result<(), String> {
    let mut sessions = state.transports.lock().await;
    if let Some(mut transport) = sessions.remove(&id) {
        transport.close().await.map_err(|e| e.code.to_string())?;
    }
    // The logger stays registered: the terminal sends the last lines of the screen
    // after the session ends, and then calls `session_log_close`.
    Ok(())
}

/// When the vault is locked the UI unmounts: sessions must be closed so no
/// ssh.exe processes are left running without a window to control them.
#[tauri::command]
pub async fn session_close_all(state: State<'_, SessionState>) -> Result<(), String> {
    let mut sessions = state.transports.lock().await;
    for (_, mut transport) in sessions.drain() {
        let _ = transport.close().await;
    }
    Ok(())
}

/// Appends plain text that the terminal built from its screen (see
/// `src/lib/logCapture.ts`). Ignored in `raw` mode or when logging is off.
#[tauri::command]
pub async fn session_log_append(
    state: State<'_, SessionState>,
    id: String,
    text: String,
) -> Result<(), String> {
    let logger = state.loggers.lock().await.get(&id).cloned();
    if let Some(logger) = logger {
        logger.lock().await.write_text(&text);
    }
    Ok(())
}

/// Writes the closing line of the log and releases the file.
#[tauri::command]
pub async fn session_log_close(state: State<'_, SessionState>, id: String) -> Result<(), String> {
    let logger = state.loggers.lock().await.remove(&id);
    if let Some(logger) = logger {
        logger.lock().await.finish();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ssh_failure_message;

    #[test]
    fn recognizes_jump_host_connection_failures() {
        let line = "stdio forwarding failed";
        assert_eq!(ssh_failure_message(line).as_deref(), Some(line));
        assert!(ssh_failure_message("ssh: connect to host bastion port 22: Connection timed out").is_some());
    }

    #[test]
    fn recognizes_local_and_reverse_forward_failures() {
        assert!(ssh_failure_message("bind: Address already in use").is_some());
        assert!(ssh_failure_message("Warning: remote port forwarding failed for listen port 18080").is_some());
        assert!(ssh_failure_message("Could not request local forwarding.").is_some());
        assert!(ssh_failure_message("Failed to add the host to the list of known hosts").is_some());
    }

    #[test]
    fn ignores_non_error_terminal_output() {
        assert!(ssh_failure_message("Welcome to Ubuntu 24.04").is_none());
        assert!(ssh_failure_message("Last login: Mon Nov 16 09:00:00").is_none());
    }
}
