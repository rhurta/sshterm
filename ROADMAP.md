# SSHTerm – Roadmap

Milestones from the project spec, updated with what has actually been built.
Each milestone ends with a working exe you can use from the field.

---

## ✅ M0 — Portable skeleton

**Done when:** Tauri app builds to one exe; portable paths and WebView2 data folder work.

- Tauri 2 + SvelteKit + TypeScript project
- `paths.rs` resolves every path from the exe's directory
- `WEBVIEW2_USER_DATA_FOLDER` set before the window opens so nothing is written outside the folder
- `ssh.exe` lookup: `bin/` next to the exe first, then `System32`

**Commit:** `bd4d6b4` (tag: m1 — M0 and M1 landed together)

---

## ✅ M1 — Vault

**Done when:** Create, lock, unlock, change password; unit tests for wrong password and corruption.

### Vault format

| Field | Size | Content |
|-------|------|---------|
| Magic | 4 B | `SSHV` |
| Version | 1 B | `1` |
| KDF params | 12 B | Argon2id memory (KiB), iterations, parallelism |
| Salt | 16 B | random per vault |
| Nonce | 24 B | random per save |
| Ciphertext | rest | XChaCha20-Poly1305 (header is AAD) |

- KDF defaults: Argon2id 64 MiB / 3 iterations / 1 lane.
- Atomic saves: `.tmp` → flush → rename; one `.bak` copy kept.
- Secrets in `zeroize`-wrapped memory; dropped on lock.
- Password change: new salt + nonce, full re-encrypt.

### Connection model (current — flat list)

```rust
struct Connection {
    id, name, host: String,
    port: u16,
    user: String,
    password: Option<String>,
    keepalive_secs: u16,
}
```

The vault payload will gain folders, jump chains, tunnels, and colors when M4 adds the session tree.

### i18n

- Languages: English, Spanish.
- Reactive switching; preference stored in `config.toml`.

**Commit:** `bd4d6b4` (tag: m1)

---

## ✅ M2 — First session on `ssh.exe`

**Done when:** `Transport` trait + `OpenSshTransport` over ConPTY; xterm.js terminal, resize, keepalive flags, password through askpass from the vault.

### Transport trait

```rust
trait Transport {
    async fn open(&mut self, cfg: &SshConfig, size: PtySize) -> Result<()>;
    async fn write(&mut self, data: &[u8]) -> Result<()>;
    async fn resize(&mut self, size: PtySize) -> Result<()>;
    async fn close(&mut self) -> Result<()>;
    fn output(&mut self) -> Option<Receiver<Vec<u8>>>;
}
```

### `OpenSshTransport`

- `ssh.exe` flags: `-tt`, `-p`, `UserKnownHostsFile=`, `StrictHostKeyChecking=accept-new`, `ServerAliveInterval`, `ServerAliveCountMax=3`, `ConnectTimeout=15`.
- PTY: `portable-pty` (ConPTY on Windows), 8 KB read buffer, dedicated reader thread.
- Password: `SSH_ASKPASS` + `SSH_ASKPASS_REQUIRE=force` + one-time named-pipe token. Password never on a command line.
- EOF watchdog: polls `child.try_wait()` every 100 ms; on exit gives ConPTY 150 ms to flush, waits up to 2 s for the reader thread, then sends an empty sentinel chunk.

### Frontend

- `xterm.js` canvas renderer + FitAddon.
- Resize debounced 80 ms; only the active tab resizes the PTY.
- Input and output over base64-encoded IPC events.

**Commit:** `2c19e40` (tag: m2)

---

## ✅ M3 — Logging

**Done when:** one log file per session with the correct name.

### The problem with the first implementation

The initial approach stripped ANSI codes from PTY bytes. ConPTY does not forward raw remote bytes — it maintains its own screen and sends redraws (full repaints on resize/scroll, partial updates with cursor jumps). Stripping those codes removed the cursor-positioning context that explained the redraws, producing repeated MOTD blocks, random number fragments, and garbage.

### The fix: log from the xterm.js screen buffer

xterm.js applies every redraw correctly, so its buffer always holds each line exactly once. `LogCapture.ts` monitors that buffer and sends complete lines to the backend only after the cursor moves past them.

```
PTY redraws ──▶ xterm.js ──▶ buffer (each line once)
                                  │
                            LogCapture.ts
                            • marker tracking (survives scroll/reflow)
                            • screen-erase detection (ESC[J, RIS)
                            • repaint dedup via recent-line history
                            • alternate-screen skip (vim, less, top)
                            • blank-line collapsing (max 1 in a row)
                            • 250 ms flush batch
                                  │ session_log_append / session_log_close
                            SessionLogger (Rust)
                            • write_text()  ← Screen mode (default)
                            • write_raw()   ← Raw mode (diagnostics)
                            • create_new collision: adds _2, _3 …
                                  │
                    logs/<connection>_<YYYYMMDD-HHMMSS>.log
```

### Configuration

```toml
log_output = true       # false → no file created
log_mode   = "screen"   # "screen" | "raw"
```

### What is logged (screen mode)

- One copy of each line the user saw, trailing whitespace trimmed.
- Blank lines collapsed to at most one.
- Alternate-screen content (vim, less, htop) excluded.
- Progress bars and `\r` overwrites never reach the file.
- Closing line: `---- ended: <RFC 3339 timestamp>`.

**Commits:** `952e5cd` (tag: m3, initial) → `248b264` (blank-line fix) → `bbc0262` (screen-buffer capture, current)

---

## ✅ M4 — Session tree

**Done when:** folders, CRUD, search, import from PuTTY / WinSSHTerm / `~/.ssh/config`, and multi-tab workspace.

### What changed

The spec's M4 is primarily about the **session tree** (sidebar with folders, drag-and-drop, search, import). The multi-tab workspace already shipped with M3 (`Workspace.svelte` kept one `TerminalPane` per session, hidden but still running), so M4 only had to verify it and add terminal focus handling. Per-connection colors were **deferred to M9** (spec §8/§11 and ROADMAP M9 both assign them there; the M4 checklist below no longer claims them).

The vault payload stays at `version: 1` with the same on-disk file format. `tree` and `connections` are now typed; old flat vaults are migrated in memory on unlock.

### Vault data model additions

```json
{
  "tree": [
    { "type": "folder", "id": "f1", "name": "Production", "children": [
      { "type": "connection", "id": "c1" }
    ]}
  ],
  "connections": {
    "c1": {
      "name": "pgw-01", "host": "10.0.0.5", "port": 22,
      "user": "admin",
      "auth": { "type": "password", "password": "…" },
      "keepalive_secs": 30,
      "colors": { "fg": "#d0d0d0", "bg": "#1b1b1b" },
      "protected": false
    }
  }
}
```

### Frontend components

```
ConnectionSidebar.svelte
├── SearchBox.svelte                 (debounced search, match count)
├── ConnectionTree.svelte            (recursive snippet; folders + connections)
│   ├── inline rename (F2 / double-click)
│   ├── context menu  (open, edit, duplicate, new, rename, delete)
│   └── HTML5 drag-and-drop  → move_node
└── ImportDialog.svelte              (source → preview → apply)

ConnectionEditor.svelte   General / Authentication / Advanced
Workspace.svelte          TabBar [● pgw-01 ×] [server2 ×] [+]
└── TerminalPane.svelte[]  (one per tab, hidden when inactive, focused on activate)
```

### Multi-tab behaviour

- Each tab = one independent SSH session (unique session ID, separate PTY, separate log file).
- Hidden tabs keep running; only the active one triggers PTY resize.
- `SessionState` already holds a `HashMap<SessionId, Transport>` — no backend changes needed for multi-tab.
- Last tab closed → show connection selector / sidebar.

### Backend additions

New Tauri commands for tree and connection management:

| Command | Description |
|---------|-------------|
| `tree_get` | Return full typed tree |
| `conn_list`, `conn_get` | Summaries and full connection (secrets only while unlocked) |
| `conn_upsert`, `conn_delete` | Create/update/delete; tree is reconciled automatically |
| `folder_upsert` | Create (`id: null`) or rename a folder |
| `folder_delete` | Delete folder, `cascade` controls whether children go too |
| `move_node` | Move/reorder; rejects cycles, keeps one parent per node |
| `search` | Case-insensitive name / host / user, returns folder breadcrumbs |
| `import_preview` | Read-only parse of one source into candidates + skips + warnings |
| `import_apply` | Write with `skip` (default) or `copy` duplicate policy |

Sources: `ssh_config` (`~/.ssh/config`), `putty` (HKCU registry, Windows only, `winreg`), `winsshterm` (`connections.xml`, schema-tolerant parser). Saved passwords are never imported (WinSSHTerm encrypts them with its own key, so a warning is shown and the connection is created with agent auth for the user to edit afterwards).

WinSSHTerm exports use one `<Node>` tag for everything, discriminated by `Type="Connection"` / `Type="Container"`, with containers nested recursively. The parser records the full ancestor path on each candidate and `import_apply` recreates it with `tree::ensure_path`, reusing same-named folders so repeated imports do not duplicate the hierarchy. Port-forward arguments, login commands and jump-host settings in the export are not imported (M5/M6 scope).

### Implementation checklist

- [x] Extend vault data model (typed tree, `auth`, `protected`, forward fields for M5–M8)
- [x] Migration of M1–M3 flat vaults on unlock (no format bump)
- [x] `store` module: tree CRUD + search (`store/tree.rs` is pure and unit-tested)
- [x] Import parsers (`~/.ssh/config`, PuTTY registry, WinSSHTerm), including recreation of the WinSSHTerm folder hierarchy
- [x] Sidebar component (tree, drag-and-drop, debounced search, context menu, inline rename)
- [x] Connection editor (General / Authentication / Advanced; `auth.type = key` passes `-i` to ssh.exe)
- [x] Multi-tab workspace verification (already in M3) + terminal focus on open/switch
- [x] Thin theme-matched scrollbars; dismissable connection picker (Escape, ×, click-outside)
- [ ] Per-connection colors applied to terminal theme and tab accent → **M9** (spec §8/§11)

---

## ✅ M5 — Jump hosts

**Done when:** the OpenSSH `-J` chain is built from saved connections and can authenticate each password-protected hop with its own credential.

### Backend

- The vault's existing ordered `jump: ["conn_id", …]` field is now active; no vault format or payload-version migration is needed.
- Per-hop enable/disable state is stored separately in `jump_disabled`; older vaults default every configured hop to enabled.
- The editor can search saved connections by name, host, or user when selecting a jump hop.
- `store::resolve_jump_chain` expands nested chains into outermost-to-target order and rejects missing references, repeated hops, cycles and routes deeper than 16 hops.
- Sessions with jump hosts run `ssh -F <temp config> sshterm-target`. The generated config has one `Host` block per hop (`HostName/Port/User`, each hop with `ProxyJump` to the previous one) and a shared `Host sshterm-*` block with our `UserKnownHostsFile`, `StrictHostKeyChecking accept-new`, keepalive and timeout. (Originally one `-J` argument was used; OpenSSH forwards only `-F`, not `-o`, to the hop processes, so the first hop failed with `Host key verification failed`. Fixed on `fix/m5-jump-config`.) The temp file holds no secrets and is removed on session close. While it is in use, `~/.ssh/config` is not read for that session.
- The askpass pipe matches password prompts against `user@host` and destination key-passphrase prompts against the configured identity-file path. Credentials are held in `Zeroizing<String>` and each match is served at most once.
- The session-scoped pipe token is retained for the session so multiple hops can authenticate. A generic `Password:` prompt is accepted only when the session has exactly one credential; unknown or ambiguous prompts receive no password.
- Duplicate `user@host` identities anywhere in the route are rejected because the OpenSSH prompt omits the port and cannot disambiguate them.
- Jump hosts configured with a saved custom private-key path are rejected in the phase-1 engine. Password and SSH-agent authentication are supported for jump hops; per-hop custom key selection is deferred until a transport can configure each hop independently.

### UI

- The connection editor has an ordered jump-host section with add, remove and move-up/down controls.
- The editor checks for loops before saving; backend validation remains authoritative.
- Session tabs show the expanded route as `hop1 → hop2 → target`; the terminal startup banner shows the same route.

### Tests

- Unit tests cover ordered/nested chain expansion, missing/duplicate/cyclic references, ProxyJump argument ordering, IPv6 formatting, prompt-to-host credential mapping, key-passphrase path matching and ambiguous generic prompts.
- Windows CI validates the frontend, formats and tests Rust, and builds the portable executable. Live multi-hop authentication still requires an SSH environment with at least two reachable hosts and distinct credentials.

**Phase 2 (M10):** chained `direct-tcpip` channels with `russh`.

---

## ✅ M6 — Tiling, command console and broadcast

**Complete and manually accepted.** Syntax highlighting is tracked separately as M6b. Windows CI and user acceptance cover the tiling, command-console, session-protection and broadcast flows.

### Tiling

- Recursive split tree with single-pane, left/right, top/bottom and 2×2 presets; explicit split actions create an empty sibling pane.
- Each pane has its own session tab strip. Existing xterm/PTY sessions stay mounted while pane tabs are switched.
- Splitters support pointer dragging and keyboard adjustment. The active pane is identified by a subtle accent outline and a small dot.
- Closing a tab leaves its pane available as an empty pane; empty panes can be explicitly removed.
- Tabs can be moved between panes, reordered, or moved into a new pane by dragging, and with Ctrl+Alt+Shift+Arrows from the keyboard. Terminals are created once per session and re-attached to their pane, so a move keeps scrollback and screen state (design and behavior in [`docs/tab-moves.md`](docs/tab-moves.md)).
- Appearance: Catppuccin themes, background image, transparency and an optional see-through window (restart to apply), fixed-size dialogs, searchable settings, password reveal, and a workspace that starts with one empty pane (see [`docs/appearance.md`](docs/appearance.md)).
- Scripts library: save and load command scripts from the console, in folders, stored in a separate encrypted `scripts.vault`; favorites get one-click buttons below the console (see [`docs/scripts.md`](docs/scripts.md)).
- Connections can be dropped from the tree onto a pane or its edges (splitting it); creating an empty pane opens the connection list; empty panes can be focused to receive the next connection (rules in [`docs/pane-targets.md`](docs/pane-targets.md)).
- Pane layout is workspace state only and is not persisted. Command-console visibility and height are persisted separately.
- Connection-tree and connection-properties-pane visibility are independently persisted in non-secret `config.toml` (`[workspace_panels]`), defaulting to visible for existing and new installs.
- The properties pane shows the selected connection in a property/value grid and switches to the full inline connection editor for updates. It covers the saved connection configuration, including authentication, jump hosts, advanced options and forwarding settings; password and passphrase contents remain masked and are represented only by saved/not-saved state. See [`docs/workspace-panels.md`](docs/workspace-panels.md).
- Tree, properties and command-console headers share one 40 px header geometry, 12 px horizontal inset, themed bottom border and 14 px semibold sentence-case title. The shared UI typography scale is defined in `.agents/skills/sshterm-ui-design/SKILL.md` and documented in [`docs/workspace-panels.md`](docs/workspace-panels.md).

### Command console and broadcast

- Persistent bottom console with a searchable, vertically scrollable connection list on the left and a multiline command editor on the right.
- Targets: Active session, Selected sessions, or All open sessions. Disconnected retained tabs are visible but are never eligible for dispatch.
- Each open session tab has a quick lock/unlock control for its saved connection’s broadcast protection. The setting persists in the vault and updates all open tabs for that connection.
- Protected sessions are excluded from Selected/All by default; an explicit opt-in is required to include them. Active mode remains a deliberate single-session target.
- Multi-line payloads and sends to multiple sessions require a second confirmation with the target set shown.
- Writes are sequential and preserve the exact UTF-8 bytes; no newline is appended and no automatic retry is performed.
- The UI reports accepted IPC write requests and per-session failures. It cannot confirm that the remote shell completed a command.
- Console drafts remain in frontend memory and are never saved in config, the encrypted vault or logs. Visibility and height are saved in non-secret config.
- Shortcuts: Ctrl+Alt+K toggles the console; Ctrl+Alt+1–4 choose layouts; Ctrl+Alt+Arrows focus the neighboring pane (Ctrl+Alt+, / Ctrl+Alt+. cycle panes); Ctrl+Alt+Shift+Arrows move the active tab to it; Ctrl+Enter requests a send.

### Verification and remaining follow-up

- Windows CI runs split-tree tests, broadcast-target resolution tests, frontend checks/build, Rust formatting/tests and the portable Windows executable build.
- Manual acceptance is complete, including session protection, multi-line confirmation, focus restoration and broadcast behavior. Automated component-level UI tests and live multi-host dispatch integration tests remain future test-coverage improvements.

### Syntax highlighting (M6b — complete)

**Implemented foundation and terminal integration:**

- TOML rule files are loaded from `highlight/` beside the executable. A working example is tracked at `examples/highlight/linux.toml`; copy it into the portable app's `highlight/` directory to enable the example set.
- Rule files are validated for TOML shape, unique names, supported scopes, regex validity, color format and size limits. Matching uses Rust's linear-time regex engine rather than browser regexes.
- The connection editor assigns rule sets by name. Folder context menus assign inherited rule sets; nested folders accumulate assignments and a non-empty connection selection overrides inheritance.
- Terminal matching reads the rendered xterm.js normal buffer and adds cell decorations. It never changes PTY bytes or sends text to the remote host.
- Wrapped rows are joined for matching and decorations are mapped back to their terminal cells. Prompt and command scopes are classified using the rule set's prompt expression; output rules apply to non-prompt lines.
- Highlighting is suppressed in the alternate screen buffer (for example, vim, top and less).
- Settings includes a sample-text rule tester. Search fields in the rule pickers/tester receive focus when opened.

**Acceptance:** User validation is complete with Linux shell profiles and Ericsson MME terminal output. Windows CI also passes the frontend, Rust and portable executable checks. Additional fixtures and automated integration coverage for xterm decorations remain future hardening; a command-line `--check-highlight` mode is optional and is not implemented.
---

## ✅ M7 — Tunnels (implementation complete; Windows CI passed)

### Implemented

- Local (`-L`), reverse (`-R`) and dynamic SOCKS (`-D`) forwards are saved in the existing per-connection `tunnels` vault field; no vault migration is required.
- The connection editor can add/remove forwards, select the forwarding kind, and enable/disable each saved forward independently. Existing entries without an enabled field remain enabled.
- Listen endpoints accept `[address:]port`; destinations require `host:port`. IPv6 literals use brackets, e.g. `[::1]:5432`. Backend validation rejects invalid ports, malformed endpoints and duplicate listeners.
- Interactive terminal sessions continue to start their configured forwards with the SSH session and stop them when that session closes.
- A separate tunnel lifecycle starts one non-interactive OpenSSH process per saved connection, groups all enabled forwards for that connection, and works without opening a terminal tab. It reuses the connection's saved authentication and ProxyJump route.
- Password-authenticated connections and password-authenticated jump hosts must have their passwords saved in the vault; a tunnel-only process has no interactive terminal in which to ask for an unsaved password.
- Tauri commands: `tunnel_start`, `tunnel_stop`, `tunnel_stop_all`, and `tunnel_list`. The `tunnel:status` event publishes runtime state; statuses are in memory only and are not written to the vault.
- Lifecycle values are Starting, Active, Stopping, Stopped and Failed. Each configured listener also carries its own status and any identifiable setup error. Repeated start while Starting/Active is idempotent; stop is safe to repeat.
- `ExitOnForwardFailure=yes` makes OpenSSH fail when a requested listener cannot be created. Verbose SSH diagnostics identify listener setup, authentication, jump-host and unexpected process failures where possible.
- Locking the vault stops independent tunnel processes by default before clearing the vault; an application preference can keep them running while locked (the tunnel SSH/askpass processes and their in-memory credentials remain active). Application exit has an explicit Tauri exit hook that stops tunnel process trees.
- Application behavior settings are stored in non-secret config.toml: stop tunnels on vault lock (default on), and hide the window to the system tray on close (default off). The tray menu restores the window or exits the app; explicit exit still stops tunnel process trees.
- On Windows, stopping a tunnel terminates the OpenSSH process tree (including nested ProxyJump ssh.exe processes) with `taskkill /T /F`, then waits for the owned child to be reaped. Direct child kill/wait remains the fallback.
- Stop requests are idempotent, and the process-tree termination path is shared by normal stop, vault lock, application exit and monitor-start/polling failure cleanup.
- **Active is a listener-setup state only.** It does not test whether the remote destination service is reachable or healthy. For reverse listeners, remote sshd policy such as GatewayPorts still applies.
- Session tabs retain their compact configured-forward count; it is not a live-health indicator.
- The title bar shows a live green-dot/count indicator whenever any independent tunnel is starting, active or stopping. Selecting it opens the Tunnel Manager; it remains visible while the vault is locked if the user chose to keep tunnels running.
- The dedicated Tunnel Manager opens from the connections sidebar, including when no terminal is open. Its search field receives focus on opening and filters by connection name, host or user.
- The manager shows the saved route, each configured forward and its live status/error; it offers per-connection Start/Stop, Start All, Stop All and Edit actions.
- Stop All retains the stopped runtime entries in the manager so their final state remains visible and they can be restarted without overlapping the prior process.
- Runtime errors distinguish listener setup failures from destination connection failures after setup; an attempted connection to an unavailable forwarded destination does not mark the listener itself failed.

### M7d — Lifecycle hardening

- Explicit application-exit cleanup for independent tunnel process trees.
- Windows ProxyJump descendant cleanup and owned-child reaping on stop, vault lock, app exit and monitor failures.
- Idempotent repeated stop requests and a process-reaping regression test.
- Tunnel start/stop/stop-all and vault lock delegate blocking work to spawn_blocking rather than the WebView UI thread.
- Stop releases the child mutex before process-tree termination; monitor cleanup does not wait indefinitely for inherited stderr pipes.
- Listener readiness updates cannot overwrite a previously reported forward failure.

### Post-merge Windows smoke-test checklist

- Exercise local, reverse and SOCKS forwards, password/key/agent authentication and a multi-hop ProxyJump route.
- Check port conflicts, Start All/Stop All and immediate stop/restart.
- Toggle Stop tunnels on vault lock and verify the selected behavior.
- Toggle Hide to system tray, restore from the tray, and choose Exit with tunnels running.
- Confirm the title-bar indicator updates on start/stop and remains visible while the vault is locked when tunnels are kept alive.

**Verification:** [Windows workflow #448012](https://github.com/rhurta/sshterm/actions/runs/44801295593) passed frontend checks/build, Rust formatting/tests and the portable Windows x64 build. The user confirmed the post-hardening application runs as expected; the new tray/indicator settings are included in that passing build and should receive a final UI smoke test.

---

## 📋 M8 — Native SFTP (design documented; implementation not started)

**Done when:** the connection-backed SFTP browser supports direct and multi-hop access, common file operations, and reliable cancellable transfers.

See [docs/M8-implementation-plan.md](docs/M8-implementation-plan.md) for architecture, security gates, API direction, delivery phases and acceptance tests.

- **M8a — Protocol/security spike:** compatible `russh` + `russh-sftp`, auth parity, known-host verification, direct SFTP and `direct-tcpip` jump chains.
- **M8b — Backend lifecycle:** managed SFTP connections, typed commands, remote listing/navigation, close on vault lock and app exit.
- **M8c — Browser UI:** open from a saved connection; navigate/list, create directory, rename/delete safely, local file dialogs and keyboard support.
- **M8d — Transfers:** multi-select, recursive folder upload/download, streaming progress, overwrite confirmation, cancellation and partial-file cleanup.
- **M8e — Hardening:** Windows portable build, local OpenSSH integration tests, large-file/Unicode/permission/network-failure coverage and regression checks for M7.
- The SFTP port override is optional and inherits the saved connection's SSH port when unset. Jump hosts retain their own saved ports.
- M8 uses the native Russh client only for SFTP. The OpenSSH/ConPTY terminal engine and M7 OpenSSH tunnel manager remain unchanged until M10.

--- 

## 🟡 M9 — Polish (partial)

**Already implemented:** application-wide Dark, Light and Nord themes, persisted in `config.toml` and applied to the lock screen, title bar, workspace, dialogs and existing terminals.

**Still to do:** per-connection colors, host-key confirmation UI, auto-reconnect with backoff, idle lock and final release packaging/signing.

- Per-connection `fg`/`bg` colors: apply to the terminal theme and tab accent.
- Host-key change: hard visual warning with the old and new fingerprints.
- Auto-reconnect: exponential backoff shown in the tab title.
- Idle lock: configurable timeout; sessions stay open, new connections need the password.
- Release build: code-stripped binary, installer.

---

## 📋 M10 — Native engine (`russh`)

**Done when:** `RusshTransport` reaches parity with phase 1 and becomes the default; `openssh` stays selectable per connection.

- `russh` + `russh-keys` implement the full `Transport` trait.
- Typed errors (no more parsing process output).
- Jump chains via chained `direct-tcpip` channels.
- Tunnels as native listeners.
- Per-connection `engine` field: `"russh"` (default after M10) | `"openssh"` (kept for legacy equipment).
- Private key files read in place; path stored in vault, key content never copied.

---

## Milestone summary

| # | Milestone | Commit / tag | Status |
|---|-----------|--------------|--------|
| M0 | Portable skeleton | `bd4d6b4` / m1 | ✅ |
| M1 | Vault + i18n | `bd4d6b4` / m1 | ✅ |
| M2 | First SSH session | `2c19e40` / m2 | ✅ |
| M3 | Session logging | `bbc0262` | ✅ |
| M4 | Session tree + multi-tab | `bbb2c6c` / m4 | ✅ |
| M5 | Jump hosts | `main` | ✅ |
| M6 | Tiling + command console + broadcast | `e6fa4bc` | ✅ Complete; manually accepted |
| M6b | Syntax highlighting | `main` | ✅ Complete; user-validated on Linux and Ericsson MME |
| M7 | Independent tunnels, SOCKS, runtime status, manager, tray behavior and live indicator | `64f640a` | ✅ Implemented; Windows CI passed |
| M8 | Native SFTP | — | 📋 Plan: `docs/M8-implementation-plan.md` |
| M9 | Polish | `main` | 🟡 Partial; global themes implemented |
| M10 | Native `russh` engine | — | 📋 |


### Fixes after the M3 review (branch `fix/m5-jump-config`)

- Jump hops now get the same options as the target (see M5 above); verified with real `ssh` against two local sshd instances starting from an empty known_hosts (`cargo test real_ssh -- --ignored`, needs `SSHTERM_E2E_HOP_PORT`, `SSHTERM_E2E_TARGET_PORT`, `SSHTERM_E2E_USER`).
- Hops whose saved connection uses password auth get `PubkeyAuthentication no` and `PreferredAuthentications password,keyboard-interactive` in their own `Host` block, so a full ssh-agent cannot exhaust the hop's `MaxAuthTries` before the password is tried (verified with real ssh: 5 agent keys, hop with `MaxAuthTries 3`; `cargo test real_ssh_password_hop -- --ignored`, extra env vars in the test doc).
- Target `user`/`host` are validated like hop values (no leading `-`, whitespace, quotes, `@`, `,`...) when saving and when opening a session; the destination is passed after `--`.
- `keepalive_secs = 0` is kept (disabled) instead of becoming 30; the editor allows 0.
- New sessions open with the size of the last fitted terminal instead of 80x24, avoiding an extra ConPTY repaint.
- `session_write` calls from the pane are chained so input keeps its order.
- `UserKnownHostsFile` with spaces was already quoted; in the jump config the path uses forward slashes.
