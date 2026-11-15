//! Transport layer (M2).
//!
//! The session code only knows this trait. In M2 there is one implementation
//! (`openssh` over ConPTY); in M10 `russh` will be added without touching the
//! interface.

pub mod openssh;

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;
use tokio::sync::mpsc::UnboundedReceiver;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct PtySize {
    pub cols: u16,
    pub rows: u16,
}

pub type OutputReceiver = UnboundedReceiver<Vec<u8>>;

#[derive(Debug)]
pub struct TransportError {
    /// Short, stable code; the frontend translates it.
    pub code: &'static str,
    pub detail: String,
}

impl TransportError {
    pub fn new(code: &'static str, detail: impl Into<String>) -> Self {
        Self { code, detail: detail.into() }
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

/// Everything an engine needs to open the connection. Contains no secrets:
/// the password travels via the askpass pipe, not through here.
#[derive(Debug, Clone)]
pub struct SshConfig {
    pub program: PathBuf,
    pub askpass_program: PathBuf,
    pub known_hosts: PathBuf,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub keepalive_secs: u32,
    /// Optional private key file (M4, `auth.type = key`). Only the path is
    /// stored in the vault; the file itself is read in place by ssh.exe.
    pub key_path: Option<PathBuf>,
}

#[async_trait]
pub trait Transport: Send {
    async fn open(&mut self, cfg: &SshConfig, size: PtySize) -> Result<(), TransportError>;
    async fn write(&mut self, data: &[u8]) -> Result<(), TransportError>;
    async fn resize(&mut self, size: PtySize) -> Result<(), TransportError>;
    async fn close(&mut self) -> Result<(), TransportError>;
    /// The receiver cannot be cloned, so it is handed out once via `&mut self`
    /// (the plan showed `&self`; adjusted to the actual channel used).
    fn output(&mut self) -> Option<OutputReceiver>;
}
