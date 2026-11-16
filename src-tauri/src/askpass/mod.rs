//! Password delivery to `ssh.exe` (M2, phase 1).
//!
//! The main process creates a named pipe per session. `ssh.exe` receives
//! `SSH_ASKPASS` pointing to this same executable and `SSH_ASKPASS_REQUIRE=force`;
//! when it needs a password it launches the exe as a helper. The helper detects
//! the mode via environment variables, connects to the pipe, presents a
//! single-use per-session token, receives the password and writes it to stdout.
//!
//! The password never appears on a command line or in a file.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

#[cfg(windows)]
mod acl;

pub const PIPE_ENV: &str = "SSHTERM_ASKPASS_PIPE";
pub const TOKEN_ENV: &str = "SSHTERM_ASKPASS_TOKEN";

#[derive(Serialize, Deserialize)]
struct Request {
    token: String,
    prompt: String,
}

/// One credential that OpenSSH may request while opening a session. A password
/// or key passphrase is kept out of command-line arguments and only delivered
/// when the prompt identifies this exact host/user or key path.
pub struct AskpassCredential {
    pub user: String,
    pub host: String,
    pub key_path: Option<String>,
    pub password: String,
}

struct StoredCredential {
    user: String,
    host: String,
    key_path: Option<String>,
    password: Zeroizing<String>,
}

#[derive(Serialize, Deserialize)]
struct Response {
    password: String,
}

fn random_hex(bytes: usize) -> String {
    let mut raw = vec![0u8; bytes];
    OsRng.fill_bytes(&mut raw);
    let mut out = String::with_capacity(bytes * 2);
    for b in raw {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Owner of a session's pipe. On drop (closing the session, locking, or exiting
/// the application) the server stops and the password is cleared from memory.
pub struct AskpassServer {
    pipe_name: String,
    token: String,
    stop: Arc<AtomicBool>,
}

impl AskpassServer {
    pub fn start(credentials: Vec<AskpassCredential>) -> Result<Self, &'static str> {
        if credentials.is_empty() {
            return Err("askpass_no_credentials");
        }
        let pipe_name = format!(
            r"\\.\pipe\sshterm-askpass-{}-{}",
            std::process::id(),
            random_hex(8)
        );
        let token = random_hex(32);
        let stop = Arc::new(AtomicBool::new(false));
        let credentials = credentials
            .into_iter()
            .map(|c| StoredCredential {
                user: c.user,
                host: c.host,
                key_path: c.key_path,
                password: Zeroizing::new(c.password),
            })
            .collect();
        start_pipe_server(pipe_name.clone(), token.clone(), credentials, stop.clone())?;
        Ok(Self { pipe_name, token, stop })
    }

    pub fn pipe_name(&self) -> &str {
        &self.pipe_name
    }

    pub fn token(&self) -> &str {
        &self.token
    }
}

impl Drop for AskpassServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Pipe exchange without stdout side effects: can be tested standalone.
#[cfg(windows)]
fn exchange(pipe: &str, token: &str, prompt: &str) -> Result<String, i32> {
    use std::io::{BufRead, BufReader, Write};

    let mut file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(pipe)
        .map_err(|_| 3)?;
    let request = serde_json::to_string(&Request {
        token: token.to_string(),
        prompt: prompt.to_string(),
    })
    .map_err(|_| 4)?;
    writeln!(file, "{request}").map_err(|_| 4)?;
    file.flush().map_err(|_| 4)?;

    let mut line = String::new();
    BufReader::new(&file).read_line(&mut line).map_err(|_| 5)?;
    let response: Response = serde_json::from_str(line.trim_end()).map_err(|_| 6)?;
    Ok(response.password)
}

/// Host-key trust questions are yes/no, not secret requests. With
/// SSH_ASKPASS_REQUIRE=force, ssh sends *any* prompt to this helper, so without
/// this guard the vault password would be handed to ssh as the answer to
/// "are you sure you want to continue connecting?" (ssh reads that as "no" and
/// aborts with "Host key verification failed.") and the one-time token would
/// be burned. The transport sets StrictHostKeyChecking=accept-new so these
/// prompts should never arrive; this is a safety net.
fn is_host_key_prompt(prompt: &str) -> bool {
    let p = prompt.to_ascii_lowercase();
    p.contains("authenticity of host")
        || p.contains("continue connecting")
        || p.contains("please type 'yes'")
        || p.contains("remote host identification has changed")
        || (p.contains("yes/no") && p.contains("host"))
}

fn normalize_host(host: &str) -> String {
    host.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

/// Select the one credential requested by OpenSSH. Password prompts identify
/// the hop as "user@host's password"; key passphrase prompts identify a path.
/// A generic "Password:" prompt is only safe to answer when there is exactly
/// one credential in the whole session.
fn credential_index(prompt: &str, credentials: &[StoredCredential]) -> Option<usize> {
    let trimmed = prompt.trim().trim_end_matches(':').trim();
    let lower = trimmed.to_ascii_lowercase();

    if let Some(start) = lower.find("passphrase for key '") {
        let path_start = start + "passphrase for key '".len();
        let path_end = trimmed.get(path_start..)?.find('\'')? + path_start;
        let requested = trimmed.get(path_start..path_end)?;
        return credentials.iter().position(|c| {
            c.key_path.as_deref().is_some_and(|path| {
                path.replace('/', "\\").eq_ignore_ascii_case(&requested.replace('/', "\\"))
            })
        });
    }

    for suffix in ["'s password", " password"] {
        if lower.ends_with(suffix) {
            let identity = trimmed.get(..trimmed.len().saturating_sub(suffix.len()))?;
            let (user, host) = identity.rsplit_once('@')?;
            let user = user.trim().to_ascii_lowercase();
            let host = normalize_host(host);
            return credentials.iter().position(|c| {
                c.user.eq_ignore_ascii_case(&user) && normalize_host(&c.host) == host
            });
        }
    }

    // A generic prompt cannot be tied to a particular hop, even when only
    // one saved credential exists: another agent-authenticated hop may be
    // prompting. Fail closed rather than send a password to the wrong host.
    None
}

/// Helper entry point. Returns an exit code; writes nothing to stdout except
/// the password (ssh reads stdout and strips the trailing newline).
#[cfg(windows)]
pub fn run_helper() -> i32 {
    use std::io::Write as _;

    let pipe = match std::env::var(PIPE_ENV) {
        Ok(v) => v,
        Err(_) => return 2,
    };
    let token = std::env::var(TOKEN_ENV).unwrap_or_default();
    let prompt = std::env::args().nth(1).unwrap_or_default();

    // Never answer a trust question with a credential, and never consume the
    // single-use token for one. A non-zero exit makes ssh treat it as a
    // failed askpass rather than a "no" vote.
    if is_host_key_prompt(&prompt) {
        return 8;
    }

    match exchange(&pipe, &token, &prompt) {
        // An error here (stdout closed) is detected by ssh as an askpass failure.
        Ok(password) => {
            if writeln!(std::io::stdout(), "{password}").is_err() {
                7
            } else {
                0
            }
        }
        Err(code) => code,
    }
}

#[cfg(not(windows))]
pub fn run_helper() -> i32 {
    1
}

#[cfg(windows)]
fn start_pipe_server(
    name: String,
    token: String,
    credentials: Vec<StoredCredential>,
    stop: Arc<AtomicBool>,
) -> Result<(), &'static str> {
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    // The pipe is created with a current-user-only DACL at creation time. If
    // that fails the session never starts: there must be no pipe that could
    // deliver the password with broader permissions than intended.
    let mut server = acl::create_secure_pipe(&name, 4096, 4096, true, true)
        .map_err(|_| "askpass_acl")?;

    // The session token authorizes requests to this private pipe. Each
    // matching credential is delivered at most once, so a copied helper
    // request cannot replay an already-used hop password.
    let mut used = HashSet::<usize>::new();

    tauri::async_runtime::spawn(async move {
        loop {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            // connect() would block forever; the timeout lets us react to the
            // stop signal without leaving a thread stuck.
            match tokio::time::timeout(Duration::from_millis(200), server.connect()).await {
                Ok(Ok(())) => {}
                Ok(Err(_)) => break,
                Err(_) => continue,
            }

            let mut line = String::new();
            {
                let mut reader = BufReader::new(&mut server);
                let _ = reader.read_line(&mut line).await;
            }
            let reply = match serde_json::from_str::<Request>(line.trim_end()) {
                Ok(req) if req.token == token => {
                    match credential_index(&req.prompt, &credentials) {
                        Some(index) if used.insert(index) => serde_json::to_string(&Response {
                            password: credentials[index].password.as_str().to_string(),
                        })
                        .unwrap_or_default(),
                        _ => String::new(), // unknown prompt or already-used credential
                    }
                }
                _ => String::new(), // token absent or wrong
            };
            let _ = server.write_all(format!("{reply}\n").as_bytes()).await;
            let _ = server.flush().await;
            let _ = server.disconnect();
        }
    });
    Ok(())
}

#[cfg(not(windows))]
fn start_pipe_server(
    _name: String,
    _token: String,
    _credentials: Vec<StoredCredential>,
    _stop: Arc<AtomicBool>,
) -> Result<(), &'static str> {
    Err("askpass_windows_only")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_long_and_unique() {
        let a = random_hex(32);
        let b = random_hex(32);
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(a.bytes().all(|c| c.is_ascii_hexdigit()));
    }

    fn credential(user: &str, host: &str, key_path: Option<&str>, password: &str) -> StoredCredential {
        StoredCredential {
            user: user.into(),
            host: host.into(),
            key_path: key_path.map(str::to_string),
            password: Zeroizing::new(password.into()),
        }
    }

    #[test]
    fn password_prompt_selects_the_matching_jump_host() {
        let credentials = vec![
            credential("admin", "bastion.example", None, "bastion-secret"),
            credential("deploy", "10.0.0.8", None, "target-secret"),
        ];
        assert_eq!(
            credential_index("admin@bastion.example's password: ", &credentials),
            Some(0)
        );
        assert_eq!(
            credential_index("deploy@10.0.0.8's password: ", &credentials),
            Some(1)
        );
        assert_eq!(credential_index("root@other-host's password: ", &credentials), None);
    }

    #[test]
    fn key_passphrase_prompt_matches_the_configured_identity_file() {
        let credentials = vec![credential(
            "deploy",
            "app.internal",
            Some(r"C:\Users\me\.ssh\id_ed25519"),
            "key-passphrase",
        )];
        assert_eq!(
            credential_index(
                "Enter passphrase for key 'C:\\Users\\me\\.ssh\\id_ed25519': ",
                &credentials
            ),
            Some(0)
        );
    }

    #[test]
    fn generic_password_prompts_fail_closed() {
        let one = vec![credential("u", "h", None, "secret")];
        assert_eq!(credential_index("Password: ", &one), None);
        assert_eq!(credential_index("Enter password: ", &one), None);
    }

    #[test]
    fn trust_questions_are_never_answered_with_the_password() {
        assert!(is_host_key_prompt(
            "The authenticity of host '192.168.1.2 (192.168.1.2)' can't be established.\n\
             ED25519 key fingerprint is SHA256:abcd.\n\
             Are you sure you want to continue connecting (yes/no/[fingerprint])? "
        ));
        assert!(is_host_key_prompt("Please type 'yes', 'no' or the fingerprint: "));
        assert!(is_host_key_prompt("WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!"));
    }

    #[test]
    fn credential_prompts_still_reach_the_helper() {
        assert!(!is_host_key_prompt("olfo@192.168.1.2's password: "));
        assert!(!is_host_key_prompt("Password: "));
        assert!(!is_host_key_prompt("Enter passphrase for key 'C:\\Users\\me\\id_ed25519': "));
    }

    /// Exercises the real production server loop (start -> connect -> reply),
    /// including the repeating timeout on server.connect(). A direct test of
    /// exchange() against a hand-made pipe would miss a cancellation bug in
    /// that loop, which is exactly what makes ssh wait forever for a password.
    #[cfg(windows)]
    #[test]
    fn askpass_server_delivers_the_password() {
        use std::time::Duration;

        // start() and the spawned server loop both use Tauri's runtime, so the
        // pipe handle and the task share the same reactor.
        tauri::async_runtime::block_on(async {
            let server = AskpassServer::start(vec![AskpassCredential {
                user: "olfo".into(),
                host: "192.168.1.2".into(),
                key_path: None,
                password: "s3cret".into(),
            }])
                .expect("AskpassServer::start must succeed");
            let pipe = server.pipe_name().to_string();
            let token = server.token().to_string();

            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                let _ = tx.send(exchange(&pipe, &token, "olfo@192.168.1.2's password: "));
            });
            let received = tauri::async_runtime::spawn_blocking(move || {
                rx.recv_timeout(Duration::from_secs(10))
            })
            .await
            .expect("blocking task panicked")
            .expect("server never answered");
            assert_eq!(received.unwrap(), "s3cret", "server must return the password");
            drop(server);
        });
    }

    #[cfg(windows)]
    #[test]
    fn askpass_server_routes_multiple_hop_passwords_once_each() {
        tauri::async_runtime::block_on(async {
            let server = AskpassServer::start(vec![
                AskpassCredential {
                    user: "admin".into(),
                    host: "bastion.internal".into(),
                    key_path: None,
                    password: "bastion-secret".into(),
                },
                AskpassCredential {
                    user: "deploy".into(),
                    host: "db.internal".into(),
                    key_path: None,
                    password: "database-secret".into(),
                },
            ])
            .expect("multi-hop askpass server must start");
            let pipe = server.pipe_name().to_string();
            let token = server.token().to_string();

            let pipe1 = pipe.clone();
            let token1 = token.clone();
            let first = tauri::async_runtime::spawn_blocking(move || {
                exchange(&pipe1, &token1, "admin@bastion.internal's password: ")
            })
            .await
            .expect("first askpass task panicked")
            .expect("bastion credential was not returned");
            assert_eq!(first, "bastion-secret");

            let pipe2 = pipe.clone();
            let token2 = token.clone();
            let second = tauri::async_runtime::spawn_blocking(move || {
                exchange(&pipe2, &token2, "deploy@db.internal's password: ")
            })
            .await
            .expect("second askpass task panicked")
            .expect("database credential was not returned");
            assert_eq!(second, "database-secret");

            let replay = tauri::async_runtime::spawn_blocking(move || {
                exchange(&pipe, &token, "admin@bastion.internal's password: ")
            })
            .await
            .expect("replay askpass task panicked");
            assert!(replay.is_err(), "the same credential must not be delivered twice");
            drop(server);
        });
    }

    /// Full helper walkthrough without launching a separate process: a server
    /// pipe in the tokio runtime serves `run_helper()` on a separate thread.
    #[cfg(windows)]
    #[test]
    fn helper_exchanges_token_for_password() {
        use std::time::Duration;
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::windows::named_pipe::ServerOptions;

        let name = format!(r"\\.\pipe\sshterm-test-{}-{}", std::process::id(), random_hex(4));

        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let code = rt.block_on(async {
            // create() needs the tokio reactor, so it goes inside the runtime.
            let mut server = ServerOptions::new()
                .first_pipe_instance(true)
                .create(name.as_str())
                .unwrap();

            // The client is launched after creating the pipe but before
            // connect(): no race, no infinite wait.
            let (tx, rx) = std::sync::mpsc::channel();
            let client_pipe = name.clone();
            let handle = std::thread::spawn(move || {
                let result = exchange(&client_pipe, "unit-test-token", "admin@host password: ");
                let _ = tx.send(result);
            });

            tokio::time::timeout(Duration::from_secs(5), server.connect())
                .await
                .expect("helper never connected")
                .unwrap();
            let mut line = String::new();
            {
                let mut reader = BufReader::new(&mut server);
                tokio::time::timeout(Duration::from_secs(5), reader.read_line(&mut line))
                    .await
                    .expect("helper never sent a request")
                    .unwrap();
            }
            let request: Request = serde_json::from_str(line.trim_end()).unwrap();
            assert_eq!(request.token, "unit-test-token");
            server.write_all(b"{\"password\":\"ok\"}\n").await.unwrap();
            server.flush().await.unwrap();

            let result = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            handle.join().unwrap();
            result
        });

        assert_eq!(code.unwrap(), "ok", "helper must receive the password");
    }
}
