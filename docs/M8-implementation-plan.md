# M8 — Native SFTP implementation plan

**Status:** Implementation complete; core Windows workflow manually accepted on 22 November 2026. Remaining unchecked items are targeted compatibility, failure-injection and lifecycle/regression hardening.  
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

**Workspace integration:** SFTP browsers are first-class tabs in the same tiled workspace and pane tab strips as SSH terminals. A pane can contain any mix of SSH and SFTP tabs, and SFTP tabs use the existing drag/drop, reorder, split, focus and keyboard-move behavior. Each top-level SFTP workspace tab owns exactly one saved connection; opening another connection creates another tab at this same level. There is no nested SFTP session tab strip. Moving an SFTP tab re-parents its mounted browser so its connection, directory state and active transfers survive.

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
- Agent: use the Windows SSH agent named pipe selected by `SSH_AUTH_SOCK` (or the standard OpenSSH pipe), with Pageant as a bounded connection fallback. Bind the agent to the verified SSH session before requesting identities when the agent supports `session-bind@openssh.com`. On Windows, if the selected agent explicitly rejects the bind request with `AgentFailure` (including the Windows OpenSSH 9.5 behavior where native `ssh.exe` logs `ssh_agent_bind_hostkey: invalid format` but continues to list and use keys), closes the pipe with `UnexpectedEof` during binding, or closes it with `UnexpectedEof` on the immediately following identity-list request, reconnect and retry ordinary agent signing once without binding. Expose this as `legacy_unbound` in session metadata and warn that this session is not cryptographically bound to its destination; users must not rely on destination constraints in this compatibility mode. The unbound retry must still obtain identities successfully; other identity-list errors and all other bind errors fail closed. Certificate identities and keyboard-interactive multi-factor challenges remain explicit follow-up cases; do not silently downgrade or answer arbitrary prompts.

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

**Progress:** The native protocol layer, integrated SFTP backend, browser UI and transfer queue are implemented. Direct and multi-hop behavior has passed the Linux local-OpenSSH integration workflow; the merged Windows x64 validation and portable executable build also pass. The user manually confirmed that the integrated Windows SFTP workflow is working as expected on 22 November 2026. Remaining unchecked items are targeted compatibility, failure-injection and lifecycle/regression hardening.

- [x] Pin `russh 0.65.2` + `russh-sftp 3.0.1`, using the Ring crypto backend.
- [x] Finalize `SftpOptions.port` inheritance and backward compatibility for saved numeric port values.
- [x] Implement direct and multi-hop SSH authentication and SFTP subsystem setup.
- [x] Implement portable known-host checking, accept-new learning and changed-key rejection.
- [x] Implement direct password and private-key authentication.
- [x] Implement SFTP subsystem setup over `Channel::into_stream()`; direct and jump SFTP setup are covered by the Linux local-OpenSSH integration workflow.
- [x] Implement ordered multi-hop `direct-tcpip` channels and retain upstream handles; exercised by the Linux local-OpenSSH integration workflow.
- [x] Implement host-key TOFU, known-key match and changed-key rejection; unit tests added.
- [x] Implement Windows SSH-agent signing with session binding and a one-time legacy-unbound retry for unsupported/rejected binding, including EOF during the first identity-list request after binding. Live interoperability with the user's specific agent and Pageant-specific behavior remain acceptance checks.
- [ ] Verify hashed host entries and live non-default-port connections against Windows OpenSSH.
- [ ] Verify Pageant, passphrase-protected key and distinct credentials on each hop against live servers.
- [ ] Prove vault compatibility with an encrypted-vault fixture and older connection payloads.
- [x] Release gate: browser UI was introduced only after the direct/jump protocol and host-key implementation gates were satisfied.

**Security constraint:** use Russh 0.65.1 or later. Russh 0.65.2 is the selected baseline; keep the default no-op `kex_done` handler and do not expose or act on KEX shared-secret material. Versions before 0.65.1 are affected by GHSA-9264-jvgf-6w2f / CVE-2026-128087.

### M8b — SFTP backend and session lifecycle

**Progress:** The backend session registry and typed directory-listing contract are implemented. Sessions open from saved vault connections, including resolved jump chains, and expose runtime-only IDs and connection metadata (never credentials). Directory navigation canonicalizes remote POSIX paths and returns typed entries with type, size, modification time and permissions. Lifecycle cleanup closes sessions on explicit close, close-all, vault lock and app exit. A lifecycle gate prevents open/list/close races during vault-lock cleanup.

- [x] Add managed SFTP session state and typed session/directory/entry responses.
- [x] Add `sftp_open`, `sftp_list`, `sftp_close` and `sftp_close_all` commands.
- [x] Resolve saved connections and ordered jump chains in Rust; keep credentials out of frontend payloads.
- [x] Close SFTP sessions on vault lock independently of the M7 tunnel-lock preference.
- [x] Close SFTP sessions on application exit.
- [x] Add remote path validation and path-navigation unit tests.
- [x] User manually confirmed the core integrated Windows SFTP workflow is working as expected (22 November 2026).
- [ ] Extend Windows acceptance to explicitly cover vault locking during active sessions/transfers and the remaining authentication variants.
- [ ] Add Tauri-command integration coverage for open/list/close and vault-lock races; the local protocol-level direct/jump coverage is in M8e.

### M8c — Browser UI and file operations

- [x] Add SFTP entry points in the title bar and saved-connection context menu; open one browser for the selected connection.
- [x] Host each browser as a first-class tab in the shared SSH/SFTP pane hierarchy; keep its SFTP session and transfer queue mounted while switching or moving between panes.
- [x] Remove the nested SFTP session tab strip; opening another SFTP connection creates another top-level workspace tab, named after the connection.
- [x] Implement remote path navigation, breadcrumbs, listing, refresh and in-browser filtering.
- [x] Add context-specific local and remote create-folder, rename and delete actions inside their respective panes.
- [x] Add safe batch-delete commands for both panes with per-entry deleted/failed results; reject symlink traversal and only remove empty directories (never recursively delete folders).
- [x] Add a side-by-side local/remote file manager with visible active-pane highlighting, local drive/path navigation, metadata listing, filtering and independent selection.
- [x] Support plain click, Ctrl-click multi-selection, Shift-click range selection and Ctrl+Shift additive range selection in both panes.
- [x] Support drag-and-drop and toolbar upload/download through the existing transfer queue; use the current local pane directory as the download destination.
- [x] Add a fixed-default, pointer-resizable transfer queue with keyboard-accessible height controls.
- [x] Add keyboard navigation, F2 rename, Delete confirmation, focus management and nested-dialog focus traps.
- [x] Core Windows browser and transfer workflow, including mixed SSH/SFTP pane tabs and the single-level SFTP tab hierarchy, manually accepted by the user (22 November 2026).
- [ ] Complete the expanded visual/lifecycle matrix across all seven themes, minimum window size and vault lock while sessions/transfers are active.

### M8d — Transfer queue (implemented)

- [x] Stream single/multi-file and recursive folder upload/download in bounded 256 KiB chunks; cap a transfer plan at 100,000 entries.
- [x] Build and validate the complete transfer plan before starting; preserve relative folder paths and reject symbolic links, unsupported names and duplicate destination mappings.
- [x] Add per-transfer progress events, bytes/files totals, current-file status and independent cancellation.
- [x] Detect destination conflicts before starting and require explicit overwrite confirmation.
- [x] Stage each destination file beside its final path and rename only after successful close/flush; restore the prior destination if overwrite commit fails.
- [x] Remove incomplete temporary files on cancellation and transfer failures; cancel and reap jobs before closing a session or locking the vault.
- [x] Resolve each transfer's local source/destination from the active local pane at launch, keeping the selected paths and destination explicit in the queued job.

### M8e — Hardening and release gate

- [x] Windows x64 portable executable build, frontend checks, Rust formatting and Rust tests in GitHub Actions on merged `main` commit `6b14f75220d1b92d36f643404b1a91bd7997ef89` (workflow [#45046483858](https://github.com/rhurta/sshterm/actions/runs/45046483858)).
- [x] Linux local-OpenSSH integration test for direct and multi-hop access, recursive upload/download, empty/large/Unicode/space-containing files, overwrite, cancellation cleanup and remote batch deletion on merged `main` commit `6b14f75220d1b92d36f643404b1a91bd7997ef89` (workflow [#45046483917](https://github.com/rhurta/sshterm/actions/runs/45046483917)).
- [x] Validate Windows-safe download names and duplicate destination rejection before copying.
- [x] Cover remote batch-delete partial failures, duplicate selection and empty-folder removal in the local OpenSSH integration test.
- [x] Core integrated browser/transfer workflow manually accepted on Windows by the user (22 November 2026).
- [x] Windows validation and portable build for the mixed SSH/SFTP pane implementation (PR workflow [#45069307061](https://github.com/rhurta/sshterm/actions/runs/45069307061)).
- [x] Linux local-OpenSSH SFTP integration for the mixed-pane implementation (PR workflow [#45069307123](https://github.com/rhurta/sshterm/actions/runs/45069307123)).
- [ ] Complete expanded Windows checks across all seven themes, minimum window size and vault-lock lifecycle.
- [ ] Add Tauri-command lifecycle/race coverage for open/list/close, vault lock and application exit.
- [ ] Add explicit integration cases for permission-denied destinations, remote disk full, network interruption and disconnect during a transfer.
- [ ] Re-run the M7 Windows smoke checklist against the final merged build.

## 6. Explicit non-goals

M8 does not replace the terminal engine, add native terminal sessions, rewrite M7 tunnels, add file synchronization/watch, remote editing, SCP/rsync, permission/chmod management, or recursive destructive directory deletion. Those require separate scope; terminal and native transport parity remains M10.

## 7. Technical references checked on 20 November 2026

- [Russh 0.65.2 documentation](https://docs.rs/russh/0.65.2/russh/) — async SSH client, `connect_stream`, authentication, direct TCP/IP channels, crypto backend features and Pageant support.
- [russh-sftp 3.0.1 documentation](https://docs.rs/russh-sftp/3.0.1/russh_sftp/) — SFTP v3 client and server.
- [SftpSession API](https://docs.rs/russh-sftp/3.0.1/russh_sftp/client/struct.SftpSession.html) — high-level filesystem-like async methods and stream-backed file handles.
- [Russh SFTP client example](https://github.com/AspectUnk/russh-sftp/blob/master/examples/client.rs) — open an SSH session channel, request the `sftp` subsystem and wrap the channel stream.
- [Russh known-hosts implementation](https://docs.rs/russh/0.65.2/src/russh/keys/known_hosts.rs.html) — custom known-hosts path, hashed host matching and non-default port representation.
- [GHSA-9264-jvgf-6w2f](https://github.com/Eugeny/russh/security/advisories/GHSA-9264-jvgf-6w2f) — minimum safe baseline is Russh 0.65.1; the proposed 0.65.2 baseline is above that floor.

