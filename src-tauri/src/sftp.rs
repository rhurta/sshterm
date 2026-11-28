//! Native SFTP protocol layer (M8a).
//!
//! This module deliberately has no UI or Tauri command dependency. It proves
//! the SSH/SFTP transport independently before M8b adds the application-facing
//! lifecycle and browser commands.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use hmac::{Hmac, KeyInit, Mac};
use russh::client::{self, Handle};
use russh::keys::agent::{client::{AgentClient, AgentStream}, AgentIdentity};
use russh::keys::{self, HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use rand::{rngs::OsRng, RngCore};
use russh_sftp::client::fs::Metadata;
use russh_sftp::client::{error::Error as SftpClientError, Config as SftpConfig, RawSftpSession, SftpSession};
use russh_sftp::protocol::StatusCode;
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};
use sha1::Sha1;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{watch, Mutex, RwLock};
use tokio::task::JoinSet;

use crate::store;
use crate::vault::{Auth, Connection};

#[derive(Debug)]
pub struct SftpError {
    pub code: &'static str,
    pub detail: String,
}

impl SftpError {
    fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into() }
    }
}

impl fmt::Display for SftpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for SftpError {}

impl From<russh::Error> for SftpError {
    fn from(error: russh::Error) -> Self {
        Self::new("sftp_ssh", error.to_string())
    }
}

impl From<russh::SendError> for SftpError {
    fn from(error: russh::SendError) -> Self {
        Self::new("sftp_ssh", format!("{error:?}"))
    }
}

fn host_pattern_matches(host: &str, pattern: &str) -> bool {
    if let Some(hash) = pattern.strip_prefix("|1|") {
        let mut parts = hash.split('|');
        let Some(salt) = parts.next().and_then(|value| base64::engine::general_purpose::STANDARD.decode(value).ok()) else {
            return false;
        };
        let Some(expected) = parts.next().and_then(|value| base64::engine::general_purpose::STANDARD.decode(value).ok()) else {
            return false;
        };
        let Ok(mut mac) = Hmac::<Sha1>::new_from_slice(&salt) else {
            return false;
        };
        mac.update(host.as_bytes());
        return mac.verify_slice(&expected).is_ok();
    }

    let pattern = pattern.as_bytes();
    let host = host.as_bytes();
    let (mut pattern_index, mut host_index, mut star, mut retry) = (0, 0, None, 0);
    while host_index < host.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?'
                || pattern[pattern_index].eq_ignore_ascii_case(&host[host_index]))
        {
            pattern_index += 1;
            host_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star = Some(pattern_index);
            pattern_index += 1;
            retry = host_index;
        } else if let Some(star_index) = star {
            pattern_index = star_index + 1;
            retry += 1;
            host_index = retry;
        } else {
            return false;
        }
    }
    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }
    pattern_index == pattern.len()
}

fn host_patterns_match(host: &str, patterns: &str) -> bool {
    let mut positive_match = false;
    for pattern in patterns.split(',') {
        if let Some(negative) = pattern.strip_prefix('!') {
            if host_pattern_matches(host, negative) {
                return false;
            }
        } else if host_pattern_matches(host, pattern) {
            positive_match = true;
        }
    }
    positive_match
}

fn is_revoked_host_key(
    host: &str,
    port: u16,
    public_key: &PublicKey,
    known_hosts: &Path,
) -> Result<bool, SftpError> {
    let contents = match std::fs::read_to_string(known_hosts) {
        Ok(contents) => contents,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(SftpError::new("sftp_known_hosts_read", error.to_string())),
    };
    let host_port = if port == 22 { host.to_string() } else { format!("[{host}]:{port}") };

    for line in contents.lines() {
        let mut fields = line.split_whitespace();
        if fields.next() != Some("@revoked") {
            continue;
        }
        let (Some(patterns), Some(_key_type), Some(encoded_key)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if !host_patterns_match(&host_port, patterns) {
            continue;
        }
        let revoked_key = keys::parse_public_key_base64(encoded_key)
            .map_err(|error| SftpError::new("sftp_known_hosts_read", error.to_string()))?;
        if revoked_key == *public_key {
            return Ok(true);
        }
    }
    Ok(false)
}

fn verify_or_learn_host_key(
    host: &str,
    port: u16,
    public_key: &PublicKey,
    known_hosts: &Path,
) -> Result<bool, SftpError> {
    if is_revoked_host_key(host, port, public_key, known_hosts)? {
        return Err(SftpError::new(
            "sftp_host_key_revoked",
            "The server host key is explicitly revoked in the portable known_hosts file.",
        ));
    }
    match keys::check_known_hosts_path(host, port, public_key, known_hosts) {
        Ok(true) => Ok(true),
        Ok(false) => {
            keys::known_hosts::learn_known_hosts_path(host, port, public_key, known_hosts)
                .map_err(|error| SftpError::new("sftp_known_hosts_write", error.to_string()))?;
            Ok(true)
        }
        Err(keys::Error::KeyChanged { line }) => Err(SftpError::new(
            "sftp_host_key_changed",
            format!("The server host key changed (known_hosts line {line})."),
        )),
        Err(error) => Err(SftpError::new("sftp_known_hosts_read", error.to_string())),
    }
}

struct SftpHostKeyHandler {
    host: String,
    port: u16,
    known_hosts: PathBuf,
}

impl SftpHostKeyHandler {
    fn new(host: &str, port: u16, known_hosts: &Path) -> Self {
        Self {
            host: host.to_string(),
            port,
            known_hosts: known_hosts.to_path_buf(),
        }
    }
}

impl client::Handler for SftpHostKeyHandler {
    type Error = SftpError;

    async fn check_server_key(
        &mut self,
        server_public_key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        // Host certificates need CA/principal/revocation validation. Until
        // that policy is implemented, reject them rather than treating the
        // certificate's subject key as an ordinary TOFU host key.
        let public_key = match server_public_key {
            PublicKeyOrCertificate::PublicKey { key, .. } => key,
            PublicKeyOrCertificate::Certificate(_) => {
                return Err(SftpError::new(
                    "sftp_host_certificate_unsupported",
                    "Host certificates are not yet supported; refusing the connection.",
                ));
            }
        };
        verify_or_learn_host_key(&self.host, self.port, public_key, &self.known_hosts)
    }
}

fn client_config(keepalive_secs: u32) -> Arc<client::Config> {
    Arc::new(client::Config {
        nodelay: true,
        keepalive_interval: (keepalive_secs > 0)
            .then_some(Duration::from_secs(u64::from(keepalive_secs))),
        keepalive_max: 3,
        ..Default::default()
    })
}

fn endpoint(connection: &Connection, use_sftp_port: bool) -> Result<(String, u16), SftpError> {
    let port = if use_sftp_port {
        connection.effective_sftp_port()
    } else {
        connection.port
    };
    if port == 0 || !store::valid_address(&connection.user, &connection.host) {
        return Err(SftpError::new("sftp_bad_address", "Invalid SSH host, user, or port."));
    }
    let host = connection.host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(&connection.host)
        .to_string();
    Ok((host, port))
}

async fn connect_tcp(
    host: &str,
    port: u16,
    keepalive_secs: u32,
    known_hosts: &Path,
) -> Result<Handle<SftpHostKeyHandler>, SftpError> {
    let handler = SftpHostKeyHandler::new(host, port, known_hosts);
    client::connect(client_config(keepalive_secs), (host, port), handler)
        .await
        .map_err(Into::into)
}

async fn connect_channel<S>(
    stream: S,
    host: &str,
    port: u16,
    keepalive_secs: u32,
    known_hosts: &Path,
) -> Result<Handle<SftpHostKeyHandler>, SftpError>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let handler = SftpHostKeyHandler::new(host, port, known_hosts);
    client::connect_stream(client_config(keepalive_secs), stream, handler)
        .await
        .map_err(Into::into)
}

async fn best_rsa_hash(handle: &Handle<SftpHostKeyHandler>) -> Result<Option<HashAlg>, SftpError> {
    // If the server omits server-sig-algs, prefer RSA/SHA-512 rather than
    // silently falling back to the legacy SHA-1 ssh-rsa signature.
    Ok(match handle.best_supported_rsa_hash().await? {
        None => Some(HashAlg::Sha512),
        Some(server_choice) => server_choice,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentBindingStatus {
    NotUsed,
    Bound,
    LegacyUnbound,
}

impl AgentBindingStatus {
    fn merge(self, other: Self) -> Self {
        use AgentBindingStatus::{Bound, LegacyUnbound, NotUsed};
        match (self, other) {
            (LegacyUnbound, _) | (_, LegacyUnbound) => LegacyUnbound,
            (Bound, _) | (_, Bound) => Bound,
            _ => NotUsed,
        }
    }
}

fn is_legacy_agent_session_binding_failure(error: &keys::Error) -> bool {
    matches!(error, keys::Error::AgentFailure)
        || matches!(
            error,
            keys::Error::IO(io_error)
                if io_error.kind() == std::io::ErrorKind::UnexpectedEof
        )
}

fn is_agent_identity_binding_disconnect(bind_session: bool, error: &keys::Error) -> bool {
    bind_session
        && matches!(
            error,
            keys::Error::IO(io_error)
                if io_error.kind() == std::io::ErrorKind::UnexpectedEof
        )
}

fn is_agent_identity_disconnect(error: &keys::Error) -> bool {
    matches!(
        error,
        keys::Error::IO(io_error)
            if io_error.kind() == std::io::ErrorKind::UnexpectedEof
    )
}

fn agent_not_running_error(detail: impl fmt::Display) -> SftpError {
    let guidance = if cfg!(windows) {
        "Start the Windows OpenSSH ssh-agent service or launch Pageant before connecting."
    } else {
        "Start ssh-agent and ensure SSH_AUTH_SOCK points to its socket before connecting."
    };
    SftpError::new(
        "sftp_agent_unavailable",
        format!("SSH agent is not running or is not responding. {guidance} ({detail})"),
    )
}

fn agent_identity_error_code(bind_session: bool, error: &keys::Error) -> &'static str {
    if is_agent_identity_binding_disconnect(bind_session, error) {
        "sftp_agent_binding_rejected"
    } else if is_agent_identity_disconnect(error) {
        "sftp_agent_unavailable"
    } else {
        "sftp_agent_identities"
    }
}

fn agent_identity_error(bind_session: bool, error: &keys::Error) -> SftpError {
    let code = agent_identity_error_code(bind_session, error);
    if code == "sftp_agent_unavailable" {
        agent_not_running_error(error)
    } else {
        SftpError::new(code, error.to_string())
    }
}

const AGENT_REQUEST_TIMEOUT: Duration = Duration::from_secs(3);

async fn authenticate_with_agent<S>(
    handle: &mut Handle<SftpHostKeyHandler>,
    user: &str,
    mut agent: AgentClient<S>,
    bind_session: bool,
) -> Result<(), SftpError>
where
    S: AgentStream + Send + Unpin + 'static,
{
    if bind_session {
        let binding = handle.agent_session_binding().cloned().ok_or_else(|| {
            SftpError::new("sftp_agent_binding", "SSH agent session binding is unavailable.")
        })?;
        let bind_result = tokio::time::timeout(
            AGENT_REQUEST_TIMEOUT,
            agent.bind_session(&binding, false),
        )
        .await;
        match bind_result {
            Err(_) => {
                return Err(SftpError::new(
                    "sftp_agent_binding_rejected",
                    "SSH agent did not respond to the session-binding request.",
                ));
            }
            Ok(Err(error)) => {
                let legacy_fallback = is_legacy_agent_session_binding_failure(&error);
                return Err(SftpError::new(
                    if legacy_fallback { "sftp_agent_binding_rejected" } else { "sftp_agent_binding" },
                    error.to_string(),
                ));
            }
            Ok(Ok(())) => {}
        }
    }

    let identities = match tokio::time::timeout(
        AGENT_REQUEST_TIMEOUT,
        agent.request_identities(),
    )
    .await
    {
        Ok(Ok(identities)) => identities,
        Ok(Err(error)) => return Err(agent_identity_error(bind_session, &error)),
        Err(_) if bind_session => {
            return Err(SftpError::new(
                "sftp_agent_binding_rejected",
                "SSH agent did not respond to the identity-list request after session binding.",
            ));
        }
        Err(_) => {
            return Err(agent_not_running_error(
                "The SSH agent did not respond to the identity-list request within 3 seconds.",
            ));
        }
    };
    let rsa_hash = best_rsa_hash(handle).await?;

    for identity in identities {
        let public_key = match identity {
            AgentIdentity::PublicKey { key, .. } => key,
            // Certificate identities need the certificate-aware public-key
            // authentication request. Do not silently downgrade them.
            AgentIdentity::Certificate { .. } => continue,
        };
        // Russh implements its private Signer trait directly for AgentClient.
        // The trait stays encapsulated by Russh; map its typed agent error here.
        let result = handle
            .authenticate_publickey_with(user, public_key, rsa_hash, &mut agent)
            .await
            .map_err(|error| SftpError::new("sftp_agent_sign", error.to_string()))?;
        if result.success() {
            return Ok(());
        }
    }

    Err(SftpError::new(
        "sftp_auth_failed",
        "No available SSH-agent identity authenticated successfully.",
    ))
}

const DEFAULT_OPENSSH_AGENT_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";
const WINDOWS_PIPE_PREFIX: &str = r"\\.\pipe\";

/// Prefer the endpoint selected by the Windows SSH client environment.
/// OpenSSH and third-party agents can publish their named-pipe endpoint here.
fn resolve_windows_agent_pipe(candidate: Option<&str>) -> String {
    candidate
        .filter(|path| {
            path.get(..WINDOWS_PIPE_PREFIX.len())
                .is_some_and(|prefix| prefix.eq_ignore_ascii_case(WINDOWS_PIPE_PREFIX))
                && path.len() > WINDOWS_PIPE_PREFIX.len()
                && !path.chars().any(char::is_control)
        })
        .map(str::to_string)
        .unwrap_or_else(|| DEFAULT_OPENSSH_AGENT_PIPE.to_string())
}

#[cfg(windows)]
#[derive(Debug, Clone)]
enum WindowsAgentSource {
    NamedPipe { path: String },
    Pageant,
}

#[cfg(windows)]
type DynamicAgent = AgentClient<Box<dyn AgentStream + Send + Unpin>>;

#[cfg(windows)]
fn selected_windows_agent_source() -> WindowsAgentSource {
    let environment = std::env::var("SSH_AUTH_SOCK").ok();
    WindowsAgentSource::NamedPipe {
        path: resolve_windows_agent_pipe(environment.as_deref()),
    }
}

#[cfg(windows)]
async fn connect_windows_agent(source: &WindowsAgentSource) -> Result<DynamicAgent, SftpError> {
    match source {
        WindowsAgentSource::NamedPipe { path } => tokio::time::timeout(
            Duration::from_secs(2),
            AgentClient::connect_named_pipe(path),
        )
        .await
        .map_err(|_| SftpError::new("sftp_agent_timeout", "Windows SSH agent connection timed out."))?
        .map(|agent| agent.dynamic())
        .map_err(|error| SftpError::new("sftp_agent_unavailable", error.to_string())),
        WindowsAgentSource::Pageant => tokio::time::timeout(
            Duration::from_secs(2),
            AgentClient::connect_pageant(),
        )
        .await
        .map_err(|_| SftpError::new("sftp_agent_timeout", "Pageant connection timed out."))?
        .map(|agent| agent.dynamic())
        .map_err(|error| SftpError::new("sftp_agent_unavailable", error.to_string())),
    }
}

#[cfg(windows)]
async fn authenticate_agent(
    handle: &mut Handle<SftpHostKeyHandler>,
    user: &str,
) -> Result<AgentBindingStatus, SftpError> {
    // Honor SSH_AUTH_SOCK first (it may point to 1Password or another agent);
    // otherwise use the standard Win32 OpenSSH named pipe. Pageant is a fallback
    // when the selected named pipe cannot be opened.
    let selected = selected_windows_agent_source();
    let (agent, source) = match connect_windows_agent(&selected).await {
        Ok(agent) => (agent, selected),
        Err(pipe_error) => {
            let pageant = WindowsAgentSource::Pageant;
            match connect_windows_agent(&pageant).await {
                Ok(agent) => (agent, pageant),
                Err(pageant_error) => {
                    return Err(agent_not_running_error(format!(
                        "Windows SSH agent pipe unavailable: {}; Pageant unavailable: {}",
                        pipe_error.detail, pageant_error.detail
                    )));
                }
            }
        }
    };

    match authenticate_with_agent(handle, user, agent, true).await {
        Ok(()) => Ok(AgentBindingStatus::Bound),
        Err(error) if error.code == "sftp_agent_binding_rejected" => {
            // Windows OpenSSH 9.5 agents may return SSH_AGENT_FAILURE for the
            // session-bind extension while remaining usable for ordinary
            // identity listing and signing. Other legacy agents may close the
            // pipe instead. Reconnect to get a clean request stream, continue
            // unbound, and expose that reduced security state to the UI.
            let legacy_agent = connect_windows_agent(&source).await.map_err(|reconnect_error| {
                agent_not_running_error(format!(
                    "Agent closed during session binding ({error}); reconnect failed: {}",
                    reconnect_error.detail
                ))
            })?;
            authenticate_with_agent(handle, user, legacy_agent, false).await?;
            Ok(AgentBindingStatus::LegacyUnbound)
        }
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
async fn authenticate_agent(
    handle: &mut Handle<SftpHostKeyHandler>,
    user: &str,
) -> Result<AgentBindingStatus, SftpError> {
    let agent = AgentClient::connect_env()
        .await
        .map_err(|error| agent_not_running_error(error))?;

    match authenticate_with_agent(handle, user, agent, true).await {
        Ok(()) => Ok(AgentBindingStatus::Bound),
        Err(error) if error.code == "sftp_agent_binding_rejected" => {
            // Some Unix agents do not support the OpenSSH session-bind extension,
            // or disconnect while processing it. Reconnect to a clean stream and
            // retry once without binding, exposing the reduced security state.
            let legacy_agent = AgentClient::connect_env().await.map_err(|reconnect_error| {
                agent_not_running_error(format!(
                    "Agent closed during session binding ({error}); reconnect failed: {reconnect_error}"
                ))
            })?;
            authenticate_with_agent(handle, user, legacy_agent, false).await?;
            Ok(AgentBindingStatus::LegacyUnbound)
        }
        Err(error) => Err(error),
    }
}

#[cfg(not(any(windows, unix)))]
async fn authenticate_agent(
    _handle: &mut Handle<SftpHostKeyHandler>,
    _user: &str,
) -> Result<AgentBindingStatus, SftpError> {
    Err(SftpError::new(
        "sftp_agent_unsupported",
        "SSH-agent authentication is not supported on this platform.",
    ))
}

async fn authenticate(
    handle: &mut Handle<SftpHostKeyHandler>,
    connection: &Connection,
) -> Result<AgentBindingStatus, SftpError> {
    match &connection.auth {
        Auth::Password { password } if !password.is_empty() => {
            if handle
                .authenticate_password(&connection.user, password.clone())
                .await?
                .success()
            {
                Ok(AgentBindingStatus::NotUsed)
            } else {
                Err(SftpError::new("sftp_auth_failed", "SSH password authentication failed."))
            }
        }
        Auth::Key { path, passphrase } if !path.trim().is_empty() => {
            let private_key = keys::load_secret_key(path, passphrase.as_deref())
                .map_err(|error| SftpError::new("sftp_key_load", error.to_string()))?;
            let key = PrivateKeyWithHashAlg::new(
                Arc::new(private_key),
                best_rsa_hash(handle).await?,
            );
            if handle.authenticate_publickey(&connection.user, key).await?.success() {
                Ok(AgentBindingStatus::NotUsed)
            } else {
                Err(SftpError::new("sftp_auth_failed", "SSH private-key authentication failed."))
            }
        }
        Auth::Agent => authenticate_agent(handle, &connection.user).await,
        _ => Err(SftpError::new(
            "sftp_credentials_required",
            "The saved connection does not contain usable SSH credentials.",
        )),
    }
}

/// An authenticated SSH/SFTP connection. The SSH handles for every jump hop
/// are retained for as long as the target SFTP channel is alive.
pub struct SftpClient {
    sftp: SftpSession,
    listing_sftp: Option<Arc<RawSftpSession>>,
    sessions: Vec<Handle<SftpHostKeyHandler>>,
    start_dir: String,
    agent_binding: AgentBindingStatus,
}

impl SftpClient {
    pub fn session(&self) -> &SftpSession {
        &self.sftp
    }

    pub fn listing_session(&self) -> Option<Arc<RawSftpSession>> {
        self.listing_sftp.clone()
    }

    pub async fn current_directory(&self) -> Result<String, SftpError> {
        let path = if self.start_dir.is_empty() { "." } else { &self.start_dir };
        self.sftp
            .canonicalize(path)
            .await
            .map_err(|error| SftpError::new("sftp_protocol", error.to_string()))
    }

    pub async fn close(&self) -> Result<(), SftpError> {
        let mut first_error = self.listing_sftp.as_ref()
            .and_then(|listing| listing.close_session().err())
            .map(|error| SftpError::new("sftp_listing_close", error.to_string()));
        if let Err(error) = self.sftp.close().await {
            if first_error.is_none() {
                first_error = Some(SftpError::new("sftp_close", error.to_string()));
            }
        }
        for session in self.sessions.iter().rev() {
            if let Err(error) = session
                .disconnect(russh::Disconnect::ByApplication, "SSH Term SFTP closed", "en")
                .await
            {
                if first_error.is_none() {
                    first_error = Some(error.into());
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// Opens SFTP over a direct SSH connection or a sequence of saved jump hosts.
///
/// `jump_chain` must be in outermost-first order (the order returned by
/// `store::resolve_jump_chain`). Each hop authenticates with its own saved
/// credentials and is retained until the target SFTP session is closed.
pub async fn connect(
    connection: &Connection,
    jump_chain: &[Connection],
    known_hosts: impl AsRef<Path>,
) -> Result<SftpClient, SftpError> {
    let known_hosts = known_hosts.as_ref();
    let (target_host, target_port) = endpoint(connection, true)?;
    let mut sessions: Vec<Handle<SftpHostKeyHandler>> =
        Vec::with_capacity(jump_chain.len() + 1);
    let mut agent_binding = AgentBindingStatus::NotUsed;

    for hop in jump_chain {
        let (hop_host, hop_port) = endpoint(hop, false)?;
        let mut handle = if let Some(previous) = sessions.last() {
            let channel = previous
                .channel_open_direct_tcpip(&hop_host, u32::from(hop_port), "127.0.0.1", 0)
                .await?;
            connect_channel(channel.into_stream(), &hop_host, hop_port, hop.keepalive_secs, known_hosts).await?
        } else {
            connect_tcp(&hop_host, hop_port, hop.keepalive_secs, known_hosts).await?
        };
        agent_binding = agent_binding.merge(authenticate(&mut handle, hop).await?);
        sessions.push(handle);
    }

    let mut target = if let Some(previous) = sessions.last() {
        let channel = previous
            .channel_open_direct_tcpip(&target_host, u32::from(target_port), "127.0.0.1", 0)
            .await?;
        connect_channel(
            channel.into_stream(),
            &target_host,
            target_port,
            connection.keepalive_secs,
            known_hosts,
        )
        .await?
    } else {
        connect_tcp(
            &target_host,
            target_port,
            connection.keepalive_secs,
            known_hosts,
        )
        .await?
    };

    agent_binding = agent_binding.merge(authenticate(&mut target, connection).await?);
    let channel = target.channel_open_session().await?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(SftpError::from)?;
    // Keep a bounded request window per file. The library pipelines writes and
    // reads internally; opening many handles on top of that only multiplies
    // outstanding requests and can starve directory operations on the same channel.
    let sftp = SftpSession::new_with_config(
        channel.into_stream(),
        SftpConfig {
            max_concurrent_reads: 16,
            max_concurrent_writes: 16,
            max_write_packet_len: 256 * 1024,
            ..SftpConfig::default()
        },
    )
    .await
    .map_err(|error| SftpError::new("sftp_subsystem", error.to_string()))?;

    // Directory requests get their own SSH/SFTP channel. A slow recursive file
    // transfer can then no longer queue ahead of READDIR responses.
    let listing_sftp = if let Ok(listing_channel) = target.channel_open_session().await {
        if listing_channel.request_subsystem(true, "sftp").await.is_ok() {
            let raw = RawSftpSession::new(listing_channel.into_stream());
            match raw.init().await {
                Ok(_) => Some(Arc::new(raw)),
                Err(_) => {
                    let _ = raw.close_session();
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };
    sessions.push(target);

    let start_dir = connection
        .sftp
        .as_ref()
        .map(|options| options.start_dir.trim().to_string())
        .filter(|path| !path.is_empty())
        .unwrap_or_default();

    Ok(SftpClient { sftp, listing_sftp, sessions, start_dir, agent_binding })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST_KEY: &str =
        "AAAAC3NzaC1lZDI1NTE5AAAAIJdD7y3aLq454yWBdwLWbieU1ebz9/cu7/QEXn9OIeZJ";
    const CHANGED_KEY: &str =
        "AAAAC3NzaC1lZDI1NTE5AAAAIA6rWI3G1sz07DnfFlrouTcysQlj2P+jpNSOEWD9OJ3X";

    #[test]
    fn unknown_host_is_learned_and_then_matched() {
        let directory = tempfile::tempdir().unwrap();
        let known_hosts = directory.path().join("known_hosts");
        let key = keys::parse_public_key_base64(FIRST_KEY).unwrap();

        assert!(verify_or_learn_host_key("sftp.example", 22, &key, &known_hosts).unwrap());
        assert!(verify_or_learn_host_key("sftp.example", 22, &key, &known_hosts).unwrap());
    }

    #[test]
    fn changed_host_key_is_rejected_without_replacing_the_trusted_key() {
        let directory = tempfile::tempdir().unwrap();
        let known_hosts = directory.path().join("known_hosts");
        let first = keys::parse_public_key_base64(FIRST_KEY).unwrap();
        let changed = keys::parse_public_key_base64(CHANGED_KEY).unwrap();

        verify_or_learn_host_key("sftp.example", 2207, &first, &known_hosts).unwrap();
        let result = verify_or_learn_host_key("sftp.example", 2207, &changed, &known_hosts);

        assert!(matches!(result, Err(SftpError { code: "sftp_host_key_changed", .. })));
        assert!(verify_or_learn_host_key("sftp.example", 2207, &first, &known_hosts).unwrap());
    }

    #[test]
    fn non_default_port_is_written_in_openssh_known_hosts_format() {
        let directory = tempfile::tempdir().unwrap();
        let known_hosts = directory.path().join("known_hosts");
        let key = keys::parse_public_key_base64(FIRST_KEY).unwrap();

        verify_or_learn_host_key("sftp.example", 2207, &key, &known_hosts).unwrap();

        let saved = std::fs::read_to_string(known_hosts).unwrap();
        let first_entry = saved.lines().find(|line| !line.is_empty()).unwrap();
        assert!(first_entry.starts_with("[sftp.example]:2207 ssh-ed25519 "));
    }

    #[test]
    fn explicitly_revoked_host_key_is_rejected_before_tofu_learning() {
        let directory = tempfile::tempdir().unwrap();
        let known_hosts = directory.path().join("known_hosts");
        std::fs::write(
            &known_hosts,
            format!("@revoked sftp.example ssh-ed25519 {FIRST_KEY}\n"),
        ).unwrap();
        let key = keys::parse_public_key_base64(FIRST_KEY).unwrap();

        let result = verify_or_learn_host_key("sftp.example", 22, &key, &known_hosts);
        assert!(matches!(result, Err(SftpError { code: "sftp_host_key_revoked", .. })));
        assert_eq!(std::fs::read_to_string(known_hosts).unwrap().lines().count(), 1);
    }

    #[test]
    fn hashed_revoked_host_key_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let known_hosts = directory.path().join("known_hosts");
        std::fs::write(
            &known_hosts,
            format!(
                "@revoked |1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF\n"
            ),
        ).unwrap();
        let key = keys::parse_public_key_base64(
            "AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF",
        ).unwrap();

        let result = verify_or_learn_host_key("example.com", 22, &key, &known_hosts);
        assert!(matches!(result, Err(SftpError { code: "sftp_host_key_revoked", .. })));
    }

    #[test]
    fn revoked_host_patterns_honor_wildcards_and_negations() {
        assert!(host_patterns_match("db.example.com", "*.example.com,!legacy.example.com"));
        assert!(!host_patterns_match("legacy.example.com", "*.example.com,!legacy.example.com"));
        assert!(!host_patterns_match("other.test", "*.example.com,!legacy.example.com"));
    }

    #[test]
    fn hashed_known_host_entry_is_matched() {
        let directory = tempfile::tempdir().unwrap();
        let known_hosts = directory.path().join("known_hosts");
        std::fs::write(
            &known_hosts,
            "|1|O33ESRMWPVkMYIwJ1Uw+n877jTo=|nuuC5vEqXlEZ/8BXQR7m619W6Ak= ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF\n",
        ).unwrap();
        let key = keys::parse_public_key_base64(
            "AAAAC3NzaC1lZDI1NTE5AAAAILIG2T/B0l0gaqj3puu510tu9N1OkQ4znY3LYuEm5zCF",
        ).unwrap();

        assert!(verify_or_learn_host_key("example.com", 22, &key, &known_hosts).unwrap());
    }

    #[test]
    fn endpoint_uses_target_sftp_override_but_not_for_jump_hops() {
        let target: Connection = serde_json::from_value(serde_json::json!({
            "id": "target",
            "host": "target.example",
            "user": "admin",
            "port": 22,
            "sftp": { "port": 2222, "start_dir": "" }
        })).unwrap();
        let hop: Connection = serde_json::from_value(serde_json::json!({
            "id": "jump",
            "host": "jump.example",
            "user": "bastion",
            "port": 2201,
            "sftp": { "port": 2202, "start_dir": "" }
        })).unwrap();
        assert_eq!(endpoint(&target, true).unwrap(), ("target.example".into(), 2222));
        assert_eq!(endpoint(&hop, false).unwrap(), ("jump.example".into(), 2201));
    }
}


// M8b: managed runtime sessions and typed directory browsing.
#[derive(Default)]
pub struct SftpState {
    gate: RwLock<()>,
    sessions: Mutex<HashMap<String, Arc<SftpClient>>>,
    transfers: Arc<Mutex<HashMap<String, SftpTransferControl>>>,
}

struct SftpTransferControl {
    session_id: String,
    cancel: watch::Sender<bool>,
    done: watch::Receiver<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SftpSessionInfo {
    pub session_id: String,
    pub connection_id: String,
    pub connection_name: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub jump_names: Vec<String>,
    pub current_path: String,
    pub agent_binding: AgentBindingStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct SftpEntry {
    pub name: String,
    pub path: String,
    pub kind: String,
    pub size: Option<u64>,
    /// Unix timestamp in seconds, as reported by the remote server.
    pub modified: Option<u64>,
    pub permissions: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SftpDirectory {
    pub current_path: String,
    pub parent_path: Option<String>,
    pub entries: Vec<SftpEntry>,
}

fn new_session_id() -> String {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Remote paths use POSIX separators regardless of the local Windows host.
fn validate_remote_path(path: &str) -> Result<(), SftpError> {
    if path.is_empty()
        || path.len() > 4096
        || path.chars().any(char::is_control)
    {
        return Err(SftpError::new(
            "sftp_bad_path",
            "The remote path is empty, too long, or contains control characters.",
        ));
    }
    Ok(())
}

fn remote_parent(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "/" {
        return None;
    }
    match trimmed.rfind('/') {
        Some(0) => Some("/".to_string()),
        Some(index) => Some(trimmed[..index].to_string()),
        None => Some(".".to_string()),
    }
}

fn remote_child(parent: &str, name: &str) -> String {
    if parent == "/" {
        format!("/{name}")
    } else {
        format!("{}/{name}", parent.trim_end_matches('/'))
    }
}

fn validate_remote_name(name: &str) -> Result<(), SftpError> {
    if name.is_empty()
        || name.len() > 255
        || name == "."
        || name == ".."
        || name.contains('/')
        || name.contains('\\')
        || name.chars().any(char::is_control)
    {
        return Err(SftpError::new(
            "sftp_bad_name",
            "Use a single file or directory name without path separators or control characters.",
        ));
    }
    Ok(())
}

async fn canonical_sibling(
    session: &SftpSession,
    remote_path: &str,
) -> Result<(String, String), SftpError> {
    validate_remote_path(remote_path)?;
    let trimmed = remote_path.trim_end_matches('/');
    let name = trimmed.rsplit('/').next().unwrap_or_default();
    validate_remote_name(name)?;
    let parent = remote_parent(trimmed).unwrap_or_else(|| "/".to_string());
    let canonical_parent = session
        .canonicalize(parent)
        .await
        .map_err(|error| SftpError::new("sftp_protocol", error.to_string()))?;
    Ok((canonical_parent, name.to_string()))
}

impl SftpState {
    async fn close_all_inner(&self) -> Result<(), String> {
        // Prevent new work, cancel and reap transfers, then close every SFTP/SSH
        // handle before the vault can be locked or the app exits.
        let _lifecycle = self.gate.write().await;
        let pending = {
            let transfers = self.transfers.lock().await;
            transfers.values().map(|item| {
                let _ = item.cancel.send(true);
                item.done.clone()
            }).collect::<Vec<_>>()
        };
        wait_for_transfers(pending).await;
        let sessions = std::mem::take(&mut *self.sessions.lock().await);
        let mut first_error = None;
        for (_, session) in sessions {
            if let Err(error) = session.close().await {
                if first_error.is_none() {
                    first_error = Some(format!("{}: {}", error.code, error.detail));
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }
}

/// Close all native SFTP sessions. Used for vault lock and app shutdown.
pub async fn close_all(state: &SftpState) -> Result<(), String> {
    state.close_all_inner().await
}

#[tauri::command]
pub async fn sftp_open(
    state: State<'_, SftpState>,
    vault_state: State<'_, crate::commands::VaultState>,
    connection_id: String,
) -> Result<SftpSessionInfo, String> {
    let _lifecycle = state.gate.read().await;
    let (connection, jump_chain, jump_names) = {
        let guard = vault_state.0.lock().unwrap_or_else(|error| error.into_inner());
        let vault = guard.as_ref().ok_or_else(|| "locked".to_string())?;
        let connection = crate::store::find(vault, &connection_id)
            .ok_or_else(|| "connection_not_found".to_string())?;
        if connection.port == 0 || !crate::store::valid_address(&connection.user, &connection.host) {
            return Err("bad_address".to_string());
        }
        let jump_chain = crate::store::resolve_jump_chain(&vault.data.connections, &connection_id)
            .map_err(str::to_string)?;
        let jump_names = jump_chain.iter().map(|hop| hop.name.clone()).collect::<Vec<_>>();
        (connection, jump_chain, jump_names)
    };

    let client = connect(&connection, &jump_chain, crate::paths::known_hosts_path())
        .await
        .map_err(|error| format!("{}: {}", error.code, error.detail))?;
    let current_path = client.current_directory().await
        .map_err(|error| format!("{}: {}", error.code, error.detail))?;
    let session_id = new_session_id();
    let port = connection.effective_sftp_port();
    let agent_binding = client.agent_binding;
    state.sessions.lock().await.insert(session_id.clone(), Arc::new(client));

    Ok(SftpSessionInfo {
        session_id,
        connection_id: connection.id,
        connection_name: connection.name,
        host: connection.host,
        port,
        user: connection.user,
        jump_names,
        current_path,
        agent_binding,
    })
}

const DIRECTORY_READ_CONCURRENCY: usize = 4;

async fn read_directory_sequential_raw(
    session: &RawSftpSession,
    path: String,
) -> Result<Vec<(String, Metadata)>, String> {
    let opened = session.opendir(path).await
        .map_err(|error| format!("sftp_protocol: {error}"))?;
    let handle = opened.handle;
    let mut entries = Vec::new();

    loop {
        match session.readdir(handle.clone()).await {
            Ok(page) => {
                entries.extend(page.files.into_iter().filter_map(|entry| {
                    let name = entry.filename.to_string();
                    if name == "." || name == ".." {
                        None
                    } else {
                        Some((name, entry.attrs))
                    }
                }));
            }
            Err(SftpClientError::Status(status)) if status.status_code == StatusCode::Eof => break,
            Err(error) => {
                let _ = session.close(handle).await;
                return Err(format!("sftp_protocol: {error}"));
            }
        }
    }

    session.close(handle).await
        .map_err(|error| format!("sftp_protocol: directory close failed: {error}"))?;
    Ok(entries)
}

async fn read_directory_pipelined(
    session: Arc<RawSftpSession>,
    path: String,
) -> Result<Vec<(String, Metadata)>, String> {
    let opened = session.opendir(path.clone()).await
        .map_err(|error| format!("sftp_protocol: {error}"))?;
    let handle = opened.handle;
    let mut tasks = JoinSet::new();

    for _ in 0..DIRECTORY_READ_CONCURRENCY {
        let session = Arc::clone(&session);
        let handle = handle.clone();
        tasks.spawn(async move { session.readdir(handle).await });
    }

    let mut entries = Vec::new();
    let mut eof = false;
    let mut failure = None;

    while let Some(joined) = tasks.join_next().await {
        let result = match joined {
            Ok(result) => result,
            Err(error) => {
                failure = Some(format!("sftp_protocol: directory worker failed: {error}"));
                break;
            }
        };
        match result {
            Ok(page) => {
                entries.extend(page.files.into_iter().filter_map(|entry| {
                    let name = entry.filename.to_string();
                    if name == "." || name == ".." {
                        None
                    } else {
                        Some((name, entry.attrs))
                    }
                }));
                if !eof {
                    let session = Arc::clone(&session);
                    let handle = handle.clone();
                    tasks.spawn(async move { session.readdir(handle).await });
                }
            }
            Err(SftpClientError::Status(status)) if status.status_code == StatusCode::Eof => {
                eof = true;
            }
            Err(error) => {
                failure = Some(format!("sftp_protocol: {error}"));
                break;
            }
        }
    }

    if failure.is_some() {
        tasks.abort_all();
    }
    while tasks.join_next().await.is_some() {}

    let close_result = session.close(handle).await
        .map_err(|error| format!("sftp_protocol: directory close failed: {error}"));
    if let Some(error) = failure {
        let _ = close_result;
        // Some servers support SFTP but reject concurrent READDIR requests
        // against one handle. Retry on the same isolated channel sequentially.
        return read_directory_sequential_raw(&session, path).await
            .or(Err(error));
    }
    close_result?;
    Ok(entries)
}

fn is_normalized_absolute_remote_path(path: &str) -> bool {
    path.starts_with('/')
        && !path.split('/').any(|component| matches!(component, "." | ".."))
}

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, SftpState>,
    session_id: String,
    remote_path: Option<String>,
) -> Result<SftpDirectory, String> {
    let _lifecycle = state.gate.read().await;
    let session = state.sessions.lock().await.get(&session_id).cloned()
        .ok_or_else(|| "sftp_session_not_found".to_string())?;
    let client = session;
    let requested = remote_path.unwrap_or_else(|| ".".to_string());
    validate_remote_path(&requested)
        .map_err(|error| format!("{}: {}", error.code, error.detail))?;
    // Paths returned by this browser are already absolute and normalized.
    // Avoid an extra REALPATH round trip on every directory double-click.
    let current_path = if is_normalized_absolute_remote_path(&requested) {
        requested
    } else {
        client.session().canonicalize(requested).await
            .map_err(|error| format!("sftp_protocol: {error}"))?
    };
    let directory_entries = if let Some(listing_session) = client.listing_session() {
        read_directory_pipelined(listing_session, current_path.clone()).await?
    } else {
        client.session().read_dir(current_path.clone()).await
            .map_err(|error| format!("sftp_protocol: {error}"))?
            .map(|entry| (entry.file_name().to_string(), entry.metadata()))
            .collect()
    };

    let mut entries = Vec::new();
    for (name, metadata) in directory_entries {
        if name == "." || name == ".." {
            continue;
        }
        let kind = if metadata.is_dir() {
            "directory"
        } else if metadata.is_symlink() {
            "symlink"
        } else if metadata.is_regular() {
            "file"
        } else {
            "other"
        };
        entries.push(SftpEntry {
            path: remote_child(&current_path, &name),
            name,
            kind: kind.to_string(),
            size: metadata.size,
            modified: metadata.mtime.map(u64::from),
            permissions: metadata.permissions,
        });
    }
    entries.sort_by(|left, right| {
        (left.kind != "directory")
            .cmp(&(right.kind != "directory"))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });

    Ok(SftpDirectory {
        parent_path: remote_parent(&current_path),
        current_path,
        entries,
    })
}

#[tauri::command]
pub async fn sftp_mkdir(
    state: State<'_, SftpState>,
    session_id: String,
    remote_path: String,
    name: String,
) -> Result<(), String> {
    let _lifecycle = state.gate.read().await;
    validate_remote_name(&name).map_err(|error| format!("{}: {}", error.code, error.detail))?;
    validate_remote_path(&remote_path).map_err(|error| format!("{}: {}", error.code, error.detail))?;
    let session = state.sessions.lock().await.get(&session_id).cloned()
        .ok_or_else(|| "sftp_session_not_found".to_string())?;
    let client = session;
    let parent = client.session().canonicalize(remote_path).await
        .map_err(|error| format!("sftp_protocol: {error}"))?;
    let target = remote_child(&parent, &name);
    if client.session().try_exists(target.clone()).await
        .map_err(|error| format!("sftp_protocol: {error}"))?
    {
        return Err(format!("sftp_already_exists: {name}"));
    }
    client.session().create_dir(target).await
        .map_err(|error| format!("sftp_protocol: {error}"))
}

#[tauri::command]
pub async fn sftp_rename(
    state: State<'_, SftpState>,
    session_id: String,
    remote_path: String,
    new_name: String,
) -> Result<(), String> {
    let _lifecycle = state.gate.read().await;
    validate_remote_name(&new_name).map_err(|error| format!("{}: {}", error.code, error.detail))?;
    let session = state.sessions.lock().await.get(&session_id).cloned()
        .ok_or_else(|| "sftp_session_not_found".to_string())?;
    let client = session;
    let (parent, old_name) = canonical_sibling(client.session(), &remote_path).await
        .map_err(|error| format!("{}: {}", error.code, error.detail))?;
    if old_name == new_name {
        return Ok(());
    }
    let old_path = remote_child(&parent, &old_name);
    let new_path = remote_child(&parent, &new_name);
    if client.session().try_exists(new_path.clone()).await
        .map_err(|error| format!("sftp_protocol: {error}"))?
    {
        return Err(format!("sftp_already_exists: {new_name}"));
    }
    client.session().rename(old_path, new_path).await
        .map_err(|error| format!("sftp_protocol: {error}"))
}

async fn delete_remote_entry(session: &SftpSession, remote_path: &str) -> Result<(), String> {
    let (parent, name) = canonical_sibling(session, remote_path).await
        .map_err(|error| format!("{}: {}", error.code, error.detail))?;
    let target = remote_child(&parent, &name);
    let metadata = session.symlink_metadata(target.clone()).await
        .map_err(|error| format!("sftp_protocol: {error}"))?;
    // Symlinks are unlinked, never followed. Directories are removed only when empty.
    if metadata.is_symlink() || metadata.is_regular() {
        session.remove_file(target).await
            .map_err(|error| format!("sftp_protocol: {error}"))
    } else if metadata.is_dir() {
        session.remove_dir(target).await
            .map_err(|error| format!("sftp_protocol: {error}"))
    } else {
        Err("sftp_delete_unsupported: This remote entry type cannot be deleted safely.".to_string())
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SftpDeleteFailure {
    pub path: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SftpDeleteResult {
    pub deleted: Vec<String>,
    pub failed: Vec<SftpDeleteFailure>,
}

#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, SftpState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let _lifecycle = state.gate.read().await;
    let session = state.sessions.lock().await.get(&session_id).cloned()
        .ok_or_else(|| "sftp_session_not_found".to_string())?;
    let client = session;
    delete_remote_entry(client.session(), &remote_path).await
}

async fn delete_remote_entries(session: &SftpSession, remote_paths: Vec<String>) -> SftpDeleteResult {
    let mut result = SftpDeleteResult { deleted: Vec::new(), failed: Vec::new() };
    let mut seen = HashSet::new();
    for path in remote_paths {
        if !seen.insert(path.clone()) {
            continue;
        }
        match delete_remote_entry(session, &path).await {
            Ok(()) => result.deleted.push(path),
            Err(detail) => result.failed.push(SftpDeleteFailure { path, detail }),
        }
    }
    result
}

#[tauri::command]
pub async fn sftp_delete_many(
    state: State<'_, SftpState>,
    session_id: String,
    remote_paths: Vec<String>,
) -> Result<SftpDeleteResult, String> {
    if remote_paths.len() > 10_000 {
        return Err("sftp_delete_limit: Select no more than 10,000 items at once.".to_string());
    }
    let _lifecycle = state.gate.read().await;
    let session = state.sessions.lock().await.get(&session_id).cloned()
        .ok_or_else(|| "sftp_session_not_found".to_string())?;
    let client = session;
    Ok(delete_remote_entries(client.session(), remote_paths).await)
}

#[tauri::command]
pub async fn sftp_close(
    state: State<'_, SftpState>,
    session_id: String,
) -> Result<(), String> {
    let _lifecycle = state.gate.write().await;
    let pending = {
        let transfers = state.transfers.lock().await;
        transfers.values().filter(|item| item.session_id == session_id).map(|item| {
            let _ = item.cancel.send(true);
            item.done.clone()
        }).collect::<Vec<_>>()
    };
    wait_for_transfers(pending).await;
    let session = state.sessions.lock().await.remove(&session_id)
        .ok_or_else(|| "sftp_session_not_found".to_string())?;
    session.close().await
        .map_err(|error| format!("{}: {}", error.code, error.detail))
}

#[tauri::command]
pub async fn sftp_close_all(state: State<'_, SftpState>) -> Result<(), String> {
    state.close_all_inner().await
}

// Match russh-sftp's default maximum SFTP read/write request length (255 KiB).
const TRANSFER_CHUNK_SIZE: usize = 255 * 1024;
const MAX_TRANSFER_ENTRIES: usize = 100_000;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SftpTransferStatus { Running, Completed, Cancelled, Failed }

#[derive(Debug, Clone, Serialize)]
pub struct SftpTransferProgress {
    pub transfer_id: String,
    pub session_id: String,
    pub direction: String,
    pub status: SftpTransferStatus,
    pub current_file: String,
    pub files_completed: u64,
    pub files_total: u64,
    pub bytes_transferred: u64,
    pub bytes_total: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SftpTransferStartResult {
    pub transfer_id: Option<String>,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TransferKind { Directory, File }

#[derive(Debug, Clone)]
struct TransferItem {
    source: String,
    destination: String,
    kind: TransferKind,
    size: u64,
}

#[derive(Debug)]
enum TransferFailure { Cancelled, Failed(String) }

impl TransferFailure {
    fn failed(code: &str, detail: impl std::fmt::Display) -> Self {
        Self::Failed(format!("{code}: {detail}"))
    }
}

async fn wait_for_transfers(mut pending: Vec<watch::Receiver<bool>>) {
    for done in &mut pending {
        while !*done.borrow() {
            if done.changed().await.is_err() { break; }
        }
    }
}

type TransferEmitter = dyn Fn(&SftpTransferProgress) + Send + Sync;

fn emit_transfer(emitter: &TransferEmitter, progress: &SftpTransferProgress) {
    emitter(progress);
}

fn local_name(path: &std::path::Path) -> Result<String, String> {
    let name = path.file_name().ok_or_else(|| "sftp_transfer_bad_local_path: Missing file name.".to_string())?;
    let name = name.to_str().ok_or_else(|| "sftp_transfer_bad_local_path: The local file name is not valid Unicode.".to_string())?.to_string();
    validate_remote_name(&name).map_err(|error| format!("{}: {}", error.code, error.detail))?;
    Ok(name)
}

/// Remote names must also be safe local path components on Windows. A malicious
/// SFTP server must not be able to turn a download name into a drive/path prefix
/// or an alternate data stream.
fn validate_local_component(name: &str) -> Result<(), String> {
    let invalid = name.is_empty()
        || name == "."
        || name == ".."
        || name.ends_with(' ')
        || name.ends_with('.')
        || name.chars().any(|character| {
            character.is_control() || matches!(character, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
        });
    let stem = name.split('.').next().unwrap_or_default().trim_end_matches(|character| character == ' ' || character == '.').to_ascii_uppercase();
    let numbered_device = |prefix: &str| {
        stem.strip_prefix(prefix).is_some_and(|suffix| {
            suffix.len() == 1 && matches!(suffix.as_bytes()[0], b'1'..=b'9')
        })
    };
    let reserved_device = matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$")
        || numbered_device("COM") || numbered_device("LPT");
    if invalid || reserved_device {
        return Err(format!("sftp_transfer_local_name_unsupported: Remote name cannot be represented safely as a Windows file name: {name}"));
    }
    Ok(())
}

fn remote_name(path: &str) -> Result<String, String> {
    let name = path.trim_end_matches('/').rsplit('/').next().unwrap_or_default().to_string();
    validate_remote_name(&name).map_err(|error| format!("{}: {}", error.code, error.detail))?;
    validate_local_component(&name)?;
    Ok(name)
}

async fn remote_target_exists(sftp: &SftpSession, path: &str) -> Result<Option<Metadata>, String> {
    if !sftp.try_exists(path.to_string()).await.map_err(|error| format!("sftp_protocol: {error}"))? {
        return Ok(None);
    }
    let metadata = sftp.symlink_metadata(path.to_string()).await
        .map_err(|error| format!("sftp_protocol: {error}"))?;
    Ok(Some(metadata))
}

async fn local_target_metadata(path: &std::path::Path) -> Result<Option<std::fs::Metadata>, String> {
    match tokio::fs::symlink_metadata(path).await {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("sftp_transfer_local_io: {error}")),
    }
}

fn push_transfer_item(
    items: &mut Vec<TransferItem>,
    destinations: &mut HashSet<String>,
    item: TransferItem,
) -> Result<(), String> {
    if items.len() >= MAX_TRANSFER_ENTRIES {
        return Err(format!("sftp_transfer_too_many_entries: A transfer is limited to {MAX_TRANSFER_ENTRIES} entries."));
    }
    if !destinations.insert(item.destination.clone()) {
        return Err(format!("sftp_transfer_duplicate_destination: Multiple selected items map to the same destination: {}", item.destination));
    }
    items.push(item);
    Ok(())
}

async fn build_upload_plan(
    sftp: &SftpSession,
    local_sources: &[String],
    remote_destination: &str,
) -> Result<(Vec<TransferItem>, Vec<String>), String> {
    if local_sources.is_empty() { return Err("sftp_transfer_empty: Select at least one local file or folder.".into()); }
    validate_remote_path(remote_destination).map_err(|error| format!("{}: {}", error.code, error.detail))?;
    let destination = sftp.canonicalize(remote_destination.to_string()).await
        .map_err(|error| format!("sftp_protocol: {error}"))?;
    let destination_meta = sftp.metadata(destination.clone()).await
        .map_err(|error| format!("sftp_protocol: {error}"))?;
    if !destination_meta.is_dir() { return Err("sftp_transfer_destination_not_directory: Choose a remote directory for uploads.".into()); }

    let mut items = Vec::new();
    let mut destinations = HashSet::new();
    let mut stack: Vec<(std::path::PathBuf, String)> = Vec::new();
    for source in local_sources {
        let path = std::path::PathBuf::from(source);
        let metadata = tokio::fs::symlink_metadata(&path).await
            .map_err(|error| format!("sftp_transfer_local_io: {source}: {error}"))?;
        if metadata.file_type().is_symlink() { return Err(format!("sftp_transfer_symlink_unsupported: Local symbolic links are not followed: {source}")); }
        let target = remote_child(&destination, &local_name(&path)?);
        if metadata.is_dir() {
            push_transfer_item(&mut items, &mut destinations, TransferItem { source: source.clone(), destination: target.clone(), kind: TransferKind::Directory, size: 0 })?;
            stack.push((path, target));
        } else if metadata.is_file() {
            push_transfer_item(&mut items, &mut destinations, TransferItem { source: source.clone(), destination: target, kind: TransferKind::File, size: metadata.len() })?;
        } else {
            return Err(format!("sftp_transfer_type_unsupported: Unsupported local item: {source}"));
        }
    }
    while let Some((directory, remote_directory)) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&directory).await
            .map_err(|error| format!("sftp_transfer_local_io: {}: {error}", directory.display()))?;
        while let Some(entry) = entries.next_entry().await
            .map_err(|error| format!("sftp_transfer_local_io: {}: {error}", directory.display()))? {
            let path = entry.path();
            let metadata = tokio::fs::symlink_metadata(&path).await
                .map_err(|error| format!("sftp_transfer_local_io: {}: {error}", path.display()))?;
            if metadata.file_type().is_symlink() { return Err(format!("sftp_transfer_symlink_unsupported: Local symbolic links are not followed: {}", path.display())); }
            let target = remote_child(&remote_directory, &local_name(&path)?);
            if metadata.is_dir() {
                push_transfer_item(&mut items, &mut destinations, TransferItem { source: path.to_string_lossy().into_owned(), destination: target.clone(), kind: TransferKind::Directory, size: 0 })?;
                stack.push((path, target));
            } else if metadata.is_file() {
                push_transfer_item(&mut items, &mut destinations, TransferItem { source: path.to_string_lossy().into_owned(), destination: target, kind: TransferKind::File, size: metadata.len() })?;
            } else {
                return Err(format!("sftp_transfer_type_unsupported: Unsupported local item: {}", path.display()));
            }
        }
    }

    let mut conflicts = Vec::new();
    for item in &items {
        if let Some(existing) = remote_target_exists(sftp, &item.destination).await? {
            if existing.is_symlink() { return Err(format!("sftp_transfer_symlink_conflict: Refusing to replace a remote symbolic link: {}", item.destination)); }
            match item.kind {
                TransferKind::Directory if existing.is_dir() => {}
                TransferKind::File if existing.is_regular() => conflicts.push(item.destination.clone()),
                _ => return Err(format!("sftp_transfer_type_conflict: Remote item type differs at {}", item.destination)),
            }
        }
    }
    Ok((items, conflicts))
}

async fn build_download_plan(
    sftp: &SftpSession,
    listing_sftp: Option<Arc<RawSftpSession>>,
    remote_sources: &[String],
    local_destination: &str,
) -> Result<(Vec<TransferItem>, Vec<String>), String> {
    if remote_sources.is_empty() { return Err("sftp_transfer_empty: Select at least one remote file or folder.".into()); }
    let local_root = std::path::PathBuf::from(local_destination);
    let root_metadata = tokio::fs::symlink_metadata(&local_root).await
        .map_err(|error| format!("sftp_transfer_local_io: {}: {error}", local_root.display()))?;
    if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
        return Err("sftp_transfer_destination_not_directory: Choose an existing local folder for downloads.".into());
    }

    let mut items = Vec::new();
    let mut destinations = HashSet::new();
    let mut stack: Vec<(String, std::path::PathBuf)> = Vec::new();
    let mut seen = HashSet::new();
    for source in remote_sources {
        validate_remote_path(source).map_err(|error| format!("{}: {}", error.code, error.detail))?;
        if !seen.insert(source.clone()) { continue; }
        let metadata = remote_target_exists(sftp, source).await?
            .ok_or_else(|| format!("sftp_transfer_source_missing: Remote item no longer exists: {source}"))?;
        if metadata.is_symlink() { return Err(format!("sftp_transfer_symlink_unsupported: Remote symbolic links are not followed: {source}")); }
        let target = local_root.join(remote_name(source)?);
        if metadata.is_dir() {
            push_transfer_item(&mut items, &mut destinations, TransferItem { source: source.clone(), destination: target.to_string_lossy().into_owned(), kind: TransferKind::Directory, size: 0 })?;
            stack.push((source.clone(), target));
        } else if metadata.is_regular() {
            push_transfer_item(&mut items, &mut destinations, TransferItem { source: source.clone(), destination: target.to_string_lossy().into_owned(), kind: TransferKind::File, size: metadata.size.unwrap_or(0) })?;
        } else {
            return Err(format!("sftp_transfer_type_unsupported: Unsupported remote item: {source}"));
        }
    }
    while let Some((remote_directory, local_directory)) = stack.pop() {
        let entries = if let Some(listing_session) = &listing_sftp {
            read_directory_pipelined(Arc::clone(listing_session), remote_directory.clone()).await?
        } else {
            sftp.read_dir(remote_directory.clone()).await
                .map_err(|error| format!("sftp_protocol: {error}"))?
                .map(|entry| (entry.file_name().to_string(), entry.metadata()))
                .collect()
        };
        for (name, metadata) in entries {
            if name == "." || name == ".." { continue; }
            validate_remote_name(&name).map_err(|error| format!("{}: {}", error.code, error.detail))?;
            validate_local_component(&name)?;
            let remote_path = remote_child(&remote_directory, &name);
            let local_path = local_directory.join(&name);
            if metadata.is_symlink() { return Err(format!("sftp_transfer_symlink_unsupported: Remote symbolic links are not followed: {remote_path}")); }
            if metadata.is_dir() {
                push_transfer_item(&mut items, &mut destinations, TransferItem { source: remote_path.clone(), destination: local_path.to_string_lossy().into_owned(), kind: TransferKind::Directory, size: 0 })?;
                stack.push((remote_path, local_path));
            } else if metadata.is_regular() {
                push_transfer_item(&mut items, &mut destinations, TransferItem { source: remote_path, destination: local_path.to_string_lossy().into_owned(), kind: TransferKind::File, size: metadata.size.unwrap_or(0) })?;
            } else {
                return Err(format!("sftp_transfer_type_unsupported: Unsupported remote item: {remote_path}"));
            }
        }
    }

    let mut conflicts = Vec::new();
    for item in &items {
        let path = std::path::Path::new(&item.destination);
        if let Some(existing) = local_target_metadata(path).await? {
            if existing.file_type().is_symlink() { return Err(format!("sftp_transfer_symlink_conflict: Refusing to replace a local symbolic link: {}", path.display())); }
            match item.kind {
                TransferKind::Directory if existing.is_dir() => {}
                TransferKind::File if existing.is_file() => conflicts.push(item.destination.clone()),
                _ => return Err(format!("sftp_transfer_type_conflict: Local item type differs at {}", path.display())),
            }
        }
    }
    Ok((items, conflicts))
}

fn remote_temp_sibling(path: &str, transfer_id: &str, index: usize, suffix: &str) -> Result<String, String> {
    let parent = remote_parent(path).unwrap_or_else(|| "/".to_string());
    let name = format!(".sshterm-{transfer_id}-{index}.{suffix}");
    validate_remote_name(&name).map_err(|error| format!("{}: {}", error.code, error.detail))?;
    Ok(remote_child(&parent, &name))
}

fn local_temp_sibling(path: &std::path::Path, transfer_id: &str, index: usize, suffix: &str) -> Result<std::path::PathBuf, String> {
    let name = format!(".sshterm-{transfer_id}-{index}.{suffix}");
    let parent = path.parent().ok_or_else(|| "sftp_transfer_bad_local_path: Destination has no parent directory.".to_string())?;
    Ok(parent.join(name))
}

async fn commit_remote_file(sftp: &SftpSession, temporary: &str, destination: &str, transfer_id: &str, index: usize, overwrite: bool) -> Result<(), String> {
    let mut backup = None;
    if let Some(existing) = remote_target_exists(sftp, destination).await? {
        if !overwrite { return Err(format!("sftp_overwrite_required: {}", destination)); }
        if !existing.is_regular() || existing.is_symlink() { return Err(format!("sftp_transfer_type_conflict: Refusing to replace non-regular remote item: {destination}")); }
        let backup_path = remote_temp_sibling(destination, transfer_id, index, "bak")?;
        sftp.rename(destination.to_string(), backup_path.clone()).await
            .map_err(|error| format!("sftp_transfer_backup_failed: {error}"))?;
        backup = Some(backup_path);
    }
    if let Err(error) = sftp.rename(temporary.to_string(), destination.to_string()).await {
        if let Some(backup_path) = &backup { let _ = sftp.rename(backup_path.clone(), destination.to_string()).await; }
        return Err(format!("sftp_transfer_commit_failed: {error}"));
    }
    if let Some(backup_path) = backup { sftp.remove_file(backup_path).await.map_err(|error| format!("sftp_transfer_backup_cleanup: {error}"))?; }
    Ok(())
}

async fn commit_local_file(temporary: &std::path::Path, destination: &std::path::Path, transfer_id: &str, index: usize, overwrite: bool) -> Result<(), String> {
    let mut backup = None;
    if let Some(existing) = local_target_metadata(destination).await? {
        if !overwrite { return Err(format!("sftp_overwrite_required: {}", destination.display())); }
        if existing.file_type().is_symlink() || !existing.is_file() { return Err(format!("sftp_transfer_type_conflict: Refusing to replace non-file local item: {}", destination.display())); }
        let backup_path = local_temp_sibling(destination, transfer_id, index, "bak")?;
        tokio::fs::rename(destination, &backup_path).await
            .map_err(|error| format!("sftp_transfer_backup_failed: {error}"))?;
        backup = Some(backup_path);
    }
    if let Err(error) = tokio::fs::rename(temporary, destination).await {
        if let Some(backup_path) = &backup { let _ = tokio::fs::rename(backup_path, destination).await; }
        return Err(format!("sftp_transfer_commit_failed: {error}"));
    }
    if let Some(backup_path) = backup { tokio::fs::remove_file(backup_path).await.map_err(|error| format!("sftp_transfer_backup_cleanup: {error}"))?; }
    Ok(())
}

async fn upload_file(
    sftp: &SftpSession, item: &TransferItem, transfer_id: &str, index: usize,
    overwrite: bool, cancel: &mut watch::Receiver<bool>, progress: &mut SftpTransferProgress, emitter: &TransferEmitter,
) -> Result<(), TransferFailure> {
    let mut source = tokio::fs::File::open(&item.source).await
        .map_err(|error| TransferFailure::failed("sftp_transfer_local_io", error))?;
    let temporary = remote_temp_sibling(&item.destination, transfer_id, index, "part")
        .map_err(TransferFailure::Failed)?;
    let mut target = sftp.create(temporary.clone()).await
        .map_err(|error| TransferFailure::failed("sftp_transfer_remote_io", error))?;
    let mut buffer = vec![0u8; TRANSFER_CHUNK_SIZE];
    let result = loop {
        if *cancel.borrow() { break Err(TransferFailure::Cancelled); }
        let count = tokio::select! {
            result = source.read(&mut buffer) => match result {
                Ok(count) => count,
                Err(error) => break Err(TransferFailure::failed("sftp_transfer_local_io", error)),
            },
            _ = cancel.changed() => break Err(TransferFailure::Cancelled),
        };
        if count == 0 { break Ok(()); }
        tokio::select! {
            result = target.write_all(&buffer[..count]) => if let Err(error) = result {
                break Err(TransferFailure::failed("sftp_transfer_remote_io", error));
            },
            _ = cancel.changed() => break Err(TransferFailure::Cancelled),
        }
        progress.bytes_transferred = progress.bytes_transferred.saturating_add(count as u64);
        emit_transfer(emitter, progress);
    };
    if let Err(error) = result {
        let _ = target.close().await;
        let _ = sftp.remove_file(temporary).await;
        return Err(error);
    }
    if let Err(error) = target.close().await {
        let _ = sftp.remove_file(&temporary).await;
        return Err(TransferFailure::failed("sftp_transfer_remote_close", error));
    }
    if let Err(error) = commit_remote_file(sftp, &temporary, &item.destination, transfer_id, index, overwrite).await {
        let _ = sftp.remove_file(&temporary).await;
        return Err(TransferFailure::Failed(error));
    }
    Ok(())
}

async fn download_file(
    sftp: &SftpSession, item: &TransferItem, transfer_id: &str, index: usize,
    overwrite: bool, cancel: &mut watch::Receiver<bool>, progress: &mut SftpTransferProgress, emitter: &TransferEmitter,
) -> Result<(), TransferFailure> {
    let mut source = sftp.open(item.source.clone()).await.map_err(|error| TransferFailure::failed("sftp_transfer_remote_io", error))?;
    let destination = std::path::PathBuf::from(&item.destination);
    let temporary = local_temp_sibling(&destination, transfer_id, index, "part").map_err(TransferFailure::Failed)?;
    let mut target = tokio::fs::OpenOptions::new().write(true).create_new(true).open(&temporary).await
        .map_err(|error| TransferFailure::failed("sftp_transfer_local_io", error))?;
    let mut buffer = vec![0u8; TRANSFER_CHUNK_SIZE];
    let result = loop {
        if *cancel.borrow() { break Err(TransferFailure::Cancelled); }
        let count = tokio::select! {
            result = source.read(&mut buffer) => match result {
                Ok(count) => count,
                Err(error) => break Err(TransferFailure::failed("sftp_transfer_remote_io", error)),
            },
            _ = cancel.changed() => break Err(TransferFailure::Cancelled),
        };
        if count == 0 { break Ok(()); }
        tokio::select! {
            result = target.write_all(&buffer[..count]) => if let Err(error) = result {
                break Err(TransferFailure::failed("sftp_transfer_local_io", error));
            },
            _ = cancel.changed() => break Err(TransferFailure::Cancelled),
        }
        progress.bytes_transferred = progress.bytes_transferred.saturating_add(count as u64);
        emit_transfer(emitter, progress);
    };
    if let Err(error) = result {
        drop(target);
        let _ = tokio::fs::remove_file(&temporary).await;
        let _ = source.close().await;
        return Err(error);
    }
    if let Err(error) = target.flush().await {
        drop(target);
        let _ = tokio::fs::remove_file(&temporary).await;
        let _ = source.close().await;
        return Err(TransferFailure::failed("sftp_transfer_local_flush", error));
    }
    if let Err(error) = target.sync_all().await {
        drop(target);
        let _ = tokio::fs::remove_file(&temporary).await;
        let _ = source.close().await;
        return Err(TransferFailure::failed("sftp_transfer_local_sync", error));
    }
    drop(target);
    if let Err(error) = source.close().await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(TransferFailure::failed("sftp_transfer_remote_close", error));
    }
    if let Err(error) = commit_local_file(&temporary, &destination, transfer_id, index, overwrite).await {
        let _ = tokio::fs::remove_file(&temporary).await;
        return Err(TransferFailure::Failed(error));
    }
    Ok(())
}

async fn run_transfer(
    sftp: &SftpSession, items: &[TransferItem], direction: &str, transfer_id: &str, overwrite: bool,
    cancel: &mut watch::Receiver<bool>, progress: &mut SftpTransferProgress, emitter: &TransferEmitter,
) -> Result<(), TransferFailure> {
    for (index, item) in items.iter().enumerate() {
        if *cancel.borrow() { return Err(TransferFailure::Cancelled); }
        progress.current_file = if direction == "upload" { item.source.clone() } else { item.source.clone() };
        match (direction, item.kind) {
            ("upload", TransferKind::Directory) => {
                if !sftp.try_exists(item.destination.clone()).await.map_err(|error| TransferFailure::failed("sftp_transfer_remote_stat", error))? {
                    sftp.create_dir(item.destination.clone()).await.map_err(|error| TransferFailure::failed("sftp_transfer_remote_mkdir", error))?;
                }
            }
            ("upload", TransferKind::File) => {
                upload_file(sftp, item, transfer_id, index, overwrite, cancel, progress, emitter).await?;
                progress.files_completed += 1;
            }
            ("download", TransferKind::Directory) => {
                tokio::fs::create_dir_all(&item.destination).await.map_err(|error| TransferFailure::failed("sftp_transfer_local_mkdir", error))?;
            }
            ("download", TransferKind::File) => {
                download_file(sftp, item, transfer_id, index, overwrite, cancel, progress, emitter).await?;
                progress.files_completed += 1;
            }
            _ => return Err(TransferFailure::Failed("sftp_transfer_bad_direction: Unsupported transfer direction.".into())),
        }
        emit_transfer(emitter, progress);
    }
    Ok(())
}

#[tauri::command]
pub async fn sftp_transfer_start(
    state: State<'_, SftpState>, app: AppHandle, session_id: String, direction: String,
    sources: Vec<String>, destination: String, overwrite: bool,
) -> Result<SftpTransferStartResult, String> {
    let _lifecycle = state.gate.read().await;
    let session = state.sessions.lock().await.get(&session_id).cloned()
        .ok_or_else(|| "sftp_session_not_found".to_string())?;
    if !matches!(direction.as_str(), "upload" | "download") { return Err("sftp_transfer_bad_direction: Expected upload or download.".into()); }
    let (items, conflicts) = {
        if direction == "upload" { build_upload_plan(session.session(), &sources, &destination).await? }
        else { build_download_plan(session.session(), session.listing_session(), &sources, &destination).await? }
    };
    if !overwrite && !conflicts.is_empty() {
        return Ok(SftpTransferStartResult { transfer_id: None, conflicts });
    }
    let transfer_id = new_session_id();
    let files_total = items.iter().filter(|item| item.kind == TransferKind::File).count() as u64;
    let bytes_total = items.iter().filter(|item| item.kind == TransferKind::File).fold(0u64, |sum, item| sum.saturating_add(item.size));
    let mut progress = SftpTransferProgress {
        transfer_id: transfer_id.clone(), session_id: session_id.clone(), direction: direction.clone(),
        status: SftpTransferStatus::Running, current_file: String::new(), files_completed: 0,
        files_total, bytes_transferred: 0, bytes_total, error: None,
    };
    let (cancel_tx, mut cancel_rx) = watch::channel(false);
    let (done_tx, done_rx) = watch::channel(false);
    state.transfers.lock().await.insert(transfer_id.clone(), SftpTransferControl {
        session_id: session_id.clone(), cancel: cancel_tx, done: done_rx,
    });
    let event_app = app.clone();
    let emitter = move |update: &SftpTransferProgress| {
        let _ = event_app.emit("sftp:progress", update.clone());
    };
    emit_transfer(&emitter, &progress);
    let transfer_registry = state.transfers.clone();
    let worker_session = session.clone();
    let worker_id = transfer_id.clone();
    tokio::spawn(async move {
        let result = {
            run_transfer(worker_session.session(), &items, &direction, &worker_id, overwrite, &mut cancel_rx, &mut progress, &emitter).await
        };
        match result {
            Ok(()) => progress.status = SftpTransferStatus::Completed,
            Err(TransferFailure::Cancelled) => progress.status = SftpTransferStatus::Cancelled,
            Err(TransferFailure::Failed(error)) => { progress.status = SftpTransferStatus::Failed; progress.error = Some(error); }
        }
        emit_transfer(&emitter, &progress);
        let _ = done_tx.send(true);
        transfer_registry.lock().await.remove(&worker_id);
    });
    Ok(SftpTransferStartResult { transfer_id: Some(transfer_id), conflicts: Vec::new() })
}

#[tauri::command]
pub async fn sftp_transfer_cancel(state: State<'_, SftpState>, transfer_id: String) -> Result<(), String> {
    let transfers = state.transfers.lock().await;
    let transfer = transfers.get(&transfer_id).ok_or_else(|| "sftp_transfer_not_found".to_string())?;
    transfer.cancel.send(true).map_err(|_| "sftp_transfer_not_running".to_string())
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;

    #[test]
    fn windows_agent_pipe_honors_ssh_auth_sock_and_rejects_non_pipe_values() {
        assert_eq!(
            resolve_windows_agent_pipe(Some(r"\\.\pipe\onepassword-ssh-agent")),
            r"\\.\pipe\onepassword-ssh-agent"
        );
        assert_eq!(
            resolve_windows_agent_pipe(Some(r"\\.\pipe\OpenSSH-SSH-Agent")),
            r"\\.\pipe\OpenSSH-SSH-Agent"
        );
        assert_eq!(
            resolve_windows_agent_pipe(Some(r"C:\Users\test\agent.sock")),
            DEFAULT_OPENSSH_AGENT_PIPE
        );
        assert_eq!(resolve_windows_agent_pipe(None), DEFAULT_OPENSSH_AGENT_PIPE);
    }

    #[test]
    fn agent_failure_or_unexpected_eof_enables_legacy_binding_fallback() {
        assert!(is_legacy_agent_session_binding_failure(&keys::Error::IO(
            std::io::Error::from(std::io::ErrorKind::UnexpectedEof),
        )));
        assert!(!is_legacy_agent_session_binding_failure(&keys::Error::IO(
            std::io::Error::from(std::io::ErrorKind::PermissionDenied),
        )));
        assert!(is_legacy_agent_session_binding_failure(&keys::Error::AgentFailure));
    }

    #[test]
    fn identity_listing_eof_after_binding_retries_unbound_on_every_platform() {
        let eof = keys::Error::IO(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        let permission_denied =
            keys::Error::IO(std::io::Error::from(std::io::ErrorKind::PermissionDenied));

        assert!(is_agent_identity_binding_disconnect(true, &eof));
        assert!(!is_agent_identity_binding_disconnect(false, &eof));
        assert_eq!(agent_identity_error_code(true, &eof), "sftp_agent_binding_rejected");
        assert_eq!(agent_identity_error_code(false, &eof), "sftp_agent_unavailable");
        assert_eq!(agent_identity_error_code(true, &permission_denied), "sftp_agent_identities");
        assert_eq!(
            agent_identity_error_code(true, &keys::Error::AgentFailure),
            "sftp_agent_identities"
        );
    }

    #[test]
    fn missing_or_unresponsive_agent_has_actionable_error() {
        let error = agent_not_running_error("pipe not found");
        assert_eq!(error.code, "sftp_agent_unavailable");
        assert!(error.detail.contains("SSH agent is not running or is not responding"));
        if cfg!(windows) {
            assert!(error.detail.contains("Pageant"));
        } else {
            assert!(error.detail.contains("SSH_AUTH_SOCK"));
        }

        let eof = keys::Error::IO(std::io::Error::from(std::io::ErrorKind::UnexpectedEof));
        let unbound_error = agent_identity_error(false, &eof);
        assert_eq!(unbound_error.code, "sftp_agent_unavailable");
        assert!(unbound_error.detail.contains("SSH agent is not running"));
    }

    #[test]
    fn remote_paths_reject_control_characters_and_excessive_length() {
        assert!(validate_remote_path("/srv/data").is_ok());
        assert!(validate_remote_path("folder with spaces").is_ok());
        assert!(matches!(validate_remote_path("/bad\npath"), Err(SftpError { code: "sftp_bad_path", .. })));
        assert!(matches!(validate_remote_path(&"x".repeat(4097)), Err(SftpError { code: "sftp_bad_path", .. })));
    }

    #[test]
    fn remote_names_reject_path_traversal_and_control_characters() {
        assert!(validate_remote_name("report final.txt").is_ok());
        assert!(validate_remote_name("資料").is_ok());
        for invalid in ["", ".", "..", "../escape", "a/b", "a\\b", "bad\nname"] {
            assert!(matches!(validate_remote_name(invalid), Err(SftpError { code: "sftp_bad_name", .. })));
        }
    }

    #[test]
    fn downloaded_names_are_safe_windows_path_components() {
        for valid in ["report final.txt", "資料", "archive.tar.gz"] {
            assert!(validate_local_component(valid).is_ok(), "{valid}");
        }
        for invalid in ["", ".", "..", "CON", "nul.txt", "COM1.log", "LPT9", "C:drive", "stream:name", "trailing.", "trailing ", "a\\b", "a/b", "bad?name"] {
            assert!(validate_local_component(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn duplicate_transfer_destinations_are_rejected_before_copying() {
        let item = TransferItem {
            source: "one".into(), destination: "/target/report.txt".into(), kind: TransferKind::File, size: 1,
        };
        let mut items = Vec::new();
        let mut destinations = HashSet::new();
        push_transfer_item(&mut items, &mut destinations, item.clone()).unwrap();
        let result = push_transfer_item(&mut items, &mut destinations, item);
        assert!(result.unwrap_err().contains("sftp_transfer_duplicate_destination"));
        assert_eq!(items.len(), 1);
    }


    fn integration_connection(id: &str, user: &str, port: u16, key_path: &str) -> Connection {
        let mut connection = Connection::default();
        connection.id = id.to_string();
        connection.name = id.to_string();
        connection.host = "127.0.0.1".to_string();
        connection.port = port;
        connection.user = user.to_string();
        connection.auth = Auth::Key { path: key_path.to_string(), passphrase: None };
        connection.keepalive_secs = 0;
        connection.sftp = Some(crate::vault::SftpOptions {
            port: None,
            start_dir: format!("/home/{user}"),
        });
        connection
    }

    fn transfer_progress(session_id: &str, transfer_id: &str, direction: &str, items: &[TransferItem]) -> SftpTransferProgress {
        let files = items.iter().filter(|item| item.kind == TransferKind::File).count() as u64;
        let bytes = items.iter().filter(|item| item.kind == TransferKind::File)
            .fold(0u64, |sum, item| sum.saturating_add(item.size));
        SftpTransferProgress {
            transfer_id: transfer_id.to_string(),
            session_id: session_id.to_string(),
            direction: direction.to_string(),
            status: SftpTransferStatus::Running,
            current_file: String::new(),
            files_completed: 0,
            files_total: files,
            bytes_transferred: 0,
            bytes_total: bytes,
            error: None,
        }
    }

    async fn read_remote_file(sftp: &SftpSession, path: &str) -> Vec<u8> {
        let mut file = sftp.open(path.to_string()).await.expect("open remote test file");
        let mut contents = Vec::new();
        file.read_to_end(&mut contents).await.expect("read remote test file");
        file.close().await.expect("close remote test file");
        contents
    }

    // The Linux integration workflow starts two isolated sshd instances:
    // one target and one jump host with PermitOpen restricted to the target.
    #[tokio::test]
    #[ignore = "requires the local OpenSSH integration workflow"]
    async fn sftp_local_openssh_direct_jump_and_transfer_smoke() {
        let user = std::env::var("SSHTERM_SFTP_TEST_USER").expect("SSHTERM_SFTP_TEST_USER");
        let key = std::env::var("SSHTERM_SFTP_TEST_KEY").expect("SSHTERM_SFTP_TEST_KEY");
        let target_port: u16 = std::env::var("SSHTERM_SFTP_TEST_PORT").expect("SSHTERM_SFTP_TEST_PORT")
            .parse().expect("valid target port");
        let hop_port: u16 = std::env::var("SSHTERM_SFTP_TEST_HOP_PORT").expect("SSHTERM_SFTP_TEST_HOP_PORT")
            .parse().expect("valid jump port");

        let workspace = tempfile::tempdir().expect("create integration workspace");
        let known_hosts = workspace.path().join("known_hosts");
        let client = integration_connection("direct-target", &user, target_port, &key);
        let direct = connect(&client, &[], &known_hosts).await.expect("connect direct SFTP");

        let remote_base = format!("/home/{user}/sshterm-m8-{}", new_session_id());
        direct.session().create_dir(remote_base.clone()).await.expect("create remote test directory");

        let local_source = workspace.path().join("upload-tree");
        tokio::fs::create_dir_all(local_source.join("nested/deeper")).await.expect("create local tree");
        tokio::fs::write(local_source.join("empty.bin"), b"").await.expect("create empty file");
        tokio::fs::write(local_source.join("hello world.txt"), b"first upload").await.expect("create text file");
        tokio::fs::write(local_source.join("nested/deeper/資料 final.txt"), b"unicode path").await.expect("create unicode file");
        let large_payload = (0..8 * 1024 * 1024).map(|index| (index % 251) as u8).collect::<Vec<_>>();
        tokio::fs::write(local_source.join("nested/large.bin"), &large_payload).await.expect("create large file");

        let (upload_items, upload_conflicts) = build_upload_plan(
            direct.session(),
            &[local_source.to_string_lossy().into_owned()],
            &remote_base,
        ).await.expect("build recursive upload plan");
        assert!(upload_conflicts.is_empty(), "new remote tree must not have conflicts");
        let upload_id = new_session_id();
        let mut upload_progress = transfer_progress(&client.id, &upload_id, "upload", &upload_items);
        let (_upload_cancel, mut upload_cancel_rx) = watch::channel(false);
        let no_emit = |_progress: &SftpTransferProgress| {};
        let (upload_result, upload_browse_result) = tokio::join!(
            run_transfer(
                direct.session(), &upload_items, "upload", &upload_id, false,
                &mut upload_cancel_rx, &mut upload_progress, &no_emit,
            ),
            async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                direct.session().read_dir(remote_base.clone()).await
            },
        );
        upload_result.expect("upload files and folders");
        upload_browse_result.expect("browse remote directory while upload is running");
        assert_eq!(upload_progress.files_completed, 4);

        let remote_tree = remote_child(&remote_base, "upload-tree");
        assert_eq!(read_remote_file(direct.session(), &remote_child(&remote_tree, "empty.bin")).await, Vec::<u8>::new());
        assert_eq!(read_remote_file(direct.session(), &remote_child(&remote_tree, "hello world.txt")).await, b"first upload");
        assert_eq!(read_remote_file(direct.session(), &remote_child(&remote_tree, "nested/deeper/資料 final.txt")).await, b"unicode path");
        assert_eq!(read_remote_file(direct.session(), &remote_child(&remote_tree, "nested/large.bin")).await, large_payload);

        // Exercise pipelined READDIR across several pages and verify that
        // concurrent requests neither lose nor duplicate directory entries.
        let paging_dir = remote_child(&remote_base, "listing-pages");
        direct.session().create_dir(paging_dir.clone()).await.expect("create large listing directory");
        for index in 0..300 {
            let path = remote_child(&paging_dir, &format!("entry-{index:03}.txt"));
            let file = direct.session().create(path).await.expect("create listing test file");
            file.close().await.expect("close listing test file");
        }
        let listing_session = direct.listing_session().expect("dedicated listing SFTP channel");
        let listed = read_directory_pipelined(listing_session, paging_dir)
            .await.expect("read all pages through pipelined READDIR");
        assert_eq!(listed.len(), 300, "all directory entries must be returned");
        let distinct_names = listed.into_iter().map(|(name, _)| name).collect::<HashSet<_>>();
        assert_eq!(distinct_names.len(), 300, "pipelined READDIR must not duplicate entries");

        let download_root = workspace.path().join("download");
        tokio::fs::create_dir_all(&download_root).await.expect("create local download folder");
        let (download_items, download_conflicts) = build_download_plan(
            direct.session(), direct.listing_session(), &[remote_tree.clone()], &download_root.to_string_lossy(),
        ).await.expect("build recursive download plan");
        assert!(download_conflicts.is_empty());
        let download_id = new_session_id();
        let mut download_progress = transfer_progress(&client.id, &download_id, "download", &download_items);
        let (_download_cancel, mut download_cancel_rx) = watch::channel(false);
        let (download_result, browse_result) = tokio::join!(
            run_transfer(
                direct.session(), &download_items, "download", &download_id, false,
                &mut download_cancel_rx, &mut download_progress, &no_emit,
            ),
            async {
                tokio::time::sleep(Duration::from_millis(1)).await;
                direct.session().read_dir(remote_tree.clone()).await
            },
        );
        download_result.expect("download files and folders");
        browse_result.expect("browse remote directory while download is running");
        assert_eq!(tokio::fs::read(download_root.join("upload-tree/nested/large.bin")).await.unwrap(), large_payload);
        assert_eq!(tokio::fs::read(download_root.join("upload-tree/nested/deeper/資料 final.txt")).await.unwrap(), b"unicode path");

        // Existing destinations are surfaced before a job starts; confirmed overwrite
        // keeps the old file until the replacement has streamed to its temp sibling.
        let replacement_path = local_source.join("hello world.txt");
        tokio::fs::write(&replacement_path, b"replacement bytes").await.expect("replace local test source");
        let (replacement_items, replacement_conflicts) = build_upload_plan(
            direct.session(), &[replacement_path.to_string_lossy().into_owned()], &remote_tree,
        ).await.expect("build overwrite plan");
        assert_eq!(replacement_conflicts, vec![remote_child(&remote_tree, "hello world.txt")]);
        let replacement_id = new_session_id();
        let mut replacement_progress = transfer_progress(&client.id, &replacement_id, "upload", &replacement_items);
        let (_replacement_cancel, mut replacement_cancel_rx) = watch::channel(false);
        run_transfer(
            direct.session(), &replacement_items, "upload", &replacement_id, true,
            &mut replacement_cancel_rx, &mut replacement_progress, &no_emit,
        ).await.expect("commit confirmed overwrite");
        assert_eq!(read_remote_file(direct.session(), &remote_child(&remote_tree, "hello world.txt")).await, b"replacement bytes");

        // Batch deletion removes all requested files and empty folders, but
        // reports a non-empty directory separately and never recurses into it.
        let batch_a = workspace.path().join("batch-delete-a.txt");
        let batch_b = workspace.path().join("batch-delete-b.txt");
        tokio::fs::write(&batch_a, b"batch a").await.unwrap();
        tokio::fs::write(&batch_b, b"batch b").await.unwrap();
        let batch_paths = vec![batch_a.to_string_lossy().into_owned(), batch_b.to_string_lossy().into_owned()];
        let (batch_items, batch_conflicts) = build_upload_plan(direct.session(), &batch_paths, &remote_base)
            .await.expect("build batch-delete source plan");
        assert!(batch_conflicts.is_empty());
        let batch_id = new_session_id();
        let mut batch_progress = transfer_progress(&client.id, &batch_id, "upload", &batch_items);
        let (_batch_cancel, mut batch_cancel_rx) = watch::channel(false);
        run_transfer(
            direct.session(), &batch_items, "upload", &batch_id, false,
            &mut batch_cancel_rx, &mut batch_progress, &no_emit,
        ).await.expect("upload batch-delete files");
        let empty_delete_dir = remote_child(&remote_base, "empty-delete-dir");
        direct.session().create_dir(empty_delete_dir.clone()).await.expect("create empty folder for batch delete");
        let batch_delete = delete_remote_entries(direct.session(), vec![
            remote_child(&remote_base, "batch-delete-a.txt"),
            remote_child(&remote_base, "batch-delete-b.txt"),
            empty_delete_dir.clone(),
            remote_tree.clone(),
            remote_child(&remote_base, "batch-delete-a.txt"),
        ]).await;
        assert_eq!(batch_delete.deleted.len(), 3, "duplicate paths should be ignored");
        assert_eq!(batch_delete.failed.len(), 1);
        assert_eq!(batch_delete.failed[0].path, remote_tree);
        assert!(remote_target_exists(direct.session(), &empty_delete_dir).await.unwrap().is_none());
        assert!(remote_target_exists(direct.session(), &remote_base).await.unwrap().is_some());

        // Cancel after the first chunk and prove neither the final file nor its
        // temporary sibling is left behind.
        let cancel_source = workspace.path().join("cancel.bin");
        tokio::fs::write(&cancel_source, vec![0x5a; TRANSFER_CHUNK_SIZE * 8]).await.expect("create cancellable file");
        let (cancel_items, cancel_conflicts) = build_upload_plan(
            direct.session(), &[cancel_source.to_string_lossy().into_owned()], &remote_base,
        ).await.expect("build cancellation plan");
        assert!(cancel_conflicts.is_empty());
        let cancel_id = new_session_id();
        let mut cancel_progress = transfer_progress(&client.id, &cancel_id, "upload", &cancel_items);
        let (cancel_tx, mut cancel_rx) = watch::channel(false);
        let cancel_on_first_chunk = move |update: &SftpTransferProgress| {
            if update.bytes_transferred > 0 { let _ = cancel_tx.send(true); }
        };
        let cancelled = run_transfer(
            direct.session(), &cancel_items, "upload", &cancel_id, false,
            &mut cancel_rx, &mut cancel_progress, &cancel_on_first_chunk,
        ).await;
        assert!(matches!(cancelled, Err(TransferFailure::Cancelled)));
        assert!(remote_target_exists(direct.session(), &remote_child(&remote_base, "cancel.bin")).await.unwrap().is_none());
        let remote_entries = direct.session().read_dir(remote_base.clone()).await.expect("list remote work folder");
        let mut temporary_found = false;
        for entry in remote_entries {
            if entry.file_name().to_string().contains(&format!(".sshterm-{cancel_id}-")) {
                temporary_found = true;
            }
        }
        assert!(!temporary_found);

        // The same target and transfer logic must work through a direct-tcpip jump.
        let hop = integration_connection("jump-host", &user, hop_port, &key);
        let through_jump = connect(&client, &[hop], &known_hosts).await.expect("connect through SFTP jump host");
        let jump_source = workspace.path().join("through-jump.txt");
        tokio::fs::write(&jump_source, b"multi-hop transfer").await.unwrap();
        let (jump_items, jump_conflicts) = build_upload_plan(
            through_jump.session(), &[jump_source.to_string_lossy().into_owned()], &remote_base,
        ).await.expect("build multi-hop upload plan");
        assert!(jump_conflicts.is_empty());
        let jump_id = new_session_id();
        let mut jump_progress = transfer_progress(&client.id, &jump_id, "upload", &jump_items);
        let (_jump_cancel, mut jump_cancel_rx) = watch::channel(false);
        run_transfer(
            through_jump.session(), &jump_items, "upload", &jump_id, false,
            &mut jump_cancel_rx, &mut jump_progress, &no_emit,
        ).await.expect("upload through jump host");
        assert_eq!(read_remote_file(through_jump.session(), &remote_child(&remote_base, "through-jump.txt")).await, b"multi-hop transfer");

        through_jump.close().await.expect("close jump SFTP");
        direct.close().await.expect("close direct SFTP");
    }

    #[test]
    fn normalized_absolute_remote_paths_skip_realpath() {
        for path in ["/", "/home/user", "/srv/data/nested dir", "/tmp/file.txt"] {
            assert!(is_normalized_absolute_remote_path(path), "{path}");
        }
        for path in ["", ".", "..", "relative/path", "/home/../etc", "/home/./user"] {
            assert!(!is_normalized_absolute_remote_path(path), "{path}");
        }
    }

    #[test]
    fn remote_parent_handles_root_and_nested_paths() {
        assert_eq!(remote_parent("/"), None);
        assert_eq!(remote_parent("/home"), Some("/".to_string()));
        assert_eq!(remote_parent("/home/user/"), Some("/home".to_string()));
        assert_eq!(remote_parent("relative/path"), Some("relative".to_string()));
    }

    #[test]
    fn remote_child_uses_posix_separators() {
        assert_eq!(remote_child("/", "file.txt"), "/file.txt");
        assert_eq!(remote_child("/srv/data/", "file.txt"), "/srv/data/file.txt");
    }
}
