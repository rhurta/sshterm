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
- `OpenSshTransport` emits one `-J` argument with comma-separated `user@host:port` entries; IPv6 literals are bracketed.
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

## 📋 M6 — Tiling, broadcast and syntax highlighting

**Done when:** splits work, broadcast reaches a hand-picked set of panes, and rule files load and colorize prompt / command / parameters / output / IPs.

### Tiling

- Layouts: tabs (current), split horizontal, split vertical, 2×2 grid.
- Pane handles: drag to resize.

### Broadcast

- Send input to: all visible panes, all open sessions, or a hand-picked set.
- `protected: true` connections are excluded by default and visually marked.
- Multi-line paste shows a confirmation dialog before sending.

### Syntax highlighting (M6b)

Rule files are plain TOML in `highlight/` next to the exe.

```toml
name   = "linux"
prompt = '^\S+@\S+:[^\n]*[$#] '

[[rule]]
name = "command"; scope = "command"
pattern = '^\S+'; fg = "#e3b341"; bold = true

[[rule]]
name = "param"; scope = "command"
pattern = '(?<=\s)--?[\w-]+'; fg = "#79c0ff"

[[rule]]
name = "ipv4"; scope = "any"
pattern = '\b\d{1,3}(\.\d{1,3}){3}(/\d+)?\b'; fg = "#d2a8ff"
```

- Matching runs on the xterm.js buffer through its decoration API, never by rewriting the byte stream.
- Scopes: `prompt`, `command`, `output`, `any`.
- Connections (or folders for all their children) link rule sets by name: `highlight = ["linux", "common"]`.
- Highlighting off while alternate screen is active (vim, top, less).
- Validation: JSON schema + `sshterm.exe --check-highlight file.toml` + in-app test panel (paste sample text, see classification and colors).
- Guard against catastrophic backtracking: pattern-length limit and timing check in the validator.

---

## 🟡 M7 — Tunnels (partial)

### Implemented in phase 1

- Local (`-L`) and reverse (`-R`) TCP forwards are saved in the existing per-connection `tunnels` vault field; no vault migration is required.
- The connection editor can add/remove forwards, select local or reverse direction, and enable/disable each saved forward independently.
- Existing vault tunnel entries deserialize as enabled by default; no vault version bump or migration is needed.
- Listen endpoints accept `[address:]port`; destinations require `host:port`. IPv6 literals use brackets, e.g. `[::1]:5432`.
- The backend validates port ranges, malformed endpoints, and duplicate listeners before saving; session startup validates older vault entries again.
- Forwards are passed as separate OpenSSH arguments. `ExitOnForwardFailure=yes` ensures SSH exits when a listener cannot be created.
- Forwards run for the lifetime of the interactive SSH session and stop when that session closes. Tabs show the enabled forward count.
- Recognized SSH diagnostics for unreachable jump hosts and failed local/reverse listeners are also emitted as workspace errors while terminal output is preserved.

### Deferred

- Standalone tunnel-only sessions and on-demand start/stop without opening an interactive terminal.
- Structured active / failed / port-in-use status and a dedicated tunnel manager panel. At present, listener failures are reported by OpenSSH in the terminal output and by the session ending.
- Dynamic SOCKS forwarding (`-D`).

The vault model already contains a `Dynamic` variant for forward compatibility, but it is not enabled in the UI or phase-1 transport.

---

## 📋 M8 — Native SFTP

**Done when:** `russh-sftp` file browser and downloads with custom ports work.

- SFTP uses the connection's own host, port, and jump chain (optional separate SFTP port).
- Operations: list directory, download (single file and recursive folder), upload, delete, rename.
- Multi-select, progress bar, cancel, overwrite confirmation.
- `russh` enters the project here (before it replaces the phase-1 engine in M10).

---

## 📋 M9 — Polish

**Done when:** per-connection colors, host-key confirmation UI, auto-reconnect with backoff, idle lock, release build.

- Per-connection `fg`/`bg` colors: applied to the terminal theme and tab accent.
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
| M6 | Tiling + command console + broadcast | `9aae6ef` (tiling presets) | 🟡 In progress |
| M7 | Local / reverse forwards (`-L` / `-R`) | `main` | 🟡 Partial |
| M8 | Native SFTP | — | 📋 |
| M9 | Polish | — | 📋 |
| M10 | Native `russh` engine | — | 📋 |
