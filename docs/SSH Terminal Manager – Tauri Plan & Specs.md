# SSH Terminal Manager – Tauri Plan & Specs

A portable, modular SSH front end for Windows 11 (WinSSHTerm-style), built with Tauri and Rust. It starts on Windows' built-in OpenSSH client (ssh.exe) and adds a native russh engine later.

## 1. Goals and constraints

- **Portable:** one executable plus data files next to it. No installer, no registry, no `%APPDATA%`.
- **Modular:** each feature is a separate Rust module and a separate UI component, so it can be built and tested in steps.
- **Secure by default:** every secret lives in one encrypted vault opened with a master password.
- **Phased SSH engine:** phase 1 runs Windows' built-in OpenSSH client (`ssh.exe`) inside a pseudo-terminal the app owns; phase 2 adds a native `russh` engine. The UI and every other module stay the same between phases.
- **Primary target:** Windows 11 (WebView2 and the OpenSSH client are preinstalled). Linux and macOS are possible later.

## 2. Technology choices

| Layer | Choice | Reason |
| --- | --- | --- |
| Shell | Tauri 2 | Small single exe, native Rust backend |
| Backend | Rust, tokio | Async processes, tunnels and transfers |
| SSH, phase 1 | `ssh.exe` (Windows OpenSSH) run through `portable-pty` (ConPTY) | Fast to build; the app keeps control of the terminal |
| SSH, phase 2 | `russh` (+ `russh-keys`) | Pure Rust, no external files, full control of tunnels and keepalive |
| SFTP | `russh-sftp`, from the start | A native file browser is hard to build on top of `sftp.exe` |
| Crypto | `argon2`, `chacha20poly1305`, `zeroize` | Vault encryption and key derivation |
| Frontend | TypeScript + Svelte 5 | Small bundle, Svelte 5 runes |
| Terminal | `xterm.js` + FitAddon, canvas renderer | Mature emulator; WebGL addon deferred |
| Config | `serde`, `serde_json`, `toml` | Vault payload and settings |
| Date/time | `chrono` (clock feature, no default features) | Readable timestamps in log file names |

Notes: `ssh.exe`, `scp.exe` and `sftp.exe` ship with Windows 11 as the OpenSSH client feature. Verify the exact `portable-pty` and `russh` APIs when starting each module. SFTP is native from the start (M8), so `russh` enters the project before phase 2.

## 3. Portable layout

```
sshterm.exe
sshterm.vault      encrypted: sessions, passwords, tunnels, colors
config.toml        plain: language, log settings, font, default keepalive
known_hosts        plain, public host keys (used by ssh.exe in phase 1)
highlight/         plain rule files (*.toml) linked to connections
logs/              <connection>_<YYYYMMDD-HHMMSS>.log  (one per session)
webview-data/      WebView2 cache (WEBVIEW2_USER_DATA_FOLDER points here)
```

Rules:

- Resolve every path from the exe's directory.
- Do not use DPAPI or the Windows Credential Manager. The master password alone must unlock the vault on any machine.
- Set `WEBVIEW2_USER_DATA_FOLDER` before the window is created so nothing is written outside the folder.
- Phase 1 needs the OpenSSH client that Windows 11 installs in System32. At startup, look for ssh.exe in a `bin/` folder next to the exe first, then in System32; show a clear error if neither has it.

## 4. Vault specification

**File format**

| Field | Size | Content |
| --- | --- | --- |
| Magic | 4 bytes | `SSHV` |
| Version | 1 byte | `1` |
| KDF params | 12 bytes | Argon2id memory (KiB), iterations, parallelism |
| Salt | 16 bytes | random per vault |
| Nonce | 24 bytes | random per save |
| Ciphertext | rest | XChaCha20-Poly1305 over the JSON payload (the header is authenticated as AAD) |

**Behavior**

- Default KDF: Argon2id, 64 MiB, 3 iterations, 1 lane. Store the parameters in the header so they can be raised later.
- A wrong password fails the AEAD check; the app reports "wrong password or corrupted file" and nothing else.
- Save atomically: write `sshterm.vault.tmp`, flush, then rename. Keep one `.bak` copy.
- Generate a new nonce on every save.
- Hold the derived key and decrypted secrets in `zeroize`-wrapped memory. Auto-lock after N idle minutes and on demand; locking drops the key.
- Changing the master password re-encrypts the whole payload with a new salt.
- Optional later: a key file as a second factor.

## 5. Data model (vault payload)

```json
{
  "version": 1,
  "tree": [
    { "type": "folder", "id": "f1", "name": "Core", "children": [
      { "type": "connection", "id": "c1" }
    ]}
  ],
  "connections": {
    "c1": {
      "name": "pgw-01",
      "host": "10.0.0.5",
      "port": 22,
      "user": "admin",
      "auth": { "type": "password", "password": "..." },
      "jump": ["c9"],
      "keepalive_secs": 30,
      "colors": { "fg": "#d0d0d0", "bg": "#1b1b1b" },
      "protected": false,
      "tunnels": [
        { "kind": "local", "bind": "127.0.0.1:8080", "dest": "10.1.1.1:80" }
      ],
      "sftp": { "port": 22, "start_dir": "/home/admin" }
    }
  },
  "known_hosts": { "10.0.0.5:22": "SHA256:..." }
}
```

`auth.type` is `password`, `key` (path to a key file plus optional passphrase) or `agent`. `jump` is an ordered list of connection ids, so multi-hop chains are possible.

`highlight` is an optional list of rule-set names on a connection or a folder (see Syntax highlighting in section 8). A folder's list applies to its children unless they set their own.

**Current state (through M3):** `tree` and `connections` are stored as free-form `serde_json::Value` in `model.rs`. The `Connection` struct in `store/mod.rs` covers only the fields needed so far: `id`, `name`, `host`, `port`, `user`, `password`, `keepalive_secs`. The full model above will be realised in M4.

## 6. Backend modules

Each module exposes Tauri commands and emits events. The frontend never touches secrets directly except to display or edit them while unlocked.

| Module | Responsibility | Main commands / events | Status |
| --- | --- | --- | --- |
| `vault` | Create, unlock, lock, save, change password | `vault_unlock`, `vault_lock`, `vault_save` | ✅ M1 |
| `store` | Connection CRUD; tree and search in M4 | `conn_list`, `conn_get`, `conn_upsert`, `conn_delete` | ✅ M2 (flat list) |
| `transport` | `Transport` trait; `openssh.rs` (phase 1) | internal | ✅ M2 |
| `askpass` | Hands vault passwords to `ssh.exe` via named pipe | internal | ✅ M2 |
| `session` | Open/close sessions, resize, keepalive, log bridge | `session_open`, `session_write`, `session_resize`, `session_close`, `session_close_all`, `session_log_append`, `session_log_close`; event `session:data`, `session:closed` | ✅ M2–M3 |
| `logger` | One log file per session | internal, called by `session` | ✅ M3 |
| `broadcast` | Send input to a set of sessions | `broadcast_send` | 📋 M6 |
| `tunnel` | Local, remote and dynamic forwarding | `tunnel_start`, `tunnel_stop` | 📋 M7 |
| `sftp` | Native list, download, upload, progress | `sftp_list`, `sftp_get`; event `sftp:progress` | 📋 M8 |
| `hostkeys` | Trust on first use, mismatch warning | `hostkey_confirm` | 📋 M9 |
| `highlight` | Load and validate rule files; matching in the frontend | `highlight_load`, `highlight_check` | 📋 M6b |

### Transport layer and phases

Session code talks to a `Transport` trait, never to `ssh.exe` or `russh` directly. A per-connection setting `engine` (`openssh` or `russh`) picks the implementation.

```rust
trait Transport {
    async fn open(&mut self, cfg: &SshConfig, size: PtySize) -> Result<(), TransportError>;
    async fn write(&mut self, data: &[u8]) -> Result<(), TransportError>;
    async fn resize(&mut self, size: PtySize) -> Result<(), TransportError>;
    async fn close(&mut self) -> Result<(), TransportError>;
    fn output(&mut self) -> Option<Receiver<Vec<u8>>>;
}
```

Note: the actual signature uses `&SshConfig` (not `&Connection`) and returns `Option<Receiver<Vec<u8>>>` from `output()`. `russh.rs` does not exist yet; it arrives in M10.

| Feature | Phase 1: `ssh.exe` over ConPTY | Phase 2: `russh` |
| --- | --- | --- |
| Jump servers | `-J host1,host2` | Chained `direct-tcpip` channels |
| Tunnels | `-L`, `-R`, `-D` flags, or a background `ssh -N` process | Native listeners |
| Keep alive | `-o ServerAliveInterval=N -o ServerAliveCountMax=3` | Client keepalive settings |
| Custom port | `-p` | Connection setting |
| Host keys | `-o UserKnownHostsFile=` pointing to `known_hosts` next to the exe | `hostkeys` module |
| Passwords | `SSH_ASKPASS` helper | Read directly from the vault |
| Errors and status | Parsed from process output and exit code | Typed errors |
| File downloads | Native SFTP | Native SFTP |
| Colors, logs, tiling, broadcast | Handled by the app | Handled by the app |

**Password hand-off (phase 1).** `ssh.exe` is started with `SSH_ASKPASS` pointing at the app's own exe (run with `--askpass`) and `SSH_ASKPASS_REQUIRE=force`. The helper connects to a named pipe owned by the main process, presents a one-time token, receives the password for the host in ssh's prompt, and writes it to stdout. The password never appears in a command line or environment variable.

**ConPTY EOF handling (implemented in M2).** ConPTY does not reliably deliver EOF on the output pipe when `ssh.exe` exits. A watchdog thread polls `child.try_wait()` every 100 ms. When the child exits, the watchdog waits 150 ms for ConPTY to flush remaining output, then waits up to 2 seconds for the reader thread to drain (using `JoinHandle::is_finished()`), then sends an empty sentinel chunk to trigger `session:closed`.

**Known limits of phase 1**

- Tunnel and connection status come from process output and exit codes, so errors are less precise than with `russh`.
- Per-hop passwords with `-J`: the prompt text names the host, so the helper can pick the right credential. Verify this in M5.
- Private keys stay outside the vault; the app stores only the path and passes it to `ssh.exe` with `-i`.
- Host keys live in a plain `known_hosts` file next to the exe, not in the vault.
- The OpenSSH client feature must be present on the machine.

## 7. Frontend components

| Component | Purpose | Status |
| --- | --- | --- |
| Lock screen | Master password entry, create-vault flow | ✅ M1 |
| Language selector | Pick language, persist to config.toml | ✅ M1 |
| Connection sidebar | List of connections; open in new tab | ✅ M2 (flat list) |
| Connection editor | Create/edit a connection | ✅ M2 |
| Workspace | Tab bar + terminal panes | ✅ M2–M3 |
| Terminal pane (`TerminalPane.svelte`) | xterm.js pane, resize, LogCapture | ✅ M2–M3 |
| Sidebar with tree | Folders, drag-and-drop, search, import | 📋 M4 |
| Tunnel manager | Active tunnels, start/stop | 📋 M7 |
| SFTP panel | Remote file list, download queue | 📋 M8 |
| Broadcast bar | Command input, per-pane checkboxes | 📋 M6 |
| Highlighting test panel | Paste sample text, see classification | 📋 M6b |

## 8. Feature specifications

### Logging (implemented in M3)

**The core problem with PTY-byte logging:** ConPTY does not forward raw remote bytes. It maintains its own screen buffer and sends redraws — full repaints on resize and scroll, partial-line updates with cursor-positioning sequences. Stripping escape codes from that stream loses the cursor-movement context that explains the redraws, producing duplicate MOTD lines, text fragments, and stray numbers from escape-sequence parameters.

**The solution: log from the xterm.js screen buffer.** xterm.js applies every redraw correctly, so its buffer holds each line exactly once. `LogCapture.ts` monitors the buffer and sends complete lines to the backend only after the cursor has moved past them.

```
PTY redraws ──▶ xterm.js ──▶ screen buffer (each line once)
                                    │
                              LogCapture.ts
                              • IMarker tracking (survives scroll/reflow)
                              • screen-erase detection (ESC[J, RIS)
                              • repaint dedup via recent-line overlap check
                              • alternate-screen skip (vim, less, top)
                              • blank-line collapsing (at most one in a row)
                              • 250 ms flush batch
                                    │  session_log_append / session_log_close
                              SessionLogger (Rust)
                              • write_text()  — Screen mode (default)
                              • write_raw()   — Raw mode (diagnostics)
                              • create_new() collision: _2, _3 suffixes
                              • finish() writes closing RFC 3339 timestamp
                                    │
                      logs/<connection>_<YYYYMMDD-HHMMSS>.log
```

Log file name: `sanitize(connection_name)` replaces Windows-illegal characters and trims trailing dots and spaces; truncated to 80 characters. Two sessions of the same connection starting within the same second get `_2`, `_3`, etc. (using `OpenOptions::create_new`).

**Configuration** (`config.toml`):
```toml
log_output = true       # false → no file created
log_mode   = "screen"   # "screen" (default) | "raw"
```

The old fields `log_input` and `strip_special` were removed in the M3 fix and are silently ignored if present in an existing `config.toml`.

**What the log contains (screen mode):**
- Header: `sshterm session log`, connection name, user, host, started timestamp (RFC 3339).
- Body: one copy of each line the user saw, trailing whitespace trimmed, blank lines collapsed to at most one.
- Alternate-screen content (vim, less, htop) is excluded entirely.
- Progress bars and `\r` overwrites never appear — they overwrite in the terminal buffer before LogCapture reads the line.
- Footer: `---- ended: <RFC 3339 timestamp>` written by `session_log_close`.

**Tiling and broadcast** (M6)

- Layouts: tabs (current), split horizontal, split vertical, grid.
- Broadcast targets: all visible panes, all open sessions, or a hand-picked set.
- Panes with `protected: true` are excluded by default and marked clearly.
- Multi-line paste asks for confirmation before sending.

**Tunnels** (M7)

- Stored per connection; start automatically or manually.
- Local (-L), remote (-R), dynamic SOCKS5 (-D).
- Show status and errors (port already in use, destination refused).

**SFTP** (M8)

- Uses the connection's own host, port and jump chain, with an optional separate SFTP port.
- Multi-select download, folder download (recursive), progress, cancel, overwrite confirmation.

**Jump servers** (M5)

- Ordered chain per connection; each hop uses its own credentials from the vault.
- Failures name the hop that failed.

**Keep alive**

- Per-connection SSH keepalive interval and max missed count (already passed as flags in M2).
- Optional auto-reconnect with backoff, shown in the tab title (M9).

**Colors** (M9)

- Per-connection foreground and background; applied to the terminal theme and a tab accent so PROD and DEV are easy to tell apart.

**Syntax highlighting** (M6b)

Rules are plain TOML files in `highlight/` next to the exe. They hold no secrets, so they stay out of the vault. A connection, or a folder that passes it down to its children, links to one or more rule sets by name: `highlight = ["linux", "common"]`.

- Rules are matched on the rendered `xterm.js` buffer through its decoration API, never by rewriting the byte stream, because escape codes can be split across chunks.
- A rule file defines a `prompt` regex. A line that matches it is a prompt line, the text after the prompt is the command line, and the lines until the next prompt line are output.
- Each rule has a `scope`: `prompt`, `command`, `output` or `any`. Rules apply in file order and a later rule wins where they overlap.
- Categories: prompt, command (first word after the prompt), parameters (`-x`, `--long`), output, IP addresses and keywords.
- If several rule sets are linked, the first one that defines a `prompt` wins.
- Wrapped lines are joined before matching, and only visible or changed lines are processed.
- Highlighting is switched off while a full-screen program (`vim`, `top`, `less`) uses the alternate screen.

```toml
name = "linux"
prompt = '^\S+@\S+:[^\n]*[$#] '

[[rule]]
name = "command"
scope = "command"
pattern = '^\S+'
fg = "#e3b341"
bold = true

[[rule]]
name = "param"
scope = "command"
pattern = '(?<=\s)--?[\w-]+'
fg = "#79c0ff"

[[rule]]
name = "ipv4"
scope = "any"
pattern = '\b\d{1,3}(\.\d{1,3}){3}(/\d+)?\b'
fg = "#d2a8ff"
```

**Rule generation and validation**

- A `skill.md` documents the schema with examples and tells an AI how to write a rule file from a pasted sample session, which must include a few prompt lines.
- Validation: a JSON Schema, a command `sshterm.exe --check-highlight file.toml`, and a test panel where you paste sample text and see each line classified as prompt, command or output with the colors applied.
- Guard against slow regexes (catastrophic backtracking): limit pattern length and test timing in the checker, or match in the Rust backend with the `regex` crate.
- No MCP for now. If one is added later, it exposes only `validate_rules`, `test_rules(sample)` and `save_rules`, with no access to sessions or the vault.

## 9. Security checklist

- Trust on first use for host keys, with a hard warning if a key changes.
- No secrets in logs, config.toml or crash output.
- Clipboard is cleared a short time after copying a password.
- Programs in the terminal cannot write to the clipboard without asking.
- Vault locks on idle; all sessions stay open but new connections need the password again.

**Phase 1 additions**

- The askpass named pipe is restricted to the current user (via ACL, `askpass/acl.rs`) and accepts a one-time token per session.
- Passwords never go on a command line; the only value in an environment variable is the one-time token.
- `ssh.exe` child processes are killed when the app exits or the vault locks.
- Key files are never copied: the app stores only their path and reads them in place.

## 10. Repository layout

```
sshterm/
  README.md         project overview, goals, tech stack, architecture notes
  ROADMAP.md        milestone table with implementation detail
  src-tauri/
    Cargo.toml      dependencies: tauri, argon2, chacha20poly1305, rand,
                    zeroize, toml, chrono, portable-pty, tokio, async-trait,
                    base64, serde, serde_json, tempfile (dev)
    src/
      main.rs       entry point; detects --askpass mode before creating window
      lib.rs        module declarations, WebView2 setup, command registration
      paths.rs      portable path resolution (all paths from exe directory)
      commands.rs   VaultState, vault_* and config_* commands
      config.rs     Config { language, log_output, log_mode }; LogMode enum
      vault/        mod.rs, crypto.rs, model.rs, error.rs, tests.rs
      store/        mod.rs — Connection CRUD (flat list through M3)
      transport/    mod.rs (Transport trait + SshConfig + PtySize),
                    openssh.rs (OpenSshTransport, build_command, watchdog)
      askpass/      mod.rs (AskpassServer, run_helper), acl.rs (pipe ACL)
      session/      mod.rs — session_open/write/resize/close + log bridge
      logger/       mod.rs — SessionLogger, create_file, sanitize, stamp
  src/
    app.html
    routes/
      +layout.ts    ssr = false
      +page.svelte  top-level orchestrator
    lib/
      LockScreen.svelte
      LanguageSelect.svelte
      ConnectionSidebar.svelte   flat connection list (M2–M3)
      ConnectionEditor.svelte    create / edit connection form (M2)
      Workspace.svelte           tab bar + terminal pane mounting (M2–M3)
      TerminalPane.svelte        xterm.js + FitAddon + LogCapture (M2–M3)
      logCapture.ts              screen-buffer log capture (M3)
      sessionBus.ts              session:data / session:closed event fan-out
      encoding.ts                base64 helpers
      types.ts                   shared TypeScript types
      i18n/
        en.ts  es.ts  index.svelte.ts
  docs/             local working documents (in .gitignore)
```

## 11. Milestones

| # | Milestone | Done when | Status |
| --- | --- | --- | --- |
| M0 | Skeleton | Tauri app builds to one exe; portable paths and WebView2 data folder work | ✅ |
| M1 | Vault | Create, lock, unlock and change password; unit tests for wrong password and corruption | ✅ |
| M2 | First session on `ssh.exe` | `Transport` trait and `OpenSshTransport` over ConPTY; xterm.js terminal, resize, keepalive flags, password through askpass from the vault | ✅ |
| M3 | Logging | One log file per session; screen-buffer capture via `LogCapture.ts`; no duplicate lines from ConPTY redraws | ✅ |
| M4 | Session tree | Folders, CRUD, search, import from PuTTY / WinSSHTerm / `~/.ssh/config`; multi-tab workspace | 🚧 |
| M5 | Jump hosts | `-J` chain works, including a different password per hop | 📋 |
| M6 | Tiling and broadcast | Splits, per-pane targeting, protected hosts | 📋 |
| M6b | Syntax highlighting | Rule files load and link to connections; prompt, command, parameter, output, IP and keyword colors work; test panel; `--check-highlight` | 📋 |
| M7 | Tunnels | `-L`, `-R`, `-D` saved per connection and started with the session or on demand | 📋 |
| M8 | Native SFTP | `russh-sftp` file browser and downloads with custom ports | 📋 |
| M9 | Polish | Colors, host key UI, auto-reconnect, idle lock, release build | 📋 |
| M10 | Native engine | `RusshTransport` reaches parity with phase 1; becomes the default; openssh stays selectable per connection | 📋 |

Each milestone ends with a working exe, so you can use the tool from M2 onward.

## 12. Decisions

- Frontend: Svelte 5 with TypeScript.
- OpenSSH client: look for `ssh.exe` in `bin/` next to the exe first, then in System32.
- Private keys: the vault stores only the path to the key file (for example `~/.ssh/id_ed25519`), which stays where it is. Nothing is written to disk by the app and the key is not covered by the master password; an optional passphrase can be kept in the vault. Both engines read the file in place, including `russh`.
- Engine: `russh` becomes the default once it reaches parity (M10); `openssh` stays selectable per connection, for example for legacy equipment.
- Import: `~/.ssh/config`, PuTTY (registry) and WinSSHTerm in M4. The WinSSHTerm file format is checked when the connections file is provided.
- Code signing: none, the tool is for personal use.
- Logging: log from the xterm.js screen buffer (not PTY bytes) so ConPTY redraws never produce duplicate lines. `LogMode::Screen` is the default; `LogMode::Raw` is available for diagnostics.
- Syntax highlighting: TOML rule files in `highlight/` with scoped rules (prompt, command, parameters, output, IPs, keywords), written with AI help from a skill file and checked by a validator; no MCP for now. Milestone M6b.
- `chrono` crate added in M3 for local date-time in log file names (default features disabled, `clock` feature only).
