# SSHTerm

A portable, modular SSH terminal for Windows 11, built with Tauri and Rust.
One executable plus data files next to it — no installer, no registry, no `%APPDATA%`.

## Goals

- **Portable** — resolve every path from the exe's directory; runs from a USB drive.
- **Modular** — each feature is a separate Rust module and a separate UI component, built and tested in steps.
- **Secure by default** — every secret lives in one encrypted vault opened with a master password.
- **Phased SSH engine** — phase 1 uses Windows' built-in `ssh.exe` inside a ConPTY the app owns; phase 2 adds a native `russh` engine. The UI and all other modules stay the same between phases.

## Status

| Milestone | What it delivers | Status |
|-----------|-----------------|--------|
| M0 | Portable skeleton, WebView2 data folder | ✅ Complete |
| M1 | Encrypted vault, i18n | ✅ Complete |
| M2 | First SSH session over ConPTY + xterm.js | ✅ Complete |
| M3 | Per-session log files | ✅ Complete |
| M4 | Session tree (folders, CRUD, search, import, drag-and-drop) | ✅ Complete |
| M5 | Jump hosts (`-J` chain, per-hop passwords) | ✅ Complete |
| M6 | Tiling, broadcast, syntax highlighting | 📋 Planned |
| M7 | Local and reverse TCP forwarding (`-L` / `-R`) | 🟡 Partial |
| M8 | Native SFTP (`russh-sftp`) | 📋 Planned |
| M9 | Polish (colors, host-key UI, auto-reconnect) | 📋 Planned |
| M10 | Native `russh` engine replaces phase 1 | 📋 Planned |

## Portable layout

```
sshterm.exe
sshterm.vault      encrypted: connections, passwords, tunnels, colors
config.toml        plain: language, log settings, font, keepalive defaults
known_hosts        SSH host-key fingerprints (used by ssh.exe in phase 1)
highlight/         *.toml syntax-highlighting rule files
logs/              <connection>_<YYYYMMDD-HHMMSS>.log  per-session logs
webview-data/      WebView2 cache (WEBVIEW2_USER_DATA_FOLDER)
```

## Technology stack

| Layer | Choice |
|-------|--------|
| Shell | Tauri 2 |
| Backend | Rust + tokio |
| SSH phase 1 | `ssh.exe` (Windows OpenSSH) through `portable-pty` (ConPTY) |
| SSH phase 2 | `russh` + `russh-keys` |
| SFTP | `russh-sftp` (M8) |
| Crypto | `argon2`, `chacha20poly1305`, `zeroize` |
| Frontend | TypeScript + Svelte 5 |
| Terminal | `xterm.js` + FitAddon |
| Config | `serde` + `toml` |

## Project structure

```
sshterm/
├── .github/
│   └── workflows/
│       └── windows.yml          # Windows validation and portable build
├── src/                         # Frontend (SvelteKit)
│   ├── lib/
│   │   ├── ConnectionTree.svelte   # Recursive folder/connection tree
│   │   ├── ConnectionSidebar.svelte
│   │   ├── SearchBox.svelte / ImportDialog.svelte
│   │   ├── i18n/                 # Translation dictionaries (en, es)
│   │   ├── encoding.ts           # Base64 helpers
│   │   ├── logCapture.ts         # Screen-buffer log capture
│   │   └── sessionBus.ts         # session:data / session:closed event bus
│   └── routes/                   # SvelteKit pages
└── src-tauri/                    # Backend (Rust)
    └── src/
        ├── vault/                # Encrypt / decrypt credential store
        ├── store/                # Connection + tree CRUD, search, imports
        │   ├── tree.rs           # Pure tree algorithms and move validation
        │   └── import/           # ssh config, PuTTY, WinSSHTerm parsers
        ├── session/              # Open / close / resize sessions
        ├── transport/            # Transport trait + openssh.rs (+ russh.rs in M10)
        ├── askpass/               # Named-pipe password delivery to ssh.exe
        ├── logger/                # Per-session log files
        ├── config.rs              # config.toml model
        └── paths.rs               # Portable path resolution
```

## Development

### Prerequisites

- Rust (stable, via rustup)
- Node.js 22 and npm
- Windows 11 — WebView2 and OpenSSH are pre-installed
- Visual Studio Build Tools (MSVC toolchain)

### Run in dev mode

```bash
npm ci
npm run tauri dev
```

### Production build

```bash
npm run tauri build
# exe: src-tauri/target/release/sshterm.exe
# MSI: src-tauri/target/release/bundle/msi/
```

### Tests and validation

Run from the repository root:

```bash
npm ci
npm run check
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --all
cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check
cargo test --manifest-path src-tauri/Cargo.toml --locked
npm run tauri -- build --no-bundle
```

The first `cargo fmt` command normalizes the Rust source before checking it. This matches the current CI workflow, which avoids committing unrelated repository-wide formatting changes while still ensuring the checked-out Rust code is formatted.

### Windows CI

The `.github/workflows/windows.yml` workflow runs on pushes to `main`, `feature/**` and `fix/**`, on pull requests, and when manually dispatched. It installs Node.js 22 and stable Rust for `x86_64-pc-windows-msvc`, validates the Svelte frontend, formats and tests the Rust backend, and builds the portable Windows executable.

A successful run publishes the `sshterm-windows-x64` artifact containing `sshterm.exe`. Download it from the workflow run's **Artifacts** section on GitHub Actions. The artifact is retained for 30 days.

### Windows HTML drag-and-drop requirement

The connection tree uses browser HTML5 drag-and-drop events. Tauri's own webview drag-and-drop handler intercepts those events when enabled, so the frontend tree cannot receive them. On Windows, disable Tauri's handler in each window configuration in `src-tauri/tauri.conf.json`:

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

Keep `dragDropEnabled: false` for the main window when using the frontend's HTML5 drag-and-drop implementation. This disables Tauri's native `DragDropEvent` handling for that webview; it does not disable the tree's HTML drag-and-drop. If native file-drop events are needed in the future, the app will need to handle those through Tauri's native event API instead of relying on browser drag events. See the [Tauri window configuration reference](https://v2.tauri.app/reference/config/#windowconfig).

## Architecture notes

### Vault

- Format: `SSHV` magic + Argon2id KDF params + 16-byte salt + 24-byte nonce + XChaCha20-Poly1305 ciphertext (header authenticated as AAD).
- KDF defaults: 64 MiB, 3 iterations, 1 lane. Parameters are stored in the header so they can be raised later.
- Atomic saves: write to `.tmp`, flush, rename. One `.bak` copy kept.
- Secrets held in `zeroize`-wrapped memory; dropped on lock.
- Password change re-encrypts with a new salt and nonce.

### Transport trait (phase 1 / phase 2 boundary)

```rust
trait Transport {
    async fn open(&mut self, conn: &Connection, size: PtySize) -> Result<()>;
    async fn write(&mut self, data: &[u8]) -> Result<()>;
    async fn resize(&mut self, size: PtySize) -> Result<()>;
    async fn close(&mut self) -> Result<()>;
    fn output(&mut self) -> Option<Receiver<Vec<u8>>>;
}
```

Session code calls this trait and never references `ssh.exe` or `russh` directly. A per-connection `engine` field (`openssh` | `russh`) selects the implementation, so both can run side by side.

### Password delivery (phase 1)

`ssh.exe` is started with `SSH_ASKPASS` pointing to the app's own exe (run with `--askpass`) and `SSH_ASKPASS_REQUIRE=force`. The helper connects to a named pipe, presents a one-time token, receives the password for the host in ssh's prompt, and writes it to stdout. The password never appears on a command line or in an environment variable.

### Logging (M3)

ConPTY does not forward raw remote bytes — it sends screen redraws (full repaints on resize, partial updates on scroll). Stripping escape codes from that stream creates duplicate lines and fragments. The fix: log from the xterm.js screen buffer instead.

```
PTY redraws ──▶ xterm.js ──▶ screen buffer (each line once)
                                    │
                              LogCapture.ts
                              (complete lines, 250 ms batch)
                                    │
                          session_log_append (IPC)
                                    │
                            SessionLogger (Rust)
                            write_text() / write_raw()
                                    │
                      logs/<connection>_<YYYYMMDD-HHMMSS>.log
```

`log_mode = "screen"` (default) captures clean readable text.
`log_mode = "raw"` stores PTY bytes with escape codes, for diagnostics.

### Session tree (M4)

The vault payload stays at `version: 1` and the file format is unchanged. `tree`
is a nested `Vec<TreeNode>` of folders and connection references; every
connection id appears exactly once in the tree. M1–M3 vaults are migrated on
unlock: the flat list is wrapped in one **Connections** folder and the old
`password` field moves into `auth`.

New commands: `tree_get`, `folder_upsert`, `folder_delete`, `move_node`,
`search`, `import_preview`, `import_apply`. Imports are read-only until the user
confirms the preview; duplicates on `(host, port, user)` are skipped unless
"import as copy" is selected.

Tree drag-and-drop supports:
- Moving a connection or folder before/after another node.
- Dropping a node into a folder, including nested folders.
- Reordering nodes within the same parent and moving nodes to the root.
- Rejecting invalid destinations, including dropping a folder into itself or one of its descendants.
- Disabling tree drag-and-drop while a search filter is active, because filtered display indices do not safely map to the underlying tree.

The Rust tree move operation validates the destination before removing the source node. A rejected move therefore leaves the original tree and node position intact. Regression coverage includes this rejected-move behavior.

WinSSHTerm exports represent folders as nested `<Node Type="Container">` elements. Each candidate carries its full folder path, and the import recreates that hierarchy in the vault tree (reusing folders that already exist), so the source folder structure is preserved. WinSSHTerm passwords are encrypted with WinSSHTerm's own key and are never imported; those connections are created with agent auth and a warning, and must be edited to set credentials. Port forwards, login commands and jump hosts from the export are not imported.

Each tab owns an independent session. Opening a session or switching tabs focuses the xterm pane so typing reaches the remote host immediately.

Scrollbars are styled thin (6px) and matched to the dark theme across all scrollable areas (sidebar, dialogs, terminal viewport). The connection picker opened from the `+` tab button can be dismissed with Escape, the × button, or click-outside.

### Local and reverse SSH tunnels (M7, partial)

The connection editor can store local (`-L`) and reverse (`-R`) TCP forwards. Each forward has a listen endpoint and a destination endpoint and its own enable/disable switch. Disabled forwards remain saved but are omitted from the SSH command line. Both accept `host:port`; a listen endpoint may also be a bare port (OpenSSH's loopback default). IPv6 addresses must be bracketed, for example `[::1]:5432`.

Local forwarding listens on the client machine and connects to the destination from the SSH server. Reverse forwarding listens on the SSH server and connects to the destination from the client machine. Forwards are attached to the connection's interactive SSH process: they start with the terminal session and close when that session closes. Multiple forwards are passed as separate `-L` and `-R` arguments. `ExitOnForwardFailure=yes` makes OpenSSH terminate the connection if it cannot establish a requested listener (for example, a port is already in use). For a reverse listener to accept connections from machines other than the SSH server itself, the server's `sshd` configuration must permit it with `GatewayPorts`.

The session tab shows the number of enabled forwards. This is a configuration indicator, not a live health check. SSH diagnostics for forwarding and connection failures are also surfaced in the workspace error banner while the original output remains visible in the terminal. Standalone tunnel-only sessions, start/stop controls independent of the terminal, structured active/failed status, and dynamic SOCKS forwarding (`-D`) remain future work.

### Jump hosts (M5)

Each connection can reference an ordered list of saved jump-host connection IDs. The connection editor lets the user add, reorder, and remove hops; the session tab shows the resolved route. The backend expands nested jump definitions into an outermost-to-target chain, validates missing IDs, duplicates and cycles, and passes the chain to Windows OpenSSH using `-J`. Each configured hop can be disabled without deleting it; disabled hops remain saved but are omitted from the active route. The jump-host picker has a search box for connection name, host, or user.

The session's secured askpass pipe holds only the credentials needed for its active route. The app also reports recognized SSH connection and jump-host errors in the workspace error banner. OpenSSH prompt text is matched to the exact `user@host` (or configured key path for the destination), and each matching credential is delivered at most once. This supports different saved passwords for different hops without putting passwords in arguments, environment variables, or files. A generic `Password:` prompt is answered only when there is a single credential, so ambiguous prompts fail closed.

Current phase-1 limitations: a jump host that uses a saved custom private-key path is rejected with a clear error; use password or SSH-agent authentication for jump hops. Every hop and the target must have a distinct `user@host` identity because OpenSSH prompts omit the port; this avoids assigning a password to the wrong route member. Native per-hop key configuration and chained `direct-tcpip` channels remain part of M10.

## Configuration

`config.toml` (next to the exe):

```toml
language   = "en"       # "en" | "es"
log_output = true
log_mode   = "screen"   # "screen" | "raw"
```

## Security notes

- No secrets in logs, `config.toml`, or crash output.
- The named askpass pipe is restricted to the current user; a session-scoped token authorizes requests, and each matched hop credential is returned at most once.
- `ssh.exe` child processes are killed when the app exits or the vault locks.
- Private key files are stored by path only (the vault keeps the path, not the key content).
- Host keys: trust-on-first-use; a changed key triggers a hard warning. The custom `UserKnownHostsFile` value is quoted so install paths containing spaces are interpreted as a single file path.
- Clipboard cleared shortly after copying a password.
