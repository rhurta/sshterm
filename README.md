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
| M4 | Session tree (folders, CRUD, search, import) | 🚧 Next |
| M5 | Jump hosts (`-J` chain, per-hop passwords) | 📋 Planned |
| M6 | Tiling, broadcast, syntax highlighting | 📋 Planned |
| M7 | Port-forwarding tunnels | 📋 Planned |
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
├── src/                        # Frontend (SvelteKit)
│   ├── lib/
│   │   ├── components/         # Svelte UI components
│   │   ├── i18n/               # Translation dictionaries (en, es)
│   │   ├── encoding.ts         # Base64 helpers
│   │   ├── logCapture.ts       # Screen-buffer log capture
│   │   └── sessionBus.ts       # session:data / session:closed event bus
│   └── routes/                 # SvelteKit pages
└── src-tauri/                  # Backend (Rust)
    └── src/
        ├── vault/              # Encrypt / decrypt credential store
        ├── store/              # Connection and tree CRUD
        ├── session/            # Open / close / resize sessions
        ├── transport/          # Transport trait + openssh.rs (+ russh.rs in M10)
        ├── askpass/            # Named-pipe password delivery to ssh.exe
        ├── logger/             # Per-session log files
        ├── config.rs           # config.toml model
        └── paths.rs            # Portable path resolution
```

## Development

### Prerequisites

- Rust (stable, via rustup)
- Node.js 18+ and npm
- Windows 11 — WebView2 and OpenSSH are pre-installed
- Visual Studio Build Tools (MSVC toolchain)

### Run in dev mode

```bash
npm install
npm run tauri dev
```

### Production build

```bash
npm run tauri build
# exe: src-tauri/target/release/sshterm.exe
# MSI: src-tauri/target/release/bundle/msi/
```

### Tests

```bash
cd src-tauri
cargo test            # all unit tests
cargo test logger     # logger module only
cargo test vault      # vault module only
```

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

## Configuration

`config.toml` (next to the exe):

```toml
language   = "en"       # "en" | "es"
log_output = true
log_mode   = "screen"   # "screen" | "raw"
```

## Security notes

- No secrets in logs, `config.toml`, or crash output.
- The named askpass pipe is restricted to the current user; it accepts one token per session.
- `ssh.exe` child processes are killed when the app exits or the vault locks.
- Private key files are stored by path only (the vault keeps the path, not the key content).
- Host keys: trust-on-first-use; a changed key triggers a hard warning.
- Clipboard cleared shortly after copying a password.
