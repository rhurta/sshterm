//! Native SFTP protocol layer (M8a).
//!
//! This module deliberately has no UI or Tauri command dependency. It proves
//! the SSH/SFTP transport independently before M8b adds the application-facing
//! lifecycle and browser commands.

use std::collections::HashMap;
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
use russh_sftp::client::SftpSession;
use serde::Serialize;
use tauri::State;
use sha1::Sha1;
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::{Mutex, RwLock};

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
        agent.bind_session(&binding, false).await.map_err(|error| {
            let legacy_fallback = is_legacy_agent_session_binding_failure(&error);
            SftpError::new(
                if legacy_fallback { "sftp_agent_binding_rejected" } else { "sftp_agent_binding" },
                error.to_string(),
            )
        })?;
    }

    let identities = agent
        .request_identities()
        .await
        .map_err(|error| SftpError::new("sftp_agent_identities", error.to_string()))?;
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
        Err(_) => {
            let pageant = WindowsAgentSource::Pageant;
            (connect_windows_agent(&pageant).await?, pageant)
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
                SftpError::new(
                    "sftp_agent_unavailable",
                    format!("Agent closed during session binding ({error}); reconnect failed: {}", reconnect_error.detail),
                )
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
        .map_err(|error| SftpError::new("sftp_agent_unavailable", error.to_string()))?;
    authenticate_with_agent(handle, user, agent, true).await?;
    Ok(AgentBindingStatus::Bound)
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
    sessions: Vec<Handle<SftpHostKeyHandler>>,
    start_dir: String,
    agent_binding: AgentBindingStatus,
}

impl SftpClient {
    pub fn session(&self) -> &SftpSession {
        &self.sftp
    }

    pub async fn current_directory(&self) -> Result<String, SftpError> {
        let path = if self.start_dir.is_empty() { "." } else { &self.start_dir };
        self.sftp
            .canonicalize(path)
            .await
            .map_err(|error| SftpError::new("sftp_protocol", error.to_string()))
    }

    pub async fn close(&self) -> Result<(), SftpError> {
        let mut first_error = self
            .sftp
            .close()
            .await
            .err()
            .map(|error| SftpError::new("sftp_close", error.to_string()));
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
    let sftp = SftpSession::new(channel.into_stream())
        .await
        .map_err(|error| SftpError::new("sftp_subsystem", error.to_string()))?;
    sessions.push(target);

    let start_dir = connection
        .sftp
        .as_ref()
        .map(|options| options.start_dir.trim().to_string())
        .filter(|path| !path.is_empty())
        .unwrap_or_default();

    Ok(SftpClient { sftp, sessions, start_dir, agent_binding })
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
    sessions: Mutex<HashMap<String, Arc<Mutex<SftpClient>>>>,
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
        // Wait for active operations and prevent a new open/list from slipping
        // in while the vault is being locked.
        let _lifecycle = self.gate.write().await;
        let sessions = std::mem::take(&mut *self.sessions.lock().await);
        let mut first_error = None;
        for (_, session) in sessions {
            if let Err(error) = session.lock().await.close().await {
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
    state.sessions.lock().await.insert(session_id.clone(), Arc::new(Mutex::new(client)));

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

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, SftpState>,
    session_id: String,
    remote_path: Option<String>,
) -> Result<SftpDirectory, String> {
    let _lifecycle = state.gate.read().await;
    let session = state.sessions.lock().await.get(&session_id).cloned()
        .ok_or_else(|| "sftp_session_not_found".to_string())?;
    let client = session.lock().await;
    let requested = remote_path.unwrap_or_else(|| ".".to_string());
    validate_remote_path(&requested)
        .map_err(|error| format!("{}: {}", error.code, error.detail))?;
    let current_path = client.session().canonicalize(requested).await
        .map_err(|error| format!("sftp_protocol: {error}"))?;
    let directory = client.session().read_dir(current_path.clone()).await
        .map_err(|error| format!("sftp_protocol: {error}"))?;

    let mut entries = Vec::new();
    for entry in directory {
        let name = entry.file_name().to_string();
        if name == "." || name == ".." {
            continue;
        }
        let metadata = entry.metadata();
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
    let client = session.lock().await;
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
    let client = session.lock().await;
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

#[tauri::command]
pub async fn sftp_delete(
    state: State<'_, SftpState>,
    session_id: String,
    remote_path: String,
) -> Result<(), String> {
    let _lifecycle = state.gate.read().await;
    let session = state.sessions.lock().await.get(&session_id).cloned()
        .ok_or_else(|| "sftp_session_not_found".to_string())?;
    let client = session.lock().await;
    let (parent, name) = canonical_sibling(client.session(), &remote_path).await
        .map_err(|error| format!("{}: {}", error.code, error.detail))?;
    let target = remote_child(&parent, &name);
    let metadata = client.session().symlink_metadata(target.clone()).await
        .map_err(|error| format!("sftp_protocol: {error}"))?;

    // Never follow symlinks and never recurse. Directory removal only succeeds
    // for an empty directory on the server.
    if metadata.is_symlink() || metadata.is_regular() {
        client.session().remove_file(target).await
            .map_err(|error| format!("sftp_protocol: {error}"))
    } else if metadata.is_dir() {
        client.session().remove_dir(target).await
            .map_err(|error| format!("sftp_protocol: {error}"))
    } else {
        Err("sftp_delete_unsupported: This remote entry type cannot be deleted safely.".to_string())
    }
}

#[tauri::command]
pub async fn sftp_close(
    state: State<'_, SftpState>,
    session_id: String,
) -> Result<(), String> {
    let _lifecycle = state.gate.write().await;
    let session = state.sessions.lock().await.remove(&session_id)
        .ok_or_else(|| "sftp_session_not_found".to_string())?;
    session.lock().await.close().await
        .map_err(|error| format!("{}: {}", error.code, error.detail))
}

#[tauri::command]
pub async fn sftp_close_all(state: State<'_, SftpState>) -> Result<(), String> {
    state.close_all_inner().await
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
