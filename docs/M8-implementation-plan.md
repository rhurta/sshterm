# M8 — Native SFTP implementation plan

**Status:** M8a in progress; architecture and delivery gates defined  
**Scope:** A connection-backed SFTP browser with direct and multi-hop access, file operations, and cancellable transfers.  
**Dependency boundary:** M8 introduces the native `russh` client for SFTP only. Interactive terminals continue using Windows OpenSSH through ConPTY; the broader transport replacement remains M10.

## 1. Goals and acceptance

M8 is complete when a user can open an SFTP browser for a saved connection and:

- Browse remote directories and inspect name, type, size, modification time and permissions.
- Navigate from the configured start directory, move between parent/child folders, and refresh.
- Upload and download individual files and recursively transfer folders.
- Select multiple items and queue their transfers with per-file and overall progress.
- Cancel an active transfer without leaving a misleading partial destination.
- Create directories, rename files/directories, and delete files or empty directories.
- Receive an explicit overwrite confirmation before replacing an existing destination.
- Use the saved host, SSH port, credentials and ordered ProxyJump route. An optional SFTP-specific SSH port can override the target connection's port.
- Keep SFTP operations independent of terminal tabs and tunnel processes.
- Close SFTP sessions and cancel transfers on vault lock and application exit.

The implementation must not store credentials, file contents, transfer drafts or remote directory listings in plain `config.toml`. Secrets remain in the encrypted vault or transient memory.

## 2. Architecture decisions

### 2.1 Use `russh` + `russh-sftp` for SFTP only

Use the async `russh::client` API to establish an authenticated SSH connection, open a session channel, request the `sftp` subsystem and pass that channel's stream to `russh_sftp::client::SftpSession`.

Keep the existing `OpenSshTransport`, ConPTY and xterm.js path unchanged. M8 does not migrate interactive terminals, independent M7 tunnels or the existing `Transport` trait to Russh. The native engine work remains M10.

Initial dependency baseline to verify in M8a:

- `russh = 0.65.2`
- `russh-sftp = 3.0.1`
- Exactly one supported Russh crypto backend feature (`aws-lc-rs` or `ring`), selected after Windows x64 build and interoperability checks.

These are the current documented releases checked on 20 November 2026, not a promise to float dependencies automatically. Commit the resolved versions in `Cargo.lock`. Do not use a Russh release earlier than 0.65.1: versions before 0.65.1 are affected by GHSA-9264-jvgf-6w2f / CVE-2026-128087. Keep the default no-op `kex_done` handler; no callback should perform work before host-key verification.

### 2.2 Reuse connection resolution and build a native jump chain

Reuse `store::resolve_jump_chain` to obtain the enabled, validated route in outermost-to-target order. Reuse the existing connection records for each hop's host, port, user and authentication; do not rebuild or reinterpret jump relationships in the frontend.

For a direct connection, open a TCP stream to the target. For a jump route:

1. Connect and authenticate to the outermost jump host.
2. Open a `direct-tcpip` channel from that hop to the next hop.
3. Feed the channel stream to `russh::client::connect_stream` for the next SSH connection.
4. Repeat until the target connection is authenticated.
5. Open the target's SFTP subsystem channel.

Keep every upstream SSH handle/channel alive for the lifetime of the target SFTP session. Close the route from the target back outward. Each hop must be allowed to use TCP forwarding; surface a clear error when the server rejects `direct-tcpip`.

### 2.3 Authentication parity

Map the saved `Auth` model to Russh explicitly for the target and every jump hop:

- Password: use the password stored in the vault. An empty saved password must fail with a clear credentials-required error; there is no terminal prompt in an SFTP browser.
- Private key: read the configured key file in place and use the saved optional passphrase. Never copy key contents into the vault, config, logs or temporary files.
- Agent: use the Windows OpenSSH agent named pipe, with Pageant as a bounded fallback. Bind the agent to the verified SSH session before requesting identities. Certificate identities and keyboard-interactive multi-factor challenges remain explicit follow-up cases; do not silently downgrade or answer arbitrary prompts.

Do not silently fall back from a selected key/password method to an unrelated credential. Keep per-hop identities separate. A saved custom key for a jump hop—currently unsupported by the OpenSSH ProxyJump tunnel path—must be covered by the native SFTP auth tests.

### 2.4 Host-key trust must match the existing application policy

The current OpenSSH phase uses the portable `known_hosts` file and trust-on-first-use (accept-new): an unknown host is recorded; a changed known key is rejected. The Russh `Handler::check_server_key` implementation must preserve that security behavior for the target and every jump hop.

Use the portable known-hosts path, not a user's unrelated default file. M8a must test `russh::keys::known_hosts::check_known_hosts_path` and `learn_known_hosts_path` against existing OpenSSH entries, including hashed hostnames and non-default ports. Learn a key only when the host is genuinely unknown; never convert a changed-key error into a new trust record. Do not implement `check_server_key` as unconditional `Ok(true)`.

The dedicated host-key confirmation UI remains an M9 item. Until then, preserve current TOFU and changed-key rejection behavior and give the user a clear error.

### 2.5 SFTP port and start directory

The vault already reserves `Connection.sftp` and `SftpOptions { port, start_dir }`. M8a must finish its semantics before adding UI:

- No SFTP port override means inherit `Connection.port`, including non-22 connection ports.
- A configured override applies only to the target SSH connection; jump hosts continue to use their own saved ports.
- Prefer `Option<u16>` for the override so inheritance is explicit; validate any supplied port as 1–65535.
- An empty `start_dir` means the server's default/home directory. A configured path is used after SFTP initialization and must be treated as a remote POSIX-style path, not a local Windows path.

Preserve deserialization of existing vaults. Add round-trip and older-payload tests before the editor starts writing these fields. A vault-format bump is not expected.

## 3. Backend design

Add a dedicated `src-tauri/src/sftp.rs` module and managed `SftpState`. Keep it separate from `session::SessionState` and `tunnels::TunnelState`.

Proposed responsibilities:

- `SftpConnection`: authenticated target handle, retained jump-route handles/channels, `SftpSession`, connection identity and initial remote path.
- `SftpState`: map of SFTP session IDs to live connections, plus a transfer registry.
- `SftpEntry`: serializable file metadata for the frontend (name, remote path, entry kind, size, modified time and permissions).
- `TransferJob`: transfer ID, session ID, direction, source/destination paths, queue state, byte counts, optional total size, cancellation token and terminal result.

Expose narrow Tauri commands; suggested names are:

- `sftp_open(connection_id)` / `sftp_close(session_id)` / `sftp_close_all()`
- `sftp_list(session_id, remote_path)`
- `sftp_mkdir(session_id, remote_path)`, `sftp_rename(...)`, `sftp_delete(...)`
- `sftp_transfer_start(session_id, items, direction, local_destination)`
- `sftp_transfer_cancel(transfer_id)`

The exact payloads should be finalized in M8b, but keep them typed and validate every path and operation in Rust. The frontend must not receive SSH handles or credentials.

Use async Tauri commands and Tokio I/O. Do not hold a synchronous mutex across an await, and do not perform file copies on the WebView thread. On vault lock, close all SFTP sessions and cancel/reap their transfer jobs regardless of the M7 `stop_tunnels_on_lock` preference; that preference controls independent tunnels only.

## 4. File operations and transfer behavior

### Browser

Create a dedicated `SftpBrowser.svelte` view, opened from a saved connection's context/action menu. It should show the connection and jump route, a remote path/breadcrumb field, refresh/up navigation, and a keyboard-navigable listing. Use the existing themes and component patterns. Give search/path input focus whenever a picker or searchable popover opens.

Initial operations: list, navigate, refresh, create directory, rename, delete file, delete empty directory. Do not recursively delete a non-empty directory in the first release; return a clear error rather than risking accidental data loss.

### Transfers

- Multi-select adds files/folders to a visible queue. Folder selection expands recursively while preserving relative paths.
- Stream files in bounded chunks with backpressure. Do not read an entire large file into a `Vec<u8>`.
- Report `sftp:progress` events with transfer ID, current file, files completed/total, bytes transferred and total bytes when known.
- Use a cancellation token per job. Cancellation closes the active file/channel operation, removes temporary partial output where possible and records Cancelled; it must not report success.
- For downloads, write to a sibling temporary `.part` file and rename to the final local path only after a successful close/flush.
- For uploads, write to a temporary remote sibling and rename only after the upload is complete. Preserve an existing destination until the replacement is ready; if the remote server cannot provide the needed rename semantics, fail explicitly rather than deleting the old file first.
- Never overwrite silently. The UI asks before replacing an existing file. The backend still validates the final operation and reports conflicts.
- Keep the initial queue sequential per SFTP session for predictable cancellation and file-handle ownership. Add bounded parallel transfers only if measured performance justifies it and the channel/session safety is tested.

Use the Tauri dialog plugin for local file/folder selection rather than exposing arbitrary local filesystem access to the WebView. The plugin permissions and Windows packaging must be included in M8c.

## 5. Delivery phases

### M8a — Protocol and security spike

**Progress:** SFTP port inheritance and backward-compatible vault decoding are complete. The native protocol layer now opens authenticated Russh sessions directly or through ordered `direct-tcpip` jump hops, verifies host keys against the portable `known_hosts` file, and starts the SFTP subsystem. UI and Tauri command integration remain deferred until the protocol is validated against real Windows OpenSSH servers.

- [x] Pin `russh 0.65.2` + `russh-sftp 3.0.1`, using the Ring crypto backend.
- [x] Finalize `SftpOptions.port` inheritance and backward compatibility for saved numeric port values.
- [x] Implement direct and multi-hop SSH authentication and SFTP subsystem setup.
- [x] Implement portable known-host checking, accept-new learning and changed-key rejection.
- [x] Implement direct password and private-key authentication (local OpenSSH integration pending).
- [x] Implement SFTP subsystem setup over `Channel::into_stream()` (integration pending).
- [x] Implement ordered multi-hop `direct-tcpip` channels and retain upstream handles (integration pending).
- [x] Implement host-key TOFU, known-key match and changed-key rejection; unit tests added.
- [x] Implement Windows OpenSSH agent / Pageant signer with session binding (integration pending).
- [ ] Verify hashed host entries and live non-default-port connections against Windows OpenSSH.
- [ ] Verify Pageant, passphrase-protected key and distinct credentials on each hop against live servers.
- [ ] Prove vault compatibility with an encrypted-vault fixture and older connection payloads.
- [ ] Gate: no SFTP browser UI until direct, jump, authentication and host-key checks are working.

**Security constraint:** use Russh 0.65.1 or later. Russh 0.65.2 is the selected baseline; keep the default no-op `kex_done` handler and do not expose or act on KEX shared-secret material. Versions before 0.65.1 are affected by GHSA-9264-jvgf-6w2f / CVE-2026-128087.

### M8b — SFTP backend and session lifecycle

**Progress:** The backend session registry and typed directory-listing contract are implemented. Sessions open from saved vault connections, including resolved jump chains, and expose runtime-only IDs and connection metadata (never credentials). Directory navigation canonicalizes remote POSIX paths and returns typed entries with type, size, modification time and permissions. Lifecycle cleanup closes sessions on explicit close, close-all, vault lock and app exit. A lifecycle gate prevents open/list/close races during vault-lock cleanup.

- [x] Add managed SFTP session state and typed session/directory/entry responses.
- [x] Add `sftp_open`, `sftp_list`, `sftp_close` and `sftp_close_all` commands.
- [x] Resolve saved connections and ordered jump chains in Rust; keep credentials out of frontend payloads.
- [x] Close SFTP sessions on vault lock independently of the M7 tunnel-lock preference.
- [x] Close SFTP sessions on application exit.
- [x] Add remote path validation and path-navigation unit tests.
- [ ] Verify lifecycle and directory listing against Windows OpenSSH (manual acceptance).
- [ ] Add integration coverage for open/list/close and vault-lock races.

### M8c — Browser UI and file operations

- Add SFTP entry points in the connection UI and the dedicated browser.
- Implement path navigation, listing, refresh, create directory, rename and safe delete.
- Add local open/save/folder dialogs and keyboard navigation.

### M8d — Transfer queue

- Implement single/multi-file and recursive folder upload/download.
- Add overwrite confirmation, per-file/overall progress, cancellation and partial-file cleanup.
- Handle network drop, permission errors, remote disk full, disconnect and retry-safe cleanup.

### M8e — Hardening and release gate

- Run the Windows portable build and Rust/frontend checks.
- Add Linux local-OpenSSH integration coverage for direct and multi-hop SFTP, while retaining Windows x64 build coverage.
- Test large files, empty files, Unicode and spaces in names, deep folders, non-22 ports, overwrite/cancel, permission errors and remote disconnect.
- Confirm vault lock and app exit close SFTP and leave no background transfers or partial final files.
- Confirm M7 tunnels and existing OpenSSH terminal sessions are unaffected.

## 6. Explicit non-goals

M8 does not replace the terminal engine, add native terminal sessions, rewrite M7 tunnels, add file synchronization/watch, remote editing, SCP/rsync, permission/chmod management, or recursive destructive directory deletion. Those require separate scope; terminal and native transport parity remains M10.

## 7. Technical references checked on 20 November 2026

- [Russh 0.65.2 documentation](https://docs.rs/russh/0.65.2/russh/) — async SSH client, `connect_stream`, authentication, direct TCP/IP channels, crypto backend features and Pageant support.
- [russh-sftp 3.0.1 documentation](https://docs.rs/russh-sftp/3.0.1/russh_sftp/) — SFTP v3 client and server.
- [SftpSession API](https://docs.rs/russh-sftp/3.0.1/russh_sftp/client/struct.SftpSession.html) — high-level filesystem-like async methods and stream-backed file handles.
- [Russh SFTP client example](https://github.com/AspectUnk/russh-sftp/blob/master/examples/client.rs) — open an SSH session channel, request the `sftp` subsystem and wrap the channel stream.
- [Russh known-hosts implementation](https://docs.rs/russh/0.65.2/src/russh/keys/known_hosts.rs.html) — custom known-hosts path, hashed host matching and non-default port representation.
- [GHSA-9264-jvgf-6w2f](https://github.com/Eugeny/russh/security/advisories/GHSA-9264-jvgf-6w2f) — minimum safe baseline is Russh 0.65.1; the proposed 0.65.2 baseline is above that floor.

