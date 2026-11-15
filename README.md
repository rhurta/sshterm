# SSHTerm

A secure, portable SSH terminal application built with Tauri, Rust, and SvelteKit.

## Overview

SSHTerm is a cross-platform SSH client that provides a secure, user-friendly terminal experience with encrypted credential storage and comprehensive session logging.

## Features

### ✅ Completed (M0-M3)

- **M0: Project Foundation**
  - Tauri + SvelteKit + TypeScript setup
  - Basic window and application structure

- **M1: Encrypted Vault & i18n**
  - AES-256-GCM encrypted credential storage
  - Argon2id key derivation from master password
  - Multi-language support (English, Spanish)
  - Secure vault operations (create, unlock, lock, change password)
  - Connection management (add, edit, delete)

- **M2: SSH Session Management**
  - Real SSH connections via Windows ssh.exe over ConPTY
  - Password delivery through secure named pipe (askpass)
  - Host key verification with trust-on-first-use (TOFU)
  - Keep-alive and timeout configuration
  - EOF watchdog for reliable session cleanup
  - Full terminal emulation with xterm.js

- **M3: Session Logging**
  - Per-session log files with readable timestamps
  - Screen buffer capture (logs what user sees, not PTY redraws)
  - Two modes: Screen (clean text) and Raw (PTY bytes for diagnostics)
  - Automatic handling of progress bars and screen repaints
  - Alternate screen programs (vim, less) excluded from logs
  - Comprehensive logging without duplicates

### 🚧 In Progress

- **M4: Multi-Tab Session Management** (Next)
  - Multiple concurrent SSH sessions in tabs
  - Tab bar with visual indicators
  - Independent sessions per tab
  - Background session maintenance
  - Per-tab logging

### 📋 Planned

- **M5: Advanced Features**
  - SFTP file transfer integration
  - Session templates and profiles
  - Custom key bindings
  - Color scheme customization
  - Font configuration

- **M6: Enhanced Security**
  - SSH key management
  - Certificate-based authentication
  - Session recording and playback
  - Audit logging

## Technology Stack

### Frontend
- **SvelteKit**: Reactive UI framework
- **TypeScript**: Type-safe development
- **xterm.js**: Terminal emulation
- **Vite**: Build tool and dev server

### Backend
- **Rust**: High-performance, memory-safe backend
- **Tauri**: Native desktop application framework
- **chacha20poly1305**: Vault encryption
- **argon2**: Password-based key derivation
- **portable-pty**: Cross-platform PTY implementation
- **tokio**: Async runtime
- **chrono**: Date/time handling for logs

## Project Structure

```
sshterm/
├── src/                    # Frontend (SvelteKit)
│   ├── lib/
│   │   ├── components/    # Svelte components
│   │   ├── i18n/         # Translations
│   │   ├── encoding.ts   # Base64 utilities
│   │   ├── logCapture.ts # Terminal logging
│   │   └── sessionBus.ts # Session event handling
│   ├── routes/           # SvelteKit routes
│   └── app.html          # HTML template
├── src-tauri/            # Backend (Rust)
│   ├── src/
│   │   ├── askpass/      # SSH password delivery
│   │   ├── config.rs     # App configuration
│   │   ├── logger/       # Session logging
│   │   ├── paths.rs      # File path management
│   │   ├── session/      # SSH session management
│   │   ├── store/        # Connection storage
│   │   ├── transport/    # SSH transport layer
│   │   └── vault/        # Encrypted credential vault
│   ├── Cargo.toml        # Rust dependencies
│   └── tauri.conf.json   # Tauri configuration
├── static/               # Static assets
└── package.json          # Node dependencies
```

## Development

### Prerequisites

- **Node.js** 18+ and npm
- **Rust** 1.70+ (rustup recommended)
- **Windows**: Visual Studio Build Tools or MSVC
- **SSH client**: OpenSSH (typically pre-installed on Windows 10+)

### Setup

```bash
# Clone repository
git clone <repository-url>
cd sshterm

# Install frontend dependencies
npm install

# Build and run in development mode
npm run tauri dev
```

### Building

```bash
# Production build
npm run tauri build

# Output:
# - MSI installer: src-tauri/target/release/bundle/msi/
# - NSIS installer: src-tauri/target/release/bundle/nsis/
# - Portable exe: src-tauri/target/release/sshterm.exe
```

### Testing

```bash
# Run Rust tests
cd src-tauri
cargo test

# Run specific test module
cargo test logger
cargo test vault
```

## Architecture

### Security Model

1. **Vault Encryption**
   - Master password → Argon2id → 256-bit key
   - Each connection encrypted separately with AES-256-GCM
   - Vault file: chacha20poly1305 with random nonce
   - Zero memory copies of sensitive data (zeroize crate)

2. **Password Delivery**
   - SSH passwords never touch command line
   - Named pipe (Windows) with random token
   - Askpass server validates token before responding
   - Pipe closed after single use

3. **Session Isolation**
   - Each session = separate ssh.exe process
   - Independent PTY per session
   - Separate log files
   - No cross-session data leakage

### Logging Architecture (M3)

```
┌─────────────────────────────────────────────────────────────┐
│  xterm.js Terminal Buffer (frontend)                         │
│  - Correctly interprets PTY redraws and cursor movements     │
│  - Each line exists exactly once                             │
└────────────────────┬────────────────────────────────────────┘
                     │ LogCapture monitors buffer
                     │ Sends complete lines only
                     ↓
┌─────────────────────────────────────────────────────────────┐
│  SessionLogger (backend)                                     │
│  - write_text() for Screen mode (default)                   │
│  - write_raw() for Raw mode (diagnostics)                   │
│  - Timestamps: YYYYMMDD-HHMMSS format                       │
└────────────────────┬────────────────────────────────────────┘
                     ↓
              Log File: logs/server_timestamp.log
```

**Why this approach:**
- ConPTY sends screen redraws, not raw terminal bytes
- Stripping escape codes from redraws creates duplicates
- xterm.js maintains correct screen state
- Logging from screen buffer gives clean, user-visible text

### Session Management (M2)

```
Frontend (TerminalPane) ←→ IPC ←→ Backend (SessionState)
                                         ↓
                                    HashMap<SessionId, Transport>
                                         ↓
                            ┌────────────┴─────────────┐
                            ↓                          ↓
                     OpenSshTransport              SessionLogger
                            ↓                          ↓
                    ConPTY + ssh.exe              Log File
```

## Configuration

Application data is stored in:
- **Windows**: `%LOCALAPPDATA%\sshterm\`
- **Linux**: `~/.local/share/sshterm/`
- **macOS**: `~/Library/Application Support/sshterm/`

### Files

- `vault.dat`: Encrypted connections and passwords
- `config.toml`: Application settings (language, log mode)
- `known_hosts`: SSH host key fingerprints
- `logs/`: Session logs directory
  - `sshterm.log`: Application diagnostics
  - `<connection>_<timestamp>.log`: Per-session logs

### config.toml

```toml
language = "en"              # or "es"
log_output = true            # Enable session logging
log_mode = "screen"          # "screen" or "raw"
```

## Milestones

### M0: Foundation (✅ Complete)
- Tauri project setup
- Basic application window
- SvelteKit integration

### M1: Vault & i18n (✅ Complete)
- Encrypted credential storage
- Password-based encryption
- Multi-language UI
- Connection CRUD operations

### M2: SSH Sessions (✅ Complete)
- SSH connection establishment
- Terminal emulation
- PTY management
- Password delivery via askpass
- Host key verification

### M3: Logging (✅ Complete)
- Session log files
- Screen buffer capture
- Clean text output
- Timestamp formatting
- Log rotation handling

### M4: Multi-Tab (🚧 In Progress)
- Tab bar component
- Multiple concurrent sessions
- Tab switching and management
- Per-tab logging
- Background session maintenance

### M5: Advanced Features (📋 Planned)
- SFTP integration
- Session profiles
- Customization options
- Enhanced UI/UX

### M6: Security Enhancements (📋 Planned)
- SSH key management
- Certificate authentication
- Session recording
- Audit logging

## Contributing

This is currently a personal project. Contributions, suggestions, and bug reports are welcome.

## License

[License information to be added]

## Acknowledgments

- Built with [Tauri](https://tauri.app/)
- Terminal emulation by [xterm.js](https://xtermjs.org/)
- UI framework: [SvelteKit](https://kit.svelte.dev/)
- Icons: [Lucide](https://lucide.dev/)

## Security Notice

- Never share your vault file or master password
- Review host key fingerprints on first connection
- Session logs may contain sensitive information
- Keep the application and dependencies updated

## Troubleshooting

### Session not connecting
- Verify SSH server is reachable
- Check username and password
- Review known_hosts for key changes
- Check logs/sshterm.log for details

### Log files too large
- Set `log_mode = "screen"` in config.toml for cleaner logs
- Use `log_output = false` to disable logging
- Manually clean old logs from logs/ directory

### Vault unlock fails
- Verify master password is correct
- Check vault.dat is not corrupted
- Backup vault.dat before attempting recovery

## Contact

[Contact information to be added]
