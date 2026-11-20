//! Independent SSH tunnel processes (M7).
//!
//! A tunnel process is owned by the application, not by a terminal tab. One
//! ssh -N process groups all enabled forwards for a saved connection so it
//! reuses that connection's authentication and ProxyJump route.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, ChildStderr, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::askpass::{AskpassCredential, AskpassServer};
use crate::commands::VaultState;
use crate::paths;
use crate::store;
use crate::transport::{openssh, JumpHost, PortForward, SshConfig};
use crate::vault::Vault;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelLifecycle {
    Starting,
    Active,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TunnelForwardLifecycle {
    Starting,
    Active,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Debug, Clone, Serialize)]
pub struct TunnelForwardInfo {
    pub index: usize,
    pub kind: String,
    pub bind: String,
    pub destination: Option<String>,
    pub status: TunnelForwardLifecycle,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TunnelInfo {
    /// A connection owns at most one tunnel process; this is also its stable id.
    pub connection_id: String,
    pub connection_name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub jump_names: Vec<String>,
    pub status: TunnelLifecycle,
    pub forwards: Vec<TunnelForwardInfo>,
    pub error: Option<String>,
    /// Unix timestamp in seconds. Runtime state is deliberately not persisted.
    pub started_at: u64,
}

struct TunnelRuntime {
    info: Arc<Mutex<TunnelInfo>>,
    stop_requested: Arc<AtomicBool>,
    child: Arc<Mutex<Option<Child>>>,
    askpass: Arc<Mutex<Option<AskpassServer>>>,
}

#[derive(Default)]
pub struct TunnelState {
    entries: Mutex<HashMap<String, Arc<TunnelRuntime>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

fn prompt_identity(user: &str, host: &str) -> (String, String) {
    (
        user.to_ascii_lowercase(),
        host.trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_ascii_lowercase(),
    )
}

struct ResolvedTunnel {
    config: SshConfig,
    credentials: Vec<AskpassCredential>,
    info: TunnelInfo,
}

fn resolve_tunnel(vault: &Vault, connection_id: &str) -> Result<ResolvedTunnel, String> {
    let conn = store::find(vault, connection_id).ok_or_else(|| "connection_not_found".to_string())?;
    if !store::valid_address(&conn.user, &conn.host) || conn.port == 0 {
        return Err("bad_address".to_string());
    }

    let forwards = store::resolve_forwards(&conn.tunnels).map_err(str::to_string)?;
    if forwards.is_empty() {
        return Err("tunnel_no_forwards".to_string());
    }

    let hops = store::resolve_jump_chain(&vault.data.connections, connection_id)
        .map_err(str::to_string)?;
    let mut credentials = Vec::new();
    let mut prompt_targets = HashSet::new();

    for hop in &hops {
        let key = prompt_identity(&hop.user, &hop.host);
        if !prompt_targets.insert(key) {
            return Err("jump_ambiguous_credentials".to_string());
        }
        // OpenSSH ProxyJump cannot select a different custom -i file per hop.
        if hop.key_path().is_some() {
            return Err("jump_key_auth_unsupported".to_string());
        }
        if hop.uses_password_auth() && hop.password().is_none() {
            return Err("tunnel_credentials_required".to_string());
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

    if !prompt_targets.insert(prompt_identity(&conn.user, &conn.host)) {
        return Err("jump_ambiguous_credentials".to_string());
    }
    // Tunnel-only processes have no terminal to receive an unsaved password.
    if conn.uses_password_auth() && conn.password().is_none() {
        return Err("tunnel_credentials_required".to_string());
    }
    if let Some(password) = conn.password() {
        credentials.push(AskpassCredential {
            user: conn.user.clone(),
            host: conn.host.clone(),
            key_path: conn.key_path(),
            password,
        });
    }

    let program = paths::ssh_program().map_err(str::to_string)?;
    let jump_names = hops.iter().map(|hop| hop.name.clone()).collect::<Vec<_>>();
    let config = SshConfig {
        program,
        askpass_program: paths::askpass_program(),
        known_hosts: paths::known_hosts_path(),
        host: conn.host.clone(),
        port: conn.port,
        user: conn.user.clone(),
        keepalive_secs: conn.keepalive_secs,
        key_path: conn.key_path().map(PathBuf::from),
        password_auth: conn.uses_password_auth(),
        jump_hosts: hops
            .iter()
            .map(|hop| JumpHost {
                user: hop.user.clone(),
                host: hop.host.clone(),
                port: hop.port,
                password_auth: hop.uses_password_auth(),
            })
            .collect(),
        forwards: forwards.clone(),
    };

    Ok(ResolvedTunnel {
        config,
        credentials,
        info: TunnelInfo {
            connection_id: conn.id,
            connection_name: conn.name,
            host: conn.host,
            port: conn.port,
            user: conn.user,
            jump_names,
            status: TunnelLifecycle::Starting,
            forwards: forwards.iter().enumerate().map(|(index, forward)| forward_info(index, forward)).collect(),
            error: None,
            started_at: now_seconds(),
        },
    })
}

fn forward_info(index: usize, forward: &PortForward) -> TunnelForwardInfo {
    match forward {
        PortForward::Local { bind, destination } => TunnelForwardInfo {
            index,
            kind: "local".into(),
            bind: bind.clone(),
            destination: Some(destination.clone()),
            status: TunnelForwardLifecycle::Starting,
            error: None,
        },
        PortForward::Remote { bind, destination } => TunnelForwardInfo {
            index,
            kind: "remote".into(),
            bind: bind.clone(),
            destination: Some(destination.clone()),
            status: TunnelForwardLifecycle::Starting,
            error: None,
        },
        PortForward::Dynamic { bind } => TunnelForwardInfo {
            index,
            kind: "dynamic".into(),
            bind: bind.clone(),
            destination: None,
            status: TunnelForwardLifecycle::Starting,
            error: None,
        },
    }
}

fn publish(app: &AppHandle, info: &Arc<Mutex<TunnelInfo>>) {
    let snapshot = lock(info).clone();
    let _ = app.emit("tunnel:status", snapshot);
}

fn set_lifecycle(info: &Arc<Mutex<TunnelInfo>>, status: TunnelLifecycle) {
    lock(info).status = status;
}

fn tunnel_failure_message(line: &str, authenticated: bool) -> Option<String> {
    let lower = line.to_ascii_lowercase();
    let forwarding_patterns = [
        "stdio forwarding failed",
        "could not request local forwarding",
        "remote port forwarding failed",
        "channel_setup_fwd_listener_tcpip",
        "cannot listen to port",
        "address already in use",
        "bind: ",
    ];
    let pre_auth_patterns = [
        "could not resolve hostname",
        "connection timed out",
        "operation timed out",
        "connection refused",
        "no route to host",
        "permission denied",
        "host key verification failed",
        "remote host identification has changed",
        "connection closed by",
    ];
    let is_failure = forwarding_patterns.iter().any(|pattern| lower.contains(pattern))
        || (!authenticated && pre_auth_patterns.iter().any(|pattern| lower.contains(pattern)));
    if !is_failure {
        return None;
    }
    let message = line
        .chars()
        .filter(|ch| !ch.is_control())
        .take(500)
        .collect::<String>()
        .trim()
        .to_string();
    (!message.is_empty()).then_some(message)
}

fn set_all_forward_lifecycle(info: &Arc<Mutex<TunnelInfo>>, status: TunnelForwardLifecycle) {
    for forward in &mut lock(info).forwards {
        forward.status = status;
    }
}

fn bind_port(bind: &str) -> String {
    if let Some(rest) = bind.strip_prefix('[') {
        return rest
            .split_once("]:")
            .map(|(_, port)| port.to_string())
            .unwrap_or_else(|| bind.to_string());
    }
    bind.rsplit_once(':')
        .map(|(_, port)| port.to_string())
        .unwrap_or_else(|| bind.to_string())
}

fn has_port(line: &str, port: &str) -> bool {
    line.split(|ch: char| !ch.is_ascii_digit()).any(|part| part == port)
}

fn ready_forward_indices(line: &str, info: &TunnelInfo) -> Vec<usize> {
    let lower = line.to_ascii_lowercase();
    let local_ready = lower.contains("local forwarding listening on");
    let remote_ready = lower.contains("remote forward success for");
    if !local_ready && !remote_ready {
        return Vec::new();
    }
    info.forwards
        .iter()
        .filter(|forward| {
            let expected = if forward.kind == "remote" { remote_ready } else { local_ready };
            expected && has_port(line, &bind_port(&forward.bind))
        })
        .map(|forward| forward.index)
        .collect()
}

fn set_failed(info: &Arc<Mutex<TunnelInfo>>, message: String, line: &str) {
    let mut current = lock(info);
    current.status = TunnelLifecycle::Failed;
    current.error = Some(message.clone());
    let port_matches = current
        .forwards
        .iter()
        .any(|forward| has_port(line, &bind_port(&forward.bind)));
    for forward in &mut current.forwards {
        if !port_matches || has_port(line, &bind_port(&forward.bind)) {
            forward.status = TunnelForwardLifecycle::Failed;
            forward.error = Some(message.clone());
        }
    }
}

fn set_active_forwards(info: &Arc<Mutex<TunnelInfo>>, indices: &HashSet<usize>) {
    let mut current = lock(info);
    for forward in &mut current.forwards {
        if indices.contains(&forward.index) {
            forward.status = TunnelForwardLifecycle::Active;
            forward.error = None;
        }
    }
}

/// Terminates OpenSSH and any ProxyJump child processes it created.
///
/// On Windows, killing ssh.exe alone can orphan the nested ssh.exe processes
/// that implement ProxyJump. taskkill /T walks and terminates the full process
/// tree; Child::kill + wait remains the fallback and reaps the owned process.
fn terminate_process_tree(child: &mut Child) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        let taskkill = std::env::var_os("SystemRoot")
            .map(|root| PathBuf::from(root).join("System32").join("taskkill.exe"))
            .unwrap_or_else(|| PathBuf::from("taskkill.exe"));
        let pid = child.id().to_string();
        let _ = Command::new(taskkill)
            .args(["/PID", pid.as_str(), "/T", "/F"])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000) // CREATE_NO_WINDOW
            .status();
    }

    let _ = child.kill();
    let _ = child.wait();
}

fn request_stop(runtime: &TunnelRuntime) {
    let already_requested = runtime.stop_requested.swap(true, Ordering::AcqRel);
    {
        let mut info = lock(&runtime.info);
        if !matches!(info.status, TunnelLifecycle::Stopped | TunnelLifecycle::Failed) {
            info.status = TunnelLifecycle::Stopping;
            for forward in &mut info.forwards {
                if matches!(forward.status, TunnelForwardLifecycle::Starting | TunnelForwardLifecycle::Active) {
                    forward.status = TunnelForwardLifecycle::Stopping;
                }
            }
        }
    }
    if !already_requested {
        if let Some(child) = lock(&runtime.child).as_mut() {
            terminate_process_tree(child);
        }
    }
}

fn monitor_process(
    app: AppHandle,
    info: Arc<Mutex<TunnelInfo>>,
    child: Arc<Mutex<Option<Child>>>,
    stop_requested: Arc<AtomicBool>,
    askpass: Arc<Mutex<Option<AskpassServer>>>,
    config_file: Option<PathBuf>,
    stderr: Option<ChildStderr>,
) {
    let (sender, receiver) = std::sync::mpsc::channel::<String>();
    let mut stderr_thread = if let Some(stderr) = stderr {
        match thread::Builder::new()
            .name("sshterm-tunnel-stderr".into())
            .spawn(move || {
                for line in BufReader::new(stderr).lines() {
                    match line {
                        Ok(line) => {
                            if sender.send(line).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
            }) {
            Ok(handle) => Some(handle),
            Err(error) => {
                if let Some(process) = lock(&child).as_mut() {
                    terminate_process_tree(process);
                }
                set_failed(&info, format!("tunnel_stderr_monitor: {error}"), "");
                lock(&child).take();
                lock(&askpass).take();
                if let Some(path) = config_file {
                    let _ = std::fs::remove_file(path);
                }
                publish(&app, &info);
                return;
            }
        }
    } else {
        drop(sender);
        None
    };

    let started = Instant::now();
    let mut authenticated = false;
    let mut ready = HashSet::new();
    let mut last_error: Option<String> = None;

    loop {
        for line in receiver.try_iter() {
            let lower = line.to_ascii_lowercase();
            if lower.contains("authenticated to ") {
                authenticated = true;
            }
            let current = lock(&info).clone();
            for index in ready_forward_indices(&line, &current) {
                ready.insert(index);
            }
            set_active_forwards(&info, &ready);

            if let Some(message) = tunnel_failure_message(&line, authenticated) {
                last_error = Some(message.clone());
                set_failed(&info, message, &line);
                publish(&app, &info);
            }
        }

        let exit = {
            let mut guard = lock(&child);
            match guard.as_mut() {
                Some(process) => process.try_wait(),
                None => Ok(None),
            }
        };

        match exit {
            Ok(Some(status)) => {
                // Wait for stderr EOF so final SSH diagnostics are not lost
                // just because the process handle became signalled first.
                if let Some(reader) = stderr_thread.take() {
                    let _ = reader.join();
                }
                for line in receiver.try_iter() {
                    if let Some(message) = tunnel_failure_message(&line, authenticated) {
                        last_error = Some(message.clone());
                        set_failed(&info, message, &line);
                    }
                }
                if stop_requested.load(Ordering::Acquire) && last_error.is_none() {
                    set_lifecycle(&info, TunnelLifecycle::Stopped);
                    set_all_forward_lifecycle(&info, TunnelForwardLifecycle::Stopped);
                    lock(&info).error = None;
                } else {
                    let message = last_error.unwrap_or_else(|| {
                        format!("ssh_tunnel_exited: OpenSSH exited unexpectedly ({status:?}).")
                    });
                    set_failed(&info, message, "");
                }
                break;
            }
            Ok(None) => {}
            Err(error) => {
                // If process polling itself fails, terminate and reap the child
                // before releasing the askpass server or generated SSH config.
                if let Some(process) = lock(&child).as_mut() {
                    terminate_process_tree(process);
                }
                if let Some(reader) = stderr_thread.take() {
                    let _ = reader.join();
                }
                for line in receiver.try_iter() {
                    if let Some(message) = tunnel_failure_message(&line, authenticated) {
                        set_failed(&info, message, &line);
                    }
                }
                set_failed(&info, format!("ssh_tunnel_monitor: {error}"), "");
                break;
            }
        }

        // The verbose OpenSSH messages normally confirm each listener. This
        // short fallback supports client versions with different debug wording:
        // authenticated + alive + ExitOnForwardFailure means setup did not fail.
        if authenticated && started.elapsed() >= Duration::from_secs(2) {
            let expected = lock(&info).forwards.len();
            if ready.len() < expected && last_error.is_none() {
                ready = (0..expected).collect();
                set_active_forwards(&info, &ready);
            }
        }

        let should_publish_active = {
            let current = lock(&info);
            authenticated
                && ready.len() == current.forwards.len()
                && current.status == TunnelLifecycle::Starting
        };
        if should_publish_active {
            set_lifecycle(&info, TunnelLifecycle::Active);
            publish(&app, &info);
        }

        thread::sleep(Duration::from_millis(100));
    }

    lock(&child).take();
    lock(&askpass).take();
    if let Some(path) = config_file {
        let _ = std::fs::remove_file(path);
    }
    publish(&app, &info);
}

#[tauri::command]
pub fn tunnel_list(state: State<'_, TunnelState>) -> Vec<TunnelInfo> {
    let entries = lock(&state.entries);
    let mut result = entries
        .values()
        .map(|runtime| lock(&runtime.info).clone())
        .collect::<Vec<_>>();
    result.sort_by(|a, b| a.connection_name.cmp(&b.connection_name));
    result
}

#[tauri::command]
pub fn tunnel_start(
    app: AppHandle,
    state: State<'_, TunnelState>,
    vault_state: State<'_, VaultState>,
    connection_id: String,
) -> Result<TunnelInfo, String> {
    // Keep the vault locked for the whole process registration. vault_lock
    // takes this same lock before draining TunnelState, so a concurrent start
    // cannot slip in after the lock cleanup has already run.
    let vault_guard = vault_state.0.lock().unwrap_or_else(|e| e.into_inner());
    let vault = vault_guard.as_ref().ok_or_else(|| "locked".to_string())?;
    let resolved = resolve_tunnel(vault, &connection_id)?;

    let mut entries = lock(&state.entries);
    if let Some(existing) = entries.get(&connection_id) {
        let snapshot = lock(&existing.info).clone();
        match snapshot.status {
            TunnelLifecycle::Starting | TunnelLifecycle::Active => return Ok(snapshot),
            TunnelLifecycle::Stopping => return Err("tunnel_stopping".into()),
            TunnelLifecycle::Stopped | TunnelLifecycle::Failed => {
                // Do not overlap a replacement process with the previous
                // child's final cleanup (especially important for bound ports).
                if lock(&existing.child).is_some() {
                    return Err("tunnel_stopping".into());
                }
                entries.remove(&connection_id);
            }
        }
    }

    let askpass = if resolved.credentials.is_empty() {
        None
    } else {
        Some(AskpassServer::start(resolved.credentials).map_err(str::to_string)?)
    };
    let askpass = Arc::new(Mutex::new(askpass));
    let spawned = {
        let guard = lock(&askpass);
        openssh::spawn_tunnel_process(&resolved.config, guard.as_ref())
    };

    let mut info = resolved.info;
    let (child_process, config_file) = match spawned {
        Ok(value) => value,
        Err(error) => {
            let message = format!("{}: {}", error.code, error.detail);
            info.status = TunnelLifecycle::Failed;
            info.error = Some(message.clone());
            for forward in &mut info.forwards {
                forward.status = TunnelForwardLifecycle::Failed;
                forward.error = Some(message.clone());
            }
            let info = Arc::new(Mutex::new(info));
            let runtime = Arc::new(TunnelRuntime {
                info: info.clone(),
                stop_requested: Arc::new(AtomicBool::new(false)),
                child: Arc::new(Mutex::new(None)),
                askpass: Arc::new(Mutex::new(None)),
            });
            let snapshot = lock(&info).clone();
            entries.insert(connection_id, runtime);
            publish(&app, &info);
            return Ok(snapshot);
        }
    };

    let mut child_process = child_process;
    let stderr = child_process.stderr.take();
    let child = Arc::new(Mutex::new(Some(child_process)));
    let info = Arc::new(Mutex::new(info));
    let runtime = Arc::new(TunnelRuntime {
        info: info.clone(),
        stop_requested: Arc::new(AtomicBool::new(false)),
        child: child.clone(),
        askpass: askpass.clone(),
    });
    entries.insert(connection_id, runtime.clone());
    publish(&app, &info);

    let monitor_app = app.clone();
    let monitor_info = info.clone();
    let monitor_stop = runtime.stop_requested.clone();
    let monitor_askpass = askpass;
    let monitor_child = child;
    let cleanup_config_file = config_file.clone();
    if let Err(error) = thread::Builder::new()
        .name("sshterm-tunnel-monitor".into())
        .spawn(move || monitor_process(
            monitor_app,
            monitor_info,
            monitor_child,
            monitor_stop,
            monitor_askpass,
            config_file,
            stderr,
        ))
    {
        runtime.stop_requested.store(true, Ordering::Release);
        if let Some(mut process) = lock(&runtime.child).take() {
            terminate_process_tree(&mut process);
        }
        lock(&runtime.askpass).take();
        if let Some(path) = cleanup_config_file {
            let _ = std::fs::remove_file(path);
        }
        let message = format!("tunnel_monitor_start: {error}");
        set_failed(&info, message, "");
        publish(&app, &info);
    }

    Ok(lock(&info).clone())
}

#[tauri::command]
pub fn tunnel_stop(
    app: AppHandle,
    state: State<'_, TunnelState>,
    connection_id: String,
) -> Result<(), String> {
    let runtime = lock(&state.entries).get(&connection_id).cloned();
    if let Some(runtime) = runtime {
        request_stop(&runtime);
        publish(&app, &runtime.info);
    }
    Ok(())
}

#[tauri::command]
pub fn tunnel_stop_all(
    app: AppHandle,
    state: State<'_, TunnelState>,
    vault_state: State<'_, VaultState>,
) {
    // Serialize against tunnel_start using the same vault -> tunnel lock order.
    let _vault_guard = vault_state.0.lock().unwrap_or_else(|e| e.into_inner());
    let runtimes = lock(&state.entries).values().cloned().collect::<Vec<_>>();
    for runtime in runtimes {
        request_stop(&runtime);
        publish(&app, &runtime.info);
    }
}

/// Stop independent tunnel process trees before the Tauri runtime exits.
pub fn stop_all_for_app_exit(state: &TunnelState) {
    let runtimes = lock(&state.entries).values().cloned().collect::<Vec<_>>();
    for runtime in runtimes {
        request_stop(&runtime);
    }
}

pub fn stop_all_for_vault_lock(state: &TunnelState) {
    let runtimes = {
        let mut entries = lock(&state.entries);
        entries.drain().map(|(_, runtime)| runtime).collect::<Vec<_>>()
    };
    for runtime in runtimes {
        request_stop(&runtime);
    }
}

impl Drop for TunnelState {
    fn drop(&mut self) {
        let entries = self.entries.get_mut().unwrap_or_else(|e| e.into_inner());
        for runtime in entries.values() {
            request_stop(runtime);
        }
        entries.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_start_destination_refusal_does_not_mark_listener_failed() {
        assert!(tunnel_failure_message("channel 3: open failed: connect failed: Connection refused", true).is_none());
        assert!(tunnel_failure_message("channel 4: open failed: administratively prohibited", true).is_none());
        assert!(tunnel_failure_message("connect to host bastion port 22: Connection timed out", false).is_some());
        assert!(tunnel_failure_message("bind [127.0.0.1]:15432: Address already in use", true).is_some());
    }

    #[test]
    fn stop_request_marks_each_active_forward_stopping() {
        let info = Arc::new(Mutex::new(TunnelInfo {
            connection_id: "c1".into(),
            connection_name: "db".into(),
            host: "example".into(),
            port: 22,
            user: "admin".into(),
            jump_names: vec![],
            status: TunnelLifecycle::Active,
            forwards: vec![
                TunnelForwardInfo { index: 0, kind: "local".into(), bind: "15432".into(), destination: Some("db:5432".into()), status: TunnelForwardLifecycle::Active, error: None },
            ],
            error: None,
            started_at: 1,
        }));
        let runtime = TunnelRuntime {
            info: info.clone(),
            stop_requested: Arc::new(AtomicBool::new(false)),
            child: Arc::new(Mutex::new(None)),
            askpass: Arc::new(Mutex::new(None)),
        };
        request_stop(&runtime);
        assert_eq!(lock(&info).status, TunnelLifecycle::Stopping);
        assert_eq!(lock(&info).forwards[0].status, TunnelForwardLifecycle::Stopping);
    }

    #[test]
    fn stop_request_terminates_and_reaps_the_owned_process() {
        #[cfg(windows)]
        let child = Command::new("cmd")
            .args(["/C", "ping -n 30 127.0.0.1 > NUL"])
            .spawn()
            .unwrap();
        #[cfg(not(windows))]
        let child = Command::new("sleep").arg("30").spawn().unwrap();

        let info = Arc::new(Mutex::new(TunnelInfo {
            connection_id: "process-test".into(),
            connection_name: "process-test".into(),
            host: "127.0.0.1".into(),
            port: 22,
            user: "test".into(),
            jump_names: vec![],
            status: TunnelLifecycle::Active,
            forwards: vec![],
            error: None,
            started_at: 1,
        }));
        let runtime = TunnelRuntime {
            info,
            stop_requested: Arc::new(AtomicBool::new(false)),
            child: Arc::new(Mutex::new(Some(child))),
            askpass: Arc::new(Mutex::new(None)),
        };

        request_stop(&runtime);
        // A second stop must not re-run process-tree termination or change
        // the already requested lifecycle transition.
        request_stop(&runtime);

        let mut child = lock(&runtime.child);
        assert!(child.as_mut().unwrap().try_wait().unwrap().is_some());
        assert!(runtime.stop_requested.load(Ordering::Acquire));
    }

    #[test]
    fn app_exit_requests_stop_for_every_registered_tunnel() {
        let state = TunnelState::default();
        for id in ["one", "two"] {
            let info = Arc::new(Mutex::new(TunnelInfo {
                connection_id: id.into(),
                connection_name: id.into(),
                host: "127.0.0.1".into(),
                port: 22,
                user: "test".into(),
                jump_names: vec![],
                status: TunnelLifecycle::Active,
                forwards: vec![],
                error: None,
                started_at: 1,
            }));
            let runtime = Arc::new(TunnelRuntime {
                info,
                stop_requested: Arc::new(AtomicBool::new(false)),
                child: Arc::new(Mutex::new(None)),
                askpass: Arc::new(Mutex::new(None)),
            });
            lock(&state.entries).insert(id.into(), runtime);
        }

        stop_all_for_app_exit(&state);

        let entries = lock(&state.entries);
        assert_eq!(entries.len(), 2);
        assert!(entries.values().all(|runtime| runtime.stop_requested.load(Ordering::Acquire)));
        assert!(entries.values().all(|runtime| lock(&runtime.info).status == TunnelLifecycle::Stopping));
    }

    #[test]
    fn bind_port_handles_bare_ports_and_ipv6() {
        assert_eq!(bind_port("18080"), "18080");
        assert_eq!(bind_port("127.0.0.1:18080"), "18080");
        assert_eq!(bind_port("[::1]:18080"), "18080");
    }

    #[test]
    fn a_listener_failure_is_assigned_to_the_matching_forward() {
        let info = Arc::new(Mutex::new(TunnelInfo {
            connection_id: "c1".into(),
            connection_name: "db".into(),
            host: "example".into(),
            port: 22,
            user: "admin".into(),
            jump_names: vec![],
            status: TunnelLifecycle::Starting,
            forwards: vec![
                TunnelForwardInfo { index: 0, kind: "local".into(), bind: "127.0.0.1:15432".into(), destination: Some("db:5432".into()), status: TunnelForwardLifecycle::Starting, error: None },
                TunnelForwardInfo { index: 1, kind: "dynamic".into(), bind: "127.0.0.1:1080".into(), destination: None, status: TunnelForwardLifecycle::Starting, error: None },
            ],
            error: None,
            started_at: 1,
        }));
        set_failed(&info, "bind: Address already in use".into(), "bind [127.0.0.1]:15432: Address already in use");

        let current = lock(&info);
        assert_eq!(current.status, TunnelLifecycle::Failed);
        assert_eq!(current.forwards[0].status, TunnelForwardLifecycle::Failed);
        assert_eq!(current.forwards[1].status, TunnelForwardLifecycle::Starting);
        assert!(current.forwards[0].error.as_deref().unwrap().contains("Address already in use"));
        assert!(current.forwards[1].error.is_none());
    }

    #[test]
    fn an_unexpected_process_exit_marks_every_forward_failed() {
        let info = Arc::new(Mutex::new(TunnelInfo {
            connection_id: "c1".into(),
            connection_name: "db".into(),
            host: "example".into(),
            port: 22,
            user: "admin".into(),
            jump_names: vec![],
            status: TunnelLifecycle::Active,
            forwards: vec![
                TunnelForwardInfo { index: 0, kind: "local".into(), bind: "15432".into(), destination: Some("db:5432".into()), status: TunnelForwardLifecycle::Active, error: None },
                TunnelForwardInfo { index: 1, kind: "dynamic".into(), bind: "1080".into(), destination: None, status: TunnelForwardLifecycle::Active, error: None },
            ],
            error: None,
            started_at: 1,
        }));
        set_failed(&info, "ssh_tunnel_exited".into(), "");

        let current = lock(&info);
        assert_eq!(current.status, TunnelLifecycle::Failed);
        assert!(current.forwards.iter().all(|forward| forward.status == TunnelForwardLifecycle::Failed));
    }

    #[test]
    fn lifecycle_states_serialize_as_stable_lowercase_values() {
        assert_eq!(serde_json::to_string(&TunnelLifecycle::Starting).unwrap(), "\"starting\"");
        assert_eq!(serde_json::to_string(&TunnelLifecycle::Active).unwrap(), "\"active\"");
        assert_eq!(serde_json::to_string(&TunnelLifecycle::Stopping).unwrap(), "\"stopping\"");
        assert_eq!(serde_json::to_string(&TunnelLifecycle::Stopped).unwrap(), "\"stopped\"");
        assert_eq!(serde_json::to_string(&TunnelLifecycle::Failed).unwrap(), "\"failed\"");
    }

    #[test]
    fn ready_diagnostics_are_matched_to_forward_kind_and_port() {
        let info = TunnelInfo {
            connection_id: "c1".into(),
            connection_name: "db".into(),
            host: "bastion.example".into(),
            port: 22,
            user: "admin".into(),
            jump_names: vec![],
            status: TunnelLifecycle::Starting,
            forwards: vec![
                TunnelForwardInfo { index: 0, kind: "local".into(), bind: "127.0.0.1:15432".into(), destination: Some("db:5432".into()), status: TunnelForwardLifecycle::Starting, error: None },
                TunnelForwardInfo { index: 1, kind: "remote".into(), bind: "127.0.0.1:18080".into(), destination: Some("127.0.0.1:8080".into()), status: TunnelForwardLifecycle::Starting, error: None },
                TunnelForwardInfo { index: 2, kind: "dynamic".into(), bind: "1080".into(), destination: None, status: TunnelForwardLifecycle::Starting, error: None },
            ],
            error: None,
            started_at: 1,
        };
        assert_eq!(ready_forward_indices("debug1: Local forwarding listening on 127.0.0.1 port 15432.", &info), vec![0]);
        assert_eq!(ready_forward_indices("debug1: remote forward success for: listen 127.0.0.1:18080, connect 127.0.0.1:8080", &info), vec![1]);
        assert_eq!(ready_forward_indices("debug1: Local forwarding listening on ::1 port 1080.", &info), vec![2]);
    }
}
