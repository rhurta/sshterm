# SSHTerm Development Roadmap

## Current Status: M3 Complete → M4 In Progress

Last Updated: November 2026

---

## ✅ M0: Project Foundation (Complete)

**Goal**: Establish project structure and tooling

### Deliverables
- [x] Tauri + SvelteKit project setup
- [x] TypeScript configuration
- [x] Build system (Vite)
- [x] Basic application window
- [x] Development workflow

### Technical Details
- Framework: Tauri 2.0 + SvelteKit 2.0
- Build: Vite 5.0
- Package manager: npm
- Target: Windows (primary), Linux/macOS (future)

---

## ✅ M1: Vault & Internationalization (Complete)

**Goal**: Secure credential storage and multi-language support

**Commit**: `bd4d6b4` (tag: m1)

### Security Implementation
- **Encryption**: AES-256-GCM via chacha20poly1305 crate
- **Key Derivation**: Argon2id (64 MiB memory, ~200ms)
  - Salt: 16 random bytes per vault
  - Output: 256-bit key
- **Data Model**:
  ```rust
  struct Connection {
      id: String,
      name: String,
      host: String,
      port: u16,
      user: String,
      password: Option<String>,
      keepalive_secs: u16,
  }
  ```
- **Vault Format**: TOML encrypted as single blob
- **Memory Safety**: zeroize crate for automatic secret clearing

### i18n Implementation
- **Languages**: English (en), Spanish (es)
- **Architecture**: 
  - TypeScript translation dictionaries
  - Reactive language switching
  - Config persists language preference
- **Coverage**: All UI strings

### Deliverables
- [x] Vault encryption/decryption
- [x] Master password validation
- [x] Password change with re-encryption
- [x] Connection CRUD operations
- [x] Language selector
- [x] Translation files (en, es)
- [x] Configuration file (config.toml)

### Testing
- [x] Vault crypto tests
- [x] Password verification
- [x] Round-trip encryption
- [x] Connection storage

**Status**: Deployed and stable

---

## ✅ M2: SSH Session Management (Complete)

**Goal**: Establish and manage SSH connections with terminal emulation

**Commit**: `2c19e40` (tag: m2)

### Architecture

```
Frontend (Svelte)           Backend (Rust)              System
     │                           │                         │
     ├─ session_open ──────────> │                         │
     │                           ├─ Create Transport       │
     │                           ├─ Start askpass server   │
     │                           ├─ Open ConPTY ──────────> ssh.exe
     │                           ├─ Spawn reader thread    │
     │                           └─ Return session ID      │
     │                                                      │
     │ <────── session:data ──── PTY Reader Thread ────< ssh.exe
     │                                                      │
     ├─ session_write ─────────> │                         │
     │                           └─ Write to PTY ─────────> ssh.exe
     │                                                      │
     ├─ session_resize ────────> │                         │
     │                           └─ Resize ConPTY          │
```

### Key Components

1. **OpenSSH Integration**
   - Uses system ssh.exe (Windows OpenSSH)
   - Flags: `-tt` (force TTY), host key policy, timeouts
   - No password on command line (security)

2. **Password Delivery (Askpass)**
   - Named pipe: `\\.\pipe\sshterm-askpass-{random}`
   - Single-use token validation
   - Automatic cleanup after use
   - Environment: `SSH_ASKPASS`, `SSH_ASKPASS_REQUIRE=force`

3. **PTY Management**
   - Windows ConPTY (portable-pty crate)
   - Dedicated reader thread (blocking I/O)
   - 8KB buffer size
   - Base64 encoding for IPC

4. **EOF Handling**
   - Watchdog thread monitors ssh.exe exit
   - 100ms polling interval
   - Closes PTY master to unblock reader
   - Sends empty chunk as EOF sentinel

5. **Terminal Emulation**
   - xterm.js with canvas renderer
   - FitAddon for responsive sizing
   - Configurable cols/rows
   - Full ANSI support

### SSH Configuration
- **Host Keys**: Custom known_hosts file
- **Policy**: StrictHostKeyChecking=accept-new (TOFU)
- **Keep-alive**: ServerAliveInterval + CountMax=3
- **Timeout**: ConnectTimeout=15 seconds

### Deliverables
- [x] SSH connection establishment
- [x] PTY creation and management
- [x] Password delivery via askpass
- [x] Host key verification
- [x] Terminal emulation (xterm.js)
- [x] Session resize handling
- [x] Clean session termination
- [x] EOF watchdog for ConPTY

### Testing
- [x] SSH command flag validation
- [x] Askpass environment setup
- [x] PTY size forwarding
- [x] ConPTY output capture (manual)

**Status**: Deployed and stable

---

## ✅ M3: Session Logging (Complete)

**Goal**: Comprehensive, readable session logs without duplicates

**Commits**: 
- `952e5cd` (tag: m3) - Initial logging
- `248b264` - Blank line collapsing
- `bbc0262` - Screen buffer capture (current)

### The Problem (Solved)

**Original Approach** (M3 initial):
- Logged PTY bytes with ANSI stripping
- **Issue**: ConPTY sends screen redraws, not raw bytes
  - Full repaints on resize/scroll
  - Partial updates with cursor positioning
  - Same content sent multiple times
- **Result**: Logs had duplicate MOTD, repeated lines, text fragments

**Root Cause**: Stripping escape codes removed cursor positioning info that explained the redraws. The log couldn't be reconstructed correctly.

### The Solution (M3 fix)

**New Approach**: Log from xterm.js screen buffer
- xterm.js correctly interprets all PTY redraws
- Each line exists exactly once in the buffer
- Lines written only when cursor moves past them
- Progress bars and redraws never reach log

### Architecture

```
PTY bytes with redraws ──> xterm.js ──> Screen Buffer (clean state)
                                              │
                                        LogCapture.ts
                                        - Monitors buffer
                                        - Detects complete lines
                                        - Batches writes (250ms)
                                              │
                                              ↓
                              session_log_append command
                                              │
                                              ↓
                              SessionLogger (Rust)
                              - write_text() or write_raw()
                              - Collision detection
                              - Timestamp formatting
                                              │
                                              ↓
                              logs/server_YYYYMMDD-HHMMSS.log
```

### Features

1. **Log Modes**
   - `Screen` (default): Clean text from terminal display
   - `Raw`: PTY bytes with escape codes (diagnostics)

2. **LogCapture Logic**
   - Marker tracking (survives scroll/reflow)
   - Screen erase detection (ESC[J, RIS)
   - Repaint recognition via recent line history
   - Alternate screen detection (vim, less, top)
   - Blank line collapsing (max one between blocks)

3. **Logger Features**
   - Readable timestamps: `YYYYMMDD-HHMMSS`
   - Collision handling: `_2`, `_3` suffixes
   - Closing timestamp when session ends
   - chrono crate for local time

4. **Configuration**
   ```toml
   log_output = true      # Enable/disable logging
   log_mode = "screen"    # "screen" or "raw"
   ```

### Deliverables
- [x] SessionLogger with dual modes
- [x] LogCapture class (TypeScript)
- [x] Screen buffer monitoring
- [x] Alternate screen filtering
- [x] Readable timestamp format
- [x] File collision detection
- [x] Config option for log mode
- [x] TerminalPane integration
- [x] Final output capture on close

### Testing
- [x] File name sanitization
- [x] Timestamp format validation
- [x] Screen vs Raw mode separation
- [x] Collision handling
- [x] No file created when log_output=false
- [x] Manual: SSH session with MOTD (no duplicates)
- [x] Manual: Progress bars don't pollute log
- [x] Manual: vim/less don't appear in log

**Status**: Deployed and stable

---

## 🚧 M4: Multi-Tab Session Management (In Progress)

**Goal**: Support multiple concurrent SSH sessions in tabbed interface

**Target**: Next release

### Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  Workspace Component                     │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Tab Bar                                           │ │
│  │  [● server1 ×] [server2 ×] [server3 ×]  [+]      │ │
│  └────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────┐ │
│  │  Active TerminalPane (visible)                     │ │
│  │  session_id: "abc123"                             │ │
│  └────────────────────────────────────────────────────┘ │
│  Hidden TerminalPanes (keep running):                  │
│  - session_id: "def456"                               │
│  - session_id: "ghi789"                               │
└─────────────────────────────────────────────────────────┘
```

### State Management

```typescript
interface Tab {
  id: string;           // Session ID
  connId: string;       // Vault connection ID  
  name: string;         // Display name
  active: boolean;      // Currently visible
}

interface WorkspaceState {
  tabs: Tab[];
  activeTabId: string;
}
```

### Key Features

1. **Tab Management**
   - New tab: [+] button or connection selector
   - Switch tab: Click tab → show that TerminalPane
   - Close tab: [×] button → session_close + remove
   - Active indicator: Visual highlight on current tab

2. **Session Behavior**
   - Each tab = unique session ID
   - Hidden tabs keep running (PTY still active)
   - Only active tab triggers PTY resize
   - All tabs log continuously
   - Can open same server multiple times

3. **UI Components**
   - `Workspace.svelte`: Tab bar + content area
   - `TabBar.svelte`: Tab list + new tab button
   - `Tab.svelte`: Individual tab with close button
   - `TerminalPane.svelte`: Modified for visibility prop

### Backend (Already Ready!)

The current `SessionState` already supports multiple sessions:
```rust
pub struct SessionState {
    pub transports: Mutex<HashMap<String, Box<dyn Transport>>>,
    pub loggers: Mutex<HashMap<String, Arc<Mutex<SessionLogger>>>>,
}
```

No backend changes needed! Just frontend work.

### Data Flow

**Opening Tab**:
```
User clicks connection → session_open → new session ID
  → Create tab in state → Mount TerminalPane → Set active
```

**Switching Tab**:
```
User clicks Tab 2 → Set active=true for Tab 2, active=false for others
  → Show Tab 2 TerminalPane → Hide others → syncSize() on Tab 2
```

**Closing Tab**:
```
User clicks [×] → onclose callback → session_close(id)
  → Kill ssh.exe → logger.finish() → Remove tab from state
  → If last tab: show connection selector
```

### Implementation Plan

#### Phase 1: Core Tab Management
- [ ] Create `Workspace.svelte` component
- [ ] Implement tab state management
- [ ] Tab bar UI with active indicator
- [ ] New tab button
- [ ] Close tab button per tab

#### Phase 2: Integration
- [ ] Modify `TerminalPane.svelte` for visibility
- [ ] Only active terminal resizes PTY
- [ ] Connection selector for new tabs
- [ ] Handle last tab closed scenario

#### Phase 3: Polish
- [ ] Tab switching animations
- [ ] Keyboard shortcuts (Ctrl+Tab, Ctrl+W)
- [ ] Tab reordering (drag & drop)
- [ ] Tab overflow handling (scrollable)

#### Phase 4: Testing
- [ ] Multiple sessions to same server
- [ ] Rapid tab switching
- [ ] Close active tab
- [ ] Close non-active tab
- [ ] Logging continues when hidden
- [ ] Memory usage with 10+ tabs

### Deliverables
- [ ] Workspace component with tab bar
- [ ] Tab state management
- [ ] Multi-session UI
- [ ] Keyboard shortcuts
- [ ] Visual polish
- [ ] Documentation

**Status**: Design complete, implementation starting

---

## 📋 M5: Advanced Features (Planned)

**Goal**: Enhanced functionality and user experience

### Proposed Features

1. **SFTP Integration**
   - File browser panel
   - Upload/download files
   - Progress indication
   - Drag-and-drop support

2. **Session Profiles**
   - Saved terminal size preferences
   - Custom color schemes per connection
   - Font size settings
   - Command history

3. **Key Bindings**
   - Customizable shortcuts
   - Common operations (copy, paste, search)
   - Tab management shortcuts

4. **Appearance**
   - Multiple color schemes (Solarized, Monokai, etc.)
   - Font family selection
   - Font size adjustment
   - Cursor style options

5. **Quality of Life**
   - Search in terminal output
   - Copy/paste improvements
   - URL detection and opening
   - Connection groups/folders

### Technical Considerations
- Store preferences in config.toml
- Per-connection settings in vault
- Color scheme JSON format
- Font configuration validation

**Status**: Requirements gathering

---

## 📋 M6: Security Enhancements (Planned)

**Goal**: Advanced authentication and auditing

### Proposed Features

1. **SSH Key Management**
   - Import existing keys
   - Generate new key pairs
   - Key-based authentication
   - Agent forwarding
   - Certificate support

2. **Security Audit**
   - Connection attempt logging
   - Failed authentication tracking
   - Session duration recording
   - Export audit logs

3. **Session Recording**
   - Record terminal sessions
   - Playback capability
   - TTYrec format support
   - Privacy controls

4. **Enhanced Encryption**
   - YubiKey support
   - Hardware security module integration
   - Biometric unlock (Windows Hello)
   - Auto-lock on idle

### Technical Considerations
- SSH key storage format
- Audit log database
- TTYrec format implementation
- Platform-specific security APIs

**Status**: Research phase

---

## Future Considerations

### M7+: Cross-Platform
- Linux support
- macOS support
- Platform-specific SSH client integration
- Native PTY handling per platform

### M8+: Advanced Networking
- Jump host / bastion support
- Port forwarding UI
- SOCKS proxy
- VPN integration

### M9+: Team Features
- Shared connection vaults
- Team authentication
- Connection templates
- Centralized management

---

## Version History

| Milestone | Version | Date | Status |
|-----------|---------|------|--------|
| M0 | 0.1.0 | - | ✅ Complete |
| M1 | 0.2.0 | - | ✅ Complete |
| M2 | 0.3.0 | - | ✅ Complete |
| M3 | 0.4.0 | Nov 2026 | ✅ Complete |
| M4 | 0.5.0 | TBD | 🚧 In Progress |
| M5 | 0.6.0 | TBD | 📋 Planned |
| M6 | 0.7.0 | TBD | 📋 Planned |

---

## Contributing

This roadmap is subject to change based on:
- User feedback
- Technical discoveries
- Security requirements
- Platform limitations

Suggestions and feedback are welcome!
