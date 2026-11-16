# SSH Terminal Manager – Tauri Plan & Specs

A portable SSH terminal for Windows 11, built with Tauri 2, Rust and Svelte 5. The current transport runs the Windows OpenSSH client (ssh.exe) inside a ConPTY owned by the application. A native russh transport is future work.

This document distinguishes **implemented behavior**, **reserved data-model fields**, and **planned work**. The implementation review baseline is commit 01060eb79b1b021decc64ff6f16691495a9ffd8d on main; this specification describes that implementation and does not imply future milestones have shipped.

## 1. Goals and constraints

- **Portable data:** application data is resolved relative to the running executable; the app does not store its own configuration under %APPDATA% or use the registry as its primary store.
- **Modular:** Rust backend modules and Svelte components keep the vault, connection store, transport, session, terminal and logging responsibilities separate.
- **Secure by default:** saved credentials and key passphrases are stored in the encrypted vault. Private-key file contents remain outside the vault.
- **Phased SSH engine:** phase 1 uses Windows OpenSSH through ConPTY. The Transport trait is the boundary for a future native russh engine.
- **Primary target:** Windows 11 with the WebView2 runtime and Windows OpenSSH Client feature. A copy of ssh.exe can instead be placed in bin/ beside the app.
- **No application-owned registry configuration:** the PuTTY importer reads existing PuTTY sessions from the current user's registry; it does not use the registry to store SSHTerm settings.

## 2. Technology choices

| Layer | Current choice | Status / notes |
| --- | --- | --- |
| Shell | Tauri 2 | Implemented |
| Backend | Rust 2024 edition + Tokio | Implemented |
| SSH phase 1 | Windows ssh.exe through portable-pty / ConPTY | Implemented; only transport currently available |
| SSH phase 2 | russh / russh-keys | Planned for M10; not current dependencies |
| SFTP | russh-sftp | Planned for M8; no SFTP module or SFTP dependency is currently present |
| Crypto | argon2, chacha20poly1305, rand, zeroize | Implemented for vault encryption |
| Frontend | TypeScript + Svelte 5 + SvelteKit + Vite | Implemented |
| Terminal | @xterm/xterm + FitAddon | Implemented; canvas renderer is used, WebGL is not enabled |
| Serialization / config | serde, serde_json, toml | Implemented |
| Timestamps | chrono with clock feature | Used for log filenames and timestamps |
| Windows support | winreg, windows-sys | Windows-only PuTTY import and secure named-pipe ACL |

**Important dependency correction:** SFTP is not implemented “from the start.” Neither russh nor russh-sftp is in the current src-tauri/Cargo.toml; they are future work. The current app uses the operating system's OpenSSH client.

## 3. Portable layout and path resolution

Expected layout beside the executable:

```text
sshterm.exe
sshterm.vault      encrypted connection data and credentials
config.toml        non-secret settings: language and logging
known_hosts        plain-text OpenSSH host-key database
bin/
  ssh.exe          optional bundled OpenSSH client
logs/
  <connection>_<YYYYMMDD-HHMMSS>.log
  sshterm.log      diagnostic metadata log
webview-data/      WebView2 user-data/cache folder
highlight/         reserved for future TOML highlighting rules
```

Actual path behavior:

- paths::app_dir() resolves to the parent directory of current_exe().
- sshterm.vault, config.toml, known_hosts, logs/, highlight/ and webview-data/ are derived from that directory.
- WEBVIEW2_USER_DATA_FOLDER is set to webview-data/ before the Tauri builder creates the window.
- SSH lookup order is bin/ssh.exe, %SystemRoot%\System32\OpenSSH\ssh.exe, then %SystemRoot%\System32\ssh.exe. If none exists, session startup returns ssh_not_found.
- config.toml supports language, logging, terminal clipboard/mouse preferences, and optional multi-line paste confirmation. Font and default-keepalive settings are not implemented there.
- known_hosts is maintained by OpenSSH through the configured UserKnownHostsFile; it is not currently populated from the known_hosts map in the vault payload.
- highlight/ has a path helper and is ignored by Git, but the rule loader and rule files are not implemented.
- logs/sshterm.log is a separate diagnostic log containing session metadata and byte counts. Per-session terminal logs are separate files.

The generated data files (*.vault, config.toml, known_hosts, logs/, webview-data/ and highlight/) are in .gitignore. The docs/ directory is tracked in Git.

### Windows HTML drag-and-drop requirement

The connection tree uses browser HTML5 drag-and-drop. Tauri's native webview drag/drop handler intercepts those events when enabled, so the main window must keep this property in src-tauri/tauri.conf.json:

```json
{
  "app": {
    "windows": [
      {
        "title": "sshterm",
        "width": 800,
        "height": 600,
        "dragDropEnabled": false
      }
    ]
  }
}
```

dragDropEnabled: false disables Tauri's native drag/drop events for that window; it does **not** disable the frontend tree's HTML drag-and-drop. If native file-drop support is added later, it must be implemented through Tauri's native event API rather than relying on the browser drag handlers.

## 4. Vault specification

### On-disk format

| Field | Size | Content |
| --- | ---: | --- |
| Magic | 4 bytes | ASCII SSHV |
| Format version | 1 byte | 1 |
| KDF parameters | 12 bytes | Argon2 memory KiB, iterations and lanes; three little-endian u32 values |
| Salt | 16 bytes | Random per vault |
| Nonce | 24 bytes | Fresh random nonce for each save |
| Ciphertext | Remaining bytes | XChaCha20-Poly1305 ciphertext plus 16-byte authentication tag |

The header is authenticated as AEAD associated data. The complete header is 33 bytes; the nonce begins at byte 33 and ciphertext begins at byte 57.

### Current behavior

- KDF: Argon2id v1.3, 64 MiB memory, 3 iterations, 1 lane by default. The KDF parameters are stored in the header.
- KDF parameters are validated before use: lanes 1–16, iterations 1–20, and memory no greater than 1 GiB (with a minimum based on the lane count).
- Derived key length is 32 bytes.
- Wrong password and authenticated-data/ciphertext corruption return the same wrong_password_or_corrupt error. Unsupported file versions and malformed headers have separate errors.
- Saves serialize the payload, generate a fresh nonce, write sshterm.vault.tmp, call sync_all, copy the prior vault to .bak when it exists, and rename the temporary file.
- Changing the master password derives a new key with a new salt and re-encrypts the payload.
- vault_create, vault_unlock, vault_lock and vault_change_password are registered commands. The vault save operation is internal to the vault/store modules.
- Argon2 work for create/unlock runs on a blocking thread so it does not freeze the UI.
- The master password can be changed through the backend command; there is not currently a password-change screen in the UI.

### Memory-handling qualification

The derived key and decrypted serialized plaintext buffer use Zeroizing; askpass-held credential copies also use Zeroizing. The deserialized VaultData fields (including password and passphrase fields) are ordinary Rust String values and are dropped when the in-memory vault is dropped; they are **not individually guaranteed to be securely zeroized**. Do not describe every secret-bearing field as zeroizing until that model is hardened.

## 5. Data model (vault payload)

The encrypted JSON payload has version 1, a nested tree and a connection map. A connection node in the tree stores only the connection ID; the actual record is in connections.

```json
{
  "version": 1,
  "tree": [
    {
      "type": "folder",
      "id": "f1",
      "name": "Core",
      "children": [
        { "type": "connection", "id": "c1" }
      ]
    }
  ],
  "connections": {
    "c1": {
      "id": "c1",
      "name": "pgw-01",
      "host": "10.0.0.5",
      "port": 22,
      "user": "admin",
      "auth": { "type": "password", "password": "..." },
      "keepalive_secs": 30,
      "protected": false,
      "jump": ["c9"],
      "jump_disabled": [],
      "tunnels": [
        {
          "kind": "local",
          "bind": "127.0.0.1:8080",
          "dest": "10.1.1.1:80",
          "enabled": true
        }
      ],
      "colors": null,
      "sftp": null,
      "highlight": []
    }
  },
  "known_hosts": {}
}
```

### Connection fields and compatibility

- auth is a tagged enum: password (password string), key (key-file path plus optional passphrase), or agent.
- Private-key contents are never copied into the vault. The key path is passed to OpenSSH with -i; an optional passphrase is delivered through askpass.
- keepalive_secs is a u32; the default is 30 seconds. The editor accepts 1–3600 seconds. OpenSSH is also configured with ServerAliveCountMax=3.
- jump is an ordered list of saved connection IDs. jump_disabled records configured hops that are currently disabled; missing jump_disabled fields default to an empty list.
- tunnels supports local, remote and reserved dynamic variants. Each variant has an enabled flag; old tunnel entries without that field deserialize as enabled.
- colors, sftp and highlight are reserved/forward-looking fields. Colors are not applied by the UI, there is no SFTP implementation, and highlighting is not loaded.
- protected is editable and stored, but it has no active behavior until broadcast is implemented.
- known_hosts remains in the payload model for compatibility/future use. The current OpenSSH trust database is the plain-text known_hosts file beside the executable.

The payload version and encrypted file format remain at version 1. Unlock migrates older M1–M3 connection records in memory: old top-level password values move to auth, and unlinked flat connections are placed in a default Connections folder. The migration saves the updated payload without a version bump.

## 6. Backend architecture and command surface

The frontend calls the registered Tauri commands; it does not invoke Rust modules directly. Passwords are not returned by conn_list summaries.

| Module | Responsibility | Current commands / events | Status |
| --- | --- | --- | --- |
| vault / commands | Vault lifecycle and password change | vault_status, vault_create, vault_unlock, vault_lock, vault_change_password | ✅ Implemented |
| config | Non-secret language, logging, and terminal clipboard/mouse settings | config_get_language, config_set_language, config_get_terminal_settings, config_set_terminal_settings | ✅ Implemented |
| store | Connection CRUD, nested tree, search and imports | conn_list, conn_get, conn_upsert, conn_delete, tree_get, folder_upsert, folder_delete, move_node, search, import_preview, import_apply | ✅ Implemented |
| transport | Transport trait and OpenSSH/ConPTY implementation | Internal | ✅ OpenSSH only |
| askpass | Session-scoped named-pipe credential delivery | Internal | ✅ Implemented on Windows |
| session | Start/write/resize/close sessions and log bridge | session_open, session_write, session_resize, session_close, session_close_all, session_log_append, session_log_close; events session:data, session:closed, session:error | ✅ Implemented |
| logger | Per-session logs and diagnostic log output | Internal | ✅ Implemented |
| broadcast | Send input to multiple sessions | — | 📋 Planned M6 |
| sftp | Remote file browser and transfer operations | — | 📋 Planned M8 |
| highlight | Rule loading, validation and buffer decorations | — | 📋 Planned M6b |
| hostkeys | Dedicated host-key confirmation UI / management | — | 📋 Not implemented; current TOFU is handled by OpenSSH |

There is no standalone tunnel module or tunnel-start/stop command today. Local/reverse forwards are stored on a connection, validated by store, and passed to the OpenSSH process when an interactive session starts.

### Transport boundary

The current trait in src-tauri/src/transport/mod.rs is:

```rust
#[async_trait]
pub trait Transport: Send {
    async fn open(&mut self, cfg: &SshConfig, size: PtySize) -> Result<(), TransportError>;
    async fn write(&mut self, data: &[u8]) -> Result<(), TransportError>;
    async fn resize(&mut self, size: PtySize) -> Result<(), TransportError>;
    async fn close(&mut self) -> Result<(), TransportError>;
    fn output(&mut self) -> Option<OutputReceiver>;
}
```

OutputReceiver is a Tokio unbounded receiver of Vec<u8>. output() takes &mut self because the receiver is handed out once. SshConfig contains the SSH executable path, helper executable path, known-hosts path, target host/user/port, keepalive interval, optional target identity-file path, ordered jump hosts and enabled local/reverse forwards.

The current Connection model has **no engine field or engine selector**. OpenSshTransport is the only registered implementation. M10 may introduce a per-connection openssh / russh selection after the native transport reaches parity.

### OpenSSH command construction

The command is built with portable-pty::CommandBuilder; there is no shell interpolation. The current arguments/options are:

- -tt to force a remote interactive TTY.
- -p <port>.
- -o UserKnownHostsFile="<path>". The value is quoted for OpenSSH's config parser so paths containing spaces remain one known-hosts path; CommandBuilder still passes it as one argv item.
- -o StrictHostKeyChecking=accept-new.
- -o ServerAliveInterval=<keepalive_secs> and -o ServerAliveCountMax=3.
- -o ConnectTimeout=15.
- Optional target key: -i <key_path>.
- Optional jump route: -J user@host:port,user@host:port,...; IPv6 addresses are bracketed.
- If one or more forwards are enabled: -o ExitOnForwardFailure=yes, followed by one -L or -R argument for each enabled forward.
- Final destination: user@host.

### Password and key-passphrase hand-off

When the target or a jump hop has a saved password/passphrase, the session creates a named-pipe server and sets SSH_ASKPASS, SSH_ASKPASS_REQUIRE=force, DISPLAY, SSHTERM_ASKPASS_PIPE and SSHTERM_ASKPASS_TOKEN for the child process. The same executable is launched in helper mode; the helper sends the prompt and session token over the pipe and writes the returned credential to stdout for OpenSSH.

Security and routing behavior:

- The pipe is created with a Windows DACL that grants access only to the current user's SID. If secure pipe creation fails, the session fails rather than creating a less-restricted pipe.
- The token is a random 32-byte session token represented as hex. It authorizes pipe requests for that session; it is not a password and is not placed in the SSH command arguments.
- Password prompts are matched to the exact user@host; the destination key-passphrase prompt is matched to the configured identity-file path. Path matching is case-insensitive and separator-normalized on Windows.
- Each matched route credential is returned at most once. Credential copies held by the askpass server use Zeroizing.
- Host-key trust prompts are never answered with a password.
- Generic prompts such as Password: or Enter password: are deliberately **not** answered. The helper fails closed instead of guessing which route member requested a credential.
- Credentials are not placed on the command line or in environment variables. The environment contains helper/pipe metadata and the session token, not credential values.

### ConPTY output and session lifecycle

- A dedicated reader thread reads ConPTY output in 8 KiB chunks and forwards bytes to a Tokio channel.
- A watchdog polls child.try_wait() every 100 ms because ConPTY does not reliably close its output pipe when ssh.exe exits.
- On child exit, the watchdog allows 150 ms for final output, drops the PTY master to unblock the reader, waits up to 2 seconds for the reader to finish, then sends an empty chunk as an EOF sentinel.
- The session bridge forwards PTY output as base64 session:data events, emits session:closed at EOF and preserves terminal output.
- Recognized SSH connection, host-key and forwarding diagnostics are also emitted as session:error. The frontend buffers early data/errors so output emitted before a terminal pane subscribes is not lost.
- On manual vault lock, the frontend calls session_close_all before vault_lock; all active SSH children are closed. Dropping the transport on app exit also kills its child.

### Known phase-1 limitations

- OpenSSH process output and exit behavior are less structured than a native transport; the app recognizes a bounded set of common diagnostic strings rather than receiving typed SSH errors.
- Jump hops can use saved passwords or the SSH agent. A jump hop configured with its own custom private-key path is rejected (jump_key_auth_unsupported); ProxyJump does not provide a separate -i per hop in this design.
- The target and every active hop must have a distinct user@host identity. OpenSSH password prompts omit the port, so duplicate identities cannot be disambiguated safely.
- A generic askpass password prompt is unsupported and fails closed.
- The host-key prompt is not shown as an interactive confirmation UI: accept-new automatically trusts a first-seen key and rejects a changed key.
- A session's forwards exist only while its interactive SSH terminal session remains open.
- Windows OpenSSH must be installed or bundled. Askpass named-pipe ACL support is Windows-specific.

## 7. Frontend components and behavior

| Component | Purpose | Status |
| --- | --- | --- |
| LockScreen.svelte | Create a vault or unlock an existing vault | ✅ Implemented |
| LanguageSelect.svelte | Switch English / Spanish and persist selection | ✅ Implemented |
| ConnectionSidebar.svelte | Load and refresh tree, create folders/connections, search and launch import | ✅ Implemented |
| ConnectionTree.svelte | Recursive folder/connection tree, context menu, inline rename, drag-and-drop | ✅ Implemented |
| SearchBox.svelte | Debounced connection search and match count | ✅ Implemented |
| ImportDialog.svelte | Preview, review warnings/skips and apply imports | ✅ Implemented |
| ConnectionEditor.svelte | General, authentication, advanced, jump-host and tunnel sections | ✅ Implemented |
| Workspace.svelte | Tab bar, connection picker, terminal panes and workspace error banner | ✅ Implemented |
| TerminalPane.svelte | xterm.js terminal, FitAddon, focus, PTY resize and log capture | ✅ Implemented |
| Tunnel manager panel | Start/stop independent tunnels and show live status | 📋 Planned; no dedicated panel |
| SFTP panel | Browse/download/upload remote files | 📋 Planned M8 |
| Broadcast bar | Target multiple sessions | 📋 Planned M6 |
| Highlighting test panel | Paste sample text and test rule classification | 📋 Planned M6b |

The tree supports folders and connections, context-menu operations (open, edit, duplicate, create, rename and delete), F2/double-click rename, search and HTML drag-and-drop. Dragging is disabled while a search filter is active, because filtered indices cannot safely be mapped to the original tree indices. Dropping can move a node before/after another node, into a folder, or to the root. Backend validation rejects cycles and invalid destinations without removing the node first.

Each workspace tab owns one independent SSH session and PTY. Hidden tabs continue running; only the active pane resizes the PTY. Opening a session and switching tabs focuses the terminal. The session tab retains a tunnel indicator as a forwarding health check, not merely as a configured-forward count. Its intended states are pending while forwarding setup is being verified, healthy when every enabled forward has been accepted by OpenSSH, and failed when setup or a recognized forwarding diagnostic reports an error. A healthy indicator means the SSH forwarding setup is active; it does not guarantee that every application behind a forwarded destination is reachable.

## 8. Implemented feature specifications and planned work

### Logging (implemented M3)

**Why PTY-byte stripping was incorrect:** ConPTY maintains a screen buffer and sends redraws, including full repaints on resize/scroll and partial-line updates with cursor movement. Stripping escape codes from that stream loses the redraw context and creates duplicated MOTD lines, fragments and stray sequence parameters.

**Current solution:** xterm.js applies redraws to its terminal buffer. LogCapture.ts reads complete rendered lines from that buffer and sends them to Rust only after the cursor has moved below them.

```text
PTY redraws ──▶ xterm.js ──▶ terminal screen buffer
                                    │
                              LogCapture.ts
                              • IMarker tracks the first unwritten line
                              • handles screen erase (CSI J) and RIS
                              • joins wrapped rows before writing
                              • skips alternate-screen applications (vim, less, top)
                              • deduplicates repainted lines using recent-line overlap
                              • collapses consecutive blank lines
                              • flushes batches every 250 ms
                                    │ session_log_append / session_log_close
                              SessionLogger (Rust)
                              • Screen mode: rendered plain text
                              • Raw mode: PTY bytes, including escape sequences
                              • create_new collision handling: _2, _3, ...
                              • closing RFC 3339 timestamp
                                    │
                      logs/<connection>_<YYYYMMDD-HHMMSS>.log
```

Configuration (config.toml):

```toml
language = "en"
log_output = true
log_mode = "screen"   # "screen" (default) or "raw"
```

- log_output defaults to true; false means no per-session log file is created.
- screen mode records rendered terminal text, with trailing whitespace trimmed and consecutive blank lines collapsed. It omits alternate-screen content and progress-bar redraws that are overwritten before capture.
- raw mode records PTY output bytes exactly as received, including terminal escape sequences; it is intended for diagnostics, not clean transcripts.
- The header contains the connection name, user, host and start timestamp. The footer contains the end timestamp.
- Filenames sanitize Windows-illegal characters, trim trailing dots/spaces, truncate the connection name to 80 characters and use _2, _3, etc. if two sessions collide within one second.
- Typed input is not separately intercepted or written to the log. Screen mode logs what xterm.js renders; raw mode stores the PTY output stream.
- Failure to create/write a per-session log is deliberately non-fatal to the SSH session.
- Legacy log_input and strip_special TOML fields are ignored; they are not current settings.

The separate logs/sshterm.log file records operational metadata such as session ID, target host/port/user, jump route, SSH executable path, event counts and byte counts. It does not record terminal contents or credential values.

### Jump hosts (M5 — implemented)

- A connection stores an ordered jump list of connection IDs. The editor lets the user add, remove, reorder and enable/disable configured hops.
- The jump picker searches saved connections by name, host or user.
- Nested jump definitions are expanded into outermost-to-target order. Disabled hops are skipped, including disabled nested hops.
- Backend validation rejects missing IDs, duplicate hops, cycles, invalid ProxyJump host/user values and chains deeper than 16 hops.
- The active route is passed to OpenSSH using one -J argument. IPv6 literals are bracketed.
- The session tab and startup banner show the resolved route.
- Different saved passwords per hop are supported through prompt-to-credential matching, subject to the distinct-identity and custom-key limitations in §6.
- Tests cover nested/ordered route expansion, disabled hops, missing/duplicate/cyclic routes, IPv6 formatting, credential prompt matching, key-passphrase path matching and fail-closed generic prompts.

### Local and reverse TCP forwarding (M7 — partial)

Implemented:

- Local (-L) and remote/reverse (-R) forwards are stored per connection.
- Each saved forward can be enabled or disabled independently. Existing entries without enabled default to enabled.
- Disabled forwards remain saved but are omitted from the SSH command line.
- Listen endpoints accept [address:]port; a bare port uses OpenSSH's loopback default. Destinations require host:port.
- IPv6 literals must use brackets, for example [::1]:5432.
- Enabled endpoints are normalized and validated for port range, malformed values and duplicate listeners of the same forwarding direction.
- ExitOnForwardFailure=yes tells OpenSSH to exit when a requested listener cannot be created.
- Forwards start with the interactive SSH session and stop when that session closes.
- Recognized local/reverse forwarding and connection failures are surfaced in the workspace error banner while the original SSH output remains in the terminal.
- **Session-tab health indicator (retain):** the tab must expose forwarding health for the active session rather than presenting only a static count. It should show pending during SSH/forward setup, healthy only after all enabled forwards have been accepted, and failed when OpenSSH reports a forwarding setup/runtime error. Associate the status with its session so one tab's error cannot mark another tab's forwards unhealthy.
- Use OpenSSH's `ExitOnForwardFailure=yes` as the initial setup guarantee, together with explicit status reporting. Do not mark forwards healthy merely because `session_open` spawned the process; it returns before the SSH handshake and forwarding setup have necessarily completed.
- Health indicates that the SSH forwarding setup/listener is active. It is not an end-to-end probe of the application listening at the forwarded destination; destination reachability may still need a separate optional probe.
- For a remote listener to accept connections from hosts other than the SSH server itself, the server-side sshd configuration may need to permit it with GatewayPorts.

Not implemented:

- Standalone tunnel-only sessions.
- Start/stop controls independent of opening a terminal.
- A dedicated tunnel manager and structured active/failed/port-in-use state.
- Dynamic SOCKS forwarding (-D). The Dynamic vault enum exists for forward compatibility, but enabling one is rejected as tunnel_dynamic_unsupported; the editor displays an unsupported notice for old entries.

### Keepalive

- Each connection stores keepalive_secs; the editor accepts 1–3600 seconds. A zero/missing value is normalized to the 30-second default.
- OpenSSH receives ServerAliveInterval=<keepalive_secs> and ServerAliveCountMax=3.
- Automatic reconnect with backoff is not implemented.

### M6 — Tiling and broadcast (planned)

- Planned layouts: current tabs, horizontal split, vertical split and 2×2 grid.
- Planned broadcast targets: visible panes, all open sessions or a selected subset.
- protected is already persisted and editable, but broadcast exclusion and its operational meaning are not implemented yet.
- Planned multi-line paste confirmation before broadcasting.

### M6b — Syntax highlighting (planned)

The vault connection model already has a highlight: string[] field, but there is no rule loader, matcher, frontend decoration layer, test panel, or highlight/ rule set in the current implementation. Folder nodes also do not yet have a highlight property, so inherited folder rules require a model change.

Intended design:

- Plain TOML rule files in highlight/ next to the executable; no secrets in rule files.
- A rule set contains a prompt regex and an ordered list of scoped rules (prompt, command, output, any).
- Classifications include prompt, command, parameters, output, IP addresses and keywords.
- Matching runs against the rendered xterm.js buffer through decorations; it must not rewrite PTY bytes.
- Wrapped lines are joined before matching; highlighting is disabled while the alternate screen is active.
- Add schema validation and an in-app test panel. A command-line checker (--check-highlight) is a proposal, not an existing command.
- Guard against pathological regex cost. Prefer a linear-time regex engine or strict validation rather than executing arbitrary backtracking patterns in the UI.

Example rule-file direction (not currently loaded by the app):

```toml
name = "linux"
prompt = '^\S+@\S+:[^\n]*[$#] '

[[rule]]
name = "command"
scope = "command"
pattern = '^\\S+'
fg = "#e3b341"
bold = true
```

### M8 — Native SFTP (planned)

- Add russh and russh-sftp; neither is currently a dependency.
- Implement remote directory listing, single-file and recursive-folder downloads, uploads, delete/rename, multi-select, progress, cancellation and overwrite confirmation.
- Support the connection's host, port and jump route, with an optional separate SFTP port.
- Decide whether SFTP requires the native SSH engine for the first release or whether a separately managed OpenSSH SFTP process is preferable. The earlier “from the start” assumption is not current implementation.

### M9 — Polish and lifecycle (planned)

- Apply per-connection foreground/background colors to xterm.js and tab accents; colors is currently only an optional stored field.
- Add a dedicated host-key management/confirmation UI. Current OpenSSH behavior is automatic first-use acceptance (accept-new) and refusal of changed keys, not a custom confirmation dialog.
- Add automatic reconnect with backoff.
- Add configurable idle locking. Current locking is manual and closes all active SSH sessions.
- Review release packaging and signing. The current CI produces a portable executable artifact without bundling an installer.

### M10 — Native SSH engine (planned)

- Add RusshTransport implementing the existing Transport trait.
- Achieve feature parity for terminal sessions, authentication, jump chains, keepalive and forwarding before making the native transport the default.
- Add typed SSH errors rather than relying on parsing OpenSSH output.
- Add chained direct-tcpip jump channels and native tunnel listeners.
- Only after parity, introduce a per-connection engine selector such as openssh / russh; no such selector exists today.
- Keep private-key file contents out of the vault.

## 9. Security checklist and current gaps

### Implemented safeguards

- Vault payload is encrypted with Argon2id-derived XChaCha20-Poly1305 keys; header metadata is authenticated.
- Wrong password and ciphertext/header tampering fail authentication.
- The askpass pipe is created with a current-user-only Windows DACL; failure to create that secure pipe prevents the session from starting.
- Askpass uses a random session token, exact prompt matching and at-most-once delivery for each matched credential.
- Passwords and key passphrases are not placed in SSH command-line arguments or environment variables.
- Private-key contents remain at the user's chosen file path; the vault stores the path and optional passphrase only.
- OpenSSH uses the app-local known_hosts file. StrictHostKeyChecking=accept-new automatically accepts first-seen keys and rejects changed keys.
- The transport kills the ssh.exe child when a session closes, the app locks the vault through the normal UI flow, or the transport is dropped on app exit.
- The session diagnostic log records metadata and byte counts, not terminal contents or credential values.

### Not implemented / security limitations to keep visible

- **Secure erasure of all in-memory secrets:** the derived key, decrypted serialization buffer and askpass credential copies are Zeroizing, but the deserialized vault's password/passphrase fields are ordinary String values.
- **Idle lock:** no idle timer exists. The user can lock manually; the UI closes all sessions before dropping the vault.
- **Clipboard controls:** clipboard auto-clear after copying a password and terminal clipboard-write approval are not implemented.
- **Custom host-key prompt UI:** first-seen hosts are accepted automatically by OpenSSH's accept-new policy. Changed keys are rejected, but there is no dedicated old/new fingerprint confirmation screen.
- **Key-file protection:** private-key contents are outside the vault and are not protected by the vault master password; file-system permissions and any key passphrase remain the user's responsibility.
- **Logging scope:** screen mode records terminal text that is rendered; raw mode records PTY output bytes. Do not treat raw logs as sanitized transcripts. Remote output may itself contain sensitive information.
- **No per-hop custom key path:** jump hosts using their own saved identity-file path are rejected by phase 1.
- **No generic askpass fallback:** unknown or generic password prompts receive no credential.

## 10. Repository layout

```text
sshterm/
  .github/workflows/windows.yml
  README.md
  ROADMAP.md
  docs/
    SSH Terminal Manager – Tauri Plan & Specs.md
    M4-implementation-plan.md
    m4-architecture.md
    ...
  src-tauri/
    Cargo.toml
    Cargo.lock
    tauri.conf.json
    src/
      main.rs              # detects askpass helper mode before creating Tauri
      lib.rs               # app setup, command registration, WebView2 folder
      paths.rs             # exe-relative paths and ssh.exe lookup
      commands.rs          # vault state and vault/config commands
      config.rs            # language, log_output, log_mode
      vault/               # crypto, errors, typed payload, tests
      store/               # connection/tree commands and import parsers
      transport/           # Transport trait and openssh.rs
      askpass/              # named-pipe protocol and Windows DACL
      session/              # PTY lifecycle, Tauri events and log bridge
      logger/               # per-session log files
  src/
    routes/
      +layout.ts
      +page.svelte
    lib/
      LockScreen.svelte
      LanguageSelect.svelte
      ConnectionSidebar.svelte
      ConnectionTree.svelte
      SearchBox.svelte
      ImportDialog.svelte
      ConnectionEditor.svelte
      Workspace.svelte
      TerminalPane.svelte
      logCapture.ts
      sessionBus.ts
      encoding.ts
      types.ts
      i18n/
        en.ts
        es.ts
        index.svelte.ts
```

There are no current source modules for broadcast, sftp, hostkeys, highlight or russh. The tree above lists the implemented code; the earlier proposed layout for these future modules should not be treated as present-day structure.

## 11. Milestone status

| # | Milestone | Current status |
| --- | --- | --- |
| M0 | Portable Tauri skeleton and WebView2 data folder | ✅ Complete |
| M1 | Encrypted vault and English/Spanish UI | ✅ Complete |
| M2 | OpenSSH session over ConPTY, xterm.js, askpass and keepalive flags | ✅ Complete |
| M3 | Per-session logging with xterm.js screen-buffer capture | ✅ Complete |
| M4 | Connection tree, CRUD, search, imports, drag-and-drop and multi-tab workspace | ✅ Complete |
| M5 | Nested ProxyJump chains and per-hop passwords (within phase-1 limits) | ✅ Complete |
| M6 | Tiling and broadcast | 📋 Planned |
| M6b | Syntax highlighting | 📋 Planned |
| M7 | Local/reverse forwarding (-L / -R) | 🟡 Partial; bound to interactive sessions |
| M8 | Native SFTP | 📋 Planned |
| M9 | Colors, host-key UI, auto-reconnect, idle lock and release polish | 📋 Planned |
| M10 | Native russh transport | 📋 Planned |

The status reflects the source tree, not the aspirational feature descriptions. In particular, M5 is implemented, M7 is partial, and the colors, sftp, highlight, protected and dynamic data fields do not imply that their corresponding features are complete.

## 12. Decisions, operational workflow and CI

### Current decisions

- Frontend: Svelte 5 + TypeScript.
- Runtime: Tauri 2, Rust 2024 edition, Tokio.
- Phase-1 SSH: Windows OpenSSH ssh.exe launched through portable-pty/ConPTY.
- SSH executable search: bin/ beside the executable, then %SystemRoot%\System32\OpenSSH, then %SystemRoot%\System32.
- Private keys: store only the file path and optional passphrase; never copy key contents into the vault.
- Host keys: OpenSSH manages the app-local known-hosts file with StrictHostKeyChecking=accept-new; a changed key is rejected.
- Logging: screen-buffer capture is the default; raw PTY capture is an opt-in diagnostic mode.
- Imports: OpenSSH config, PuTTY registry sessions (Windows only) and WinSSHTerm connections.xml.
- No code signing is configured at present.

### Import behavior and limitations

- **OpenSSH config:** imports the supported Host, HostName, Port, User and IdentityFile values. Wildcard host blocks and Match blocks are skipped; Include directives are reported as ignored. This is not a full OpenSSH config evaluator.
- **PuTTY:** reads SSH sessions from HKCU\Software\SimonTatham\PuTTY\Sessions, including host, port, user and public-key-file path. Saved passwords are not imported.
- **WinSSHTerm:** parses connections.xml, including nested containers/folders and schema-tolerant attribute/child-element fields. Folder paths are recreated and matching folder names are reused.
- Import always has a read-only preview showing candidates, skips and warnings before apply.
- Duplicate detection is by case-insensitive (host, port, user). Default policy skips duplicates; “copy” creates another connection with a unique display name.
- Saved passwords from PuTTY and WinSSHTerm are never imported. WinSSHTerm passwords are encrypted with its own key; imported records needing credentials must be edited.
- Importers do not currently bring in jump chains, port forwards or login commands.

### Windows CI and portable build

.github/workflows/windows.yml runs on pushes to main, feature/** and fix/**, on pull requests, and by manual dispatch. It:

1. Checks out the source.
2. Installs Node.js 22 and stable Rust for x86_64-pc-windows-msvc, including rustfmt.
3. Runs npm ci, npm run check and npm run build.
4. Runs cargo fmt --all to normalize the CI checkout, then runs the formatter with --check. This normalization is not committed back to the repository.
5. Runs cargo test --locked.
6. Runs npm run tauri -- build --no-bundle.
7. Uploads src-tauri/target/release/sshterm.exe as the sshterm-windows-x64 artifact for 30 days.

A successful workflow run therefore validates frontend checks/build, Rust formatting/tests and the Windows executable build. It does not run a live SSH integration test against external servers, and the ignored ConPTY diagnostic test requires an attached interactive console.

### Release and runtime prerequisites

- The CI artifact is the portable sshterm.exe; because CI uses --no-bundle, it does not upload an MSI installer.
- The runtime machine still needs WebView2 and OpenSSH, unless ssh.exe is supplied in the adjacent bin/ directory.
- Keep dragDropEnabled: false on the main Tauri window while the connection tree uses HTML5 drag-and-drop.


## 13. Terminal clipboard and mouse settings (implemented)

Terminal clipboard behavior is configured independently in the application Settings dialog and persisted in `config.toml` under the `terminal` table. Missing fields in older config files use the defaults (enabled).

| Setting | Default | Behavior |
| --- | --- | --- |
| `copy_on_selection` | `true` | Copy selected terminal text automatically. |
| `right_click_copy` | `true` | Right-click copies selected text when enabled and a selection exists; the selection is cleared after copying. |
| `middle_click_copy` | `true` | Middle-click copies selected text when enabled and a selection exists; the selection is cleared after copying. |
| `right_click_paste` | `true` | Right-click pastes when there is no selection, or when a selection exists but right-click copy is disabled. Any selection is cleared before paste. |
| `middle_click_paste` | `true` | Middle-click pastes when there is no selection, or when a selection exists but middle-click copy is disabled. Any selection is cleared before paste. |
| `ctrl_c_copy` | `true` | Ctrl+C copies when text is selected and clears the selection. With no selection, Ctrl+C is sent to the remote shell as the interrupt character. If this setting is disabled, Ctrl+C is sent to the remote shell even when text is selected. |
| `confirm_multiline_paste` | `true` | Ask for confirmation before pasting clipboard text containing a line break. The warning is optional and can be disabled in Settings. |

Ctrl+V always pastes from the system clipboard and is not configurable. Its custom key handler prevents WebView's native paste handling so the same clipboard text is not inserted twice. Right- and middle-click prefer copy when a selection exists and their copy option is enabled; otherwise, their paste option can run and clears any existing selection first. When multi-line confirmation is enabled, all paste entry points (Ctrl+V, right-click and middle-click) use the same warning before sending clipboard text to the terminal. The dialog reports the number of lines but does not display clipboard contents; Cancel, Escape or clicking outside the dialog cancels the paste. Single-line paste proceeds immediately. Clipboard access is performed by the WebView clipboard API; unavailable or denied clipboard permissions fail without sending unintended terminal input.

Example:

```toml
[terminal]
copy_on_selection = true
right_click_copy = true
middle_click_copy = true
right_click_paste = true
middle_click_paste = true
ctrl_c_copy = true
confirm_multiline_paste = true
```
