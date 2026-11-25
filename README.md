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
| M6 | Tiling, command console, broadcast and refined workspace panels | ✅ Complete; documented in [workspace panel guide](docs/workspace-panels.md) |
| M7 | Local, reverse and dynamic SOCKS forwarding, independent lifecycle and manager UI | ✅ Implemented; Windows CI passed |
| M8 | Native SFTP (`russh` + `russh-sftp`) | ✅ Implemented; mixed SSH/SFTP panes manually accepted on Windows; hardening follow-ups tracked; [implementation plan](docs/M8-implementation-plan.md) |
| M9 | Polish (per-connection colors, host-key UI, auto-reconnect) | 🟡 Partial; global themes implemented |
| M10 | Native `russh` engine replaces phase 1 | 📋 Planned |

M6 implementation details and acceptance criteria: [docs/M6-implementation-plan.md](docs/M6-implementation-plan.md). Syntax highlighting remains tracked separately as M6b. Workspace panel visibility, connection properties and shared typography/header rules are documented in [docs/workspace-panels.md](docs/workspace-panels.md).

### M6 delivered

- Recursive workspace tiling with single, left/right, top/bottom and 2×2 layouts, draggable and keyboard-adjustable dividers, per-pane session tabs, empty-pane handling, and an active-pane outline/dot.
- A bottom command console with a searchable left-side connection list, Active / Selected / All targeting, protected-session opt-in, multiline and multi-session confirmation, exact-byte sequential dispatch, and per-session failure reporting. Shift+Enter sends only the line at the caret and restores editor focus; Ctrl+Enter sends the complete draft.
- Console visibility and height persist in non-secret `config.toml`; drafts remain in frontend memory only.
- The connection tree and selected-connection properties pane have independently persisted visibility. The properties pane presents the selected connection in a compact property grid and supports inline editing of the complete connection configuration; secret values are never displayed in plaintext. See [`docs/workspace-panels.md`](docs/workspace-panels.md).
- Appearance: seven themes (dark, light, Nord and the four Catppuccin flavors), an optional background image or see-through window (Transparent or Mica on Windows 11) with adjustable interface and terminal opacity, fixed-size proportional dialogs, a searchable settings dialog, and a show/hide eye on password fields. See [`docs/appearance.md`](docs/appearance.md). The window opens with one empty pane.
- Scripts library: the command console has Save script… and Load script… buttons. Scripts are stored in folders in a second encrypted file, `scripts.vault`, opened with the same master password. Scripts marked as favorites get a one-click button below the console. See [`docs/scripts.md`](docs/scripts.md).
- Keyboard shortcuts: Ctrl+Alt+K toggles the console; Ctrl+Alt+1–4 select the four layout presets; Ctrl+Alt+Arrows focus the neighboring pane (Ctrl+Alt+, / Ctrl+Alt+. cycle panes, for keyboards where Windows takes the arrows) and Ctrl+Alt+Shift+Arrows move the active tab there; Shift+Enter sends the current command-console line; Ctrl+Enter sends the draft.
- SSH and SFTP workspace tabs share the same pane hierarchy. Tabs move between panes without reconnecting: drag onto another pane's tab strip (or between tabs to reorder), onto a pane's center to move there, or onto an edge to split and move into the new pane. Each SFTP tab represents one connection and has no nested tab strip; moving it preserves the live SFTP session and transfer queue. See [`docs/tab-moves.md`](docs/tab-moves.md).
- Connections can be dragged from the tree onto a pane or onto its edges (which split it); a new empty pane opens the connection list straight away; an empty pane can be focused (click or Ctrl+Alt+Arrows) so the next connection opened from the tree lands in it. See [`docs/pane-targets.md`](docs/pane-targets.md).
- Automated coverage includes pure split-tree and broadcast-target resolution tests plus Rust config tests. Component-level UI automation and live multi-host dispatch integration tests remain follow-up work.

## Portable layout

```
sshterm.exe
sshterm.vault      encrypted: connections, passwords, tunnels, colors
scripts.vault      encrypted: saved command scripts (created on first unlock)
config.toml        plain: language, theme, logging, terminal and application-behavior preferences, console layout
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
│   │   ├── ConnectionTree.svelte / ConnectionSidebar.svelte
│   │   ├── ConnectionEditor.svelte / ImportDialog.svelte / ExportDialog.svelte / SearchBox.svelte
│   │   ├── Workspace.svelte / TerminalPane.svelte / CommandConsole.svelte
│   │   ├── splitTree.ts          # Recursive tiling model and geometry helpers
│   │   ├── broadcastTargets.ts   # Credential-free broadcast target resolution
│   │   ├── i18n/                 # Translation dictionaries (en, es)
│   │   └── encoding.ts / logCapture.ts / sessionBus.ts
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

`ssh.exe` is started with `SSH_ASKPASS` pointing to the app's own exe (run with `--askpass`) and `SSH_ASKPASS_REQUIRE=force`. The helper connects to a named pipe, presents a session-scoped random token, receives the password for the host identified by ssh's prompt, and writes it to stdout. Each route credential is delivered at most once. The password never appears on a command line or in an environment variable.

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
`search`, `import_preview`, `import_apply`, `conn_export`. Imports are read-only until
the user confirms the preview; duplicates on `(host, port, user)` are skipped unless
"import as copy" is selected. Export writes the complete decrypted vault payload to
plaintext JSON after an explicit warning; it includes stored credentials and should be
kept secure.

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

### SSH tunnels (M7 — implemented)

The connection editor stores local (`-L`), reverse (`-R`) and dynamic SOCKS (`-D`) forwards per connection. Each forward can be enabled independently. Listen endpoints accept `[address:]port`; destinations require `host:port`. IPv6 literals use brackets, for example `[::1]:5432`.

Interactive terminal sessions start their configured forwards with the SSH session. The independent Tunnel Manager instead runs one non-interactive OpenSSH `ssh -N -T` process per saved connection, grouping all enabled forwards and reusing its saved authentication and ProxyJump route. It does not require a terminal tab. Password-authenticated target and jump connections need their passwords saved in the encrypted vault.

The Tunnel Manager is available from the connection sidebar. It lists the saved route, listener endpoints and per-forward status/errors, and offers Start/Stop for one connection plus Start All and Stop All. Its search field receives focus automatically. The title bar shows a live green-dot/count indicator for tunnels that are starting, active or stopping; selecting it opens the manager while the vault is unlocked.

**Active confirms listener setup only.** It does not probe the remote destination service or prove end-to-end traffic. A failed connection to the forwarded destination does not, by itself, mean the listener has failed. Reverse listeners may require server-side `GatewayPorts` configuration.

Application behavior preferences are stored outside the encrypted vault in `config.toml`:

- `stop_tunnels_on_lock` defaults to true. When disabled, independent tunnel processes and their authentication helper remain active in memory while the vault is locked.
- `close_to_tray` defaults to false. When enabled, closing the window hides it to the Windows system tray; the tray menu or a double-click restores the window. Choosing Exit still shuts down all tunnel process trees.

Windows lifecycle hardening keeps process termination off the WebView UI thread, terminates ProxyJump descendants with `taskkill /T /F`, reaps the owned child, and bounds monitor cleanup so inherited stderr pipes cannot stall tunnel shutdown.

**Verification:** Windows workflow [#448012](https://github.com/rhurta/sshterm/actions/runs/44801295593) passed frontend/Svelte checks, Rust formatting and tests, and the portable Windows executable build. The user confirmed the post-hardening application works as expected; a final UI smoke test of the newer tray preferences and title-bar indicator remains recommended.

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

[app_behavior]
stop_tunnels_on_lock = true
close_to_tray = false
```

## Security notes

- No secrets in logs, `config.toml`, or crash output.
- The named askpass pipe is restricted to the current user; a session-scoped token authorizes requests, and each matched hop credential is returned at most once.
- Interactive SSH sessions are closed on vault lock. Independent tunnel processes stop on vault lock by default; the non-secret application preference can keep them running while locked. All SSH process trees are stopped on explicit application exit.
- Private key files are stored by path only (the vault keeps the path, not the key content).
- Host keys: trust-on-first-use; a changed key triggers a hard warning. The custom `UserKnownHostsFile` value is quoted so install paths containing spaces are interpreted as a single file path.
- Clipboard cleared shortly after copying a password.
