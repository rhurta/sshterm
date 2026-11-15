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

## 🚧 M4 — Session tree

**Done when:** folders, CRUD, search, import from PuTTY / WinSSHTerm / `~/.ssh/config`, and multi-tab workspace.

### What changes

The spec's M4 is primarily about the **session tree** (sidebar with folders, drag-and-drop, search, import). The multi-tab workspace — multiple open sessions at once — ships together with it because the sidebar is the natural place to open new tabs.

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
Sidebar
├── SearchBox
├── Tree (folders + connections, drag-and-drop)
│   ├── FolderNode (expand/collapse, rename, delete)
│   └── ConnectionNode (open, edit, delete)
└── ImportButton (PuTTY / WinSSHTerm / ~/.ssh/config)

Workspace
├── TabBar  [● pgw-01 ×] [server2 ×]  [+]
└── TerminalPane[]  (one per tab, hidden when inactive)
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
| `tree_get` | Return full tree |
| `conn_upsert` | Create or update a connection |
| `conn_delete` | Delete a connection |
| `folder_upsert` | Create or rename a folder |
| `folder_delete` | Delete folder (and optionally its children) |
| `search` | Search connections by name / host / user |
| `import_sshconfig` | Import from `~/.ssh/config` |
| `import_putty` | Import from PuTTY registry |
| `import_winsshterm` | Import from WinSSHTerm config file |

### Implementation checklist

- [ ] Extend vault data model (tree, colors, protected)
- [ ] `store` module: tree CRUD + search
- [ ] Import parsers (`~/.ssh/config`, PuTTY registry, WinSSHTerm)
- [ ] Sidebar component (tree, drag-and-drop, search)
- [ ] Connection editor (general, auth, colors, keepalive)
- [ ] `Workspace.svelte` with tab bar
- [ ] `TerminalPane` visibility + deferred resize
- [ ] Per-connection colors applied to terminal theme and tab accent

---

## 📋 M5 — Jump hosts

**Done when:** `-J` chain works end-to-end, including a different password per hop.

- Each connection gains an ordered `jump: ["conn_id", …]` list.
- Phase 1: passed as `-J host1,host2` to `ssh.exe`; the askpass helper identifies the hop from ssh's prompt text and returns the right password.
- Phase 2 (M10): chained `direct-tcpip` channels with `russh`.
- Failures name the hop that failed.
- UI: jump chain editor in the connection editor, visual chain diagram in the tab.

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

## 📋 M7 — Tunnels

**Done when:** `-L`, `-R`, `-D` saved per connection and started with the session or on demand.

- Tunnels stored in the vault per connection.
- Phase 1: passed as `-L`/`-R`/`-D` flags, or a background `ssh -N` process.
- Status display: active / failed / port-in-use.
- UI: tunnel manager panel (list, start, stop, status).

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
| M4 | Session tree + multi-tab | — | 🚧 |
| M5 | Jump hosts | — | 📋 |
| M6 | Tiling + broadcast + highlighting | — | 📋 |
| M7 | Tunnels | — | 📋 |
| M8 | Native SFTP | — | 📋 |
| M9 | Polish | — | 📋 |
| M10 | Native `russh` engine | — | 📋 |
