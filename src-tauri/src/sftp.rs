//! Native SFTP protocol layer (M8a).
//!
//! This module deliberately has no UI or Tauri command dependency. It proves
//! the SSH/SFTP transport independently before M8b adds the application-facing
//! lifecycle and browser commands.

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use russh::client::{self, Handle};
use russh::keys::agent::{client::{AgentClient, AgentStream}, AgentIdentity};
use russh::keys::{self, HashAlg, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate};
use russh_sftp::client::SftpSession;
use tokio::io::{AsyncRead, AsyncWrite};

use crate::store;
use crate::vault::{Auth, Connection, SftpOptions};

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

fn verify_or_learn_host_key(
    host: &str,
    port: u16,
    public_key: &PublicKey,
    known_hosts: &Path,
) -> Result<bool, SftpError> {
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

async fn authenticate_with_agent<S>(
    handle: &mut Handle<SftpHostKeyHandler>,
    user: &str,
    mut agent: AgentClient<S>,
) -> Result<(), SftpError>
where
    S: AgentStream + Send + Unpin + 'static,
{
    let binding = handle.agent_session_binding().cloned().ok_or_else(|| {
        SftpError::new("sftp_agent_binding", "SSH agent session binding is unavailable.")
    })?;
    agent
        .bind_session(&binding, false)
        .await
        .map_err(|error| SftpError::new("sftp_agent_binding", error.to_string()))?;

    let identities = agent
        .request_identities()
        .await
        .map_err(|error| SftpError::new("sftp_agent", error.to_string()))?;
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
            .map_err(|error| SftpError::new("sftp_agent", error.to_string()))?;
        if result.success() {
            return Ok(());
        }
    }

    Err(SftpError::new(
        "sftp_auth_failed",
        "No available SSH-agent identity authenticated successfully.",
    ))
}

#[cfg(windows)]
async fn authenticate_agent(
    handle: &mut Handle<SftpHostKeyHandler>,
    user: &str,
) -> Result<(), SftpError> {
    // Prefer the Windows OpenSSH agent. Pageant is a fallback for users who
    // keep their keys there. Bound both connection attempts so a missing or
    // busy agent cannot stall SFTP startup indefinitely.
    let agent = match tokio::time::timeout(
        Duration::from_secs(2),
        AgentClient::connect_named_pipe(r"\\.\pipe\openssh-ssh-agent"),
    )
    .await
    {
        Ok(Ok(agent)) => agent,
        _ => tokio::time::timeout(Duration::from_secs(2), AgentClient::connect_pageant())
            .await
            .map_err(|_| SftpError::new("sftp_agent_timeout", "Pageant connection timed out."))?
            .map_err(|error| SftpError::new("sftp_agent_unavailable", error.to_string()))?,
    };
    authenticate_with_agent(handle, user, agent).await
}

#[cfg(unix)]
async fn authenticate_agent(
    handle: &mut Handle<SftpHostKeyHandler>,
    user: &str,
) -> Result<(), SftpError> {
    let agent = AgentClient::connect_env()
        .await
        .map_err(|error| SftpError::new("sftp_agent_unavailable", error.to_string()))?;
    authenticate_with_agent(handle, user, agent).await
}

#[cfg(not(any(windows, unix)))]
async fn authenticate_agent(
    _handle: &mut Handle<SftpHostKeyHandler>,
    _user: &str,
) -> Result<(), SftpError> {
    Err(SftpError::new(
        "sftp_agent_unsupported",
        "SSH-agent authentication is not supported on this platform.",
    ))
}

async fn authenticate(
    handle: &mut Handle<SftpHostKeyHandler>,
    connection: &Connection,
) -> Result<(), SftpError> {
    match &connection.auth {
        Auth::Password { password } if !password.is_empty() => {
            if handle
                .authenticate_password(&connection.user, password.clone())
                .await?
                .success()
            {
                Ok(())
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
                Ok(())
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
        authenticate(&mut handle, hop).await?;
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

    authenticate(&mut target, connection).await?;
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

    Ok(SftpClient { sftp, sessions, start_dir })
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
        assert!(saved.starts_with("[sftp.example]:2207 ssh-ed25519 "));
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
        let target = Connection {
            host: "target.example".into(),
            user: "admin".into(),
            port: 22,
            sftp: Some(SftpOptions { port: Some(2222), start_dir: String::new() }),
            ..Connection::default()
        };
        let hop = Connection {
            host: "jump.example".into(),
            user: "bastion".into(),
            port: 2201,
            sftp: Some(crate::vault::SftpOptions { port: Some(2202), start_dir: String::new() }),
            ..Connection::default()
        };
        assert_eq!(endpoint(&target, true).unwrap(), ("target.example".into(), 2222));
        assert_eq!(endpoint(&hop, false).unwrap(), ("jump.example".into(), 2201));
    }
}
