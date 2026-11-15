# M4: Multi-Tab Session Management Architecture

> **Historical design note (M3).** Multi-tab was implemented in M3 in `src/lib/Workspace.svelte`
> (tabs array, `activeId`, one mounted `TerminalPane` per session, hidden panes keep running,
> only the active pane resizes the PTY). M4 kept that design, added focus-on-open/switch in
> `TerminalPane.svelte`, and spent its effort on the session tree instead:
> typed vault model, tree CRUD/search, imports, sidebar UI and the connection editor.
> The backend `SessionState` HashMap needed no changes.

## Overview
M4 adds the ability to open multiple SSH sessions simultaneously, each in its own tab within the application window.

## Component Diagram

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           SSHTerm Application Window                         │
│ ┌─────────────────────────────────────────────────────────────────────────┐ │
│ │                        Workspace Component                               │ │
│ │  ┌────────────────────────────────────────────────────────────────────┐ │ │
│ │  │  Tab Bar                                                            │ │ │
│ │  │  ┌──────────┐ ┌──────────┐ ┌──────────┐ ┌──────────┐  [+]        │ │ │
│ │  │  │ Tab 1    │ │ Tab 2    │ │ Tab 3    │ │ Tab 4    │              │ │ │
│ │  │  │ server1  │ │ server2  │ │ server1  │ │ server3  │              │ │ │
│ │  │  │  [x]     │ │  [x]     │ │  [x]     │ │  [x]     │              │ │ │
│ │  │  └──────────┘ └──────────┘ └──────────┘ └──────────┘              │ │ │
│ │  └────────────────────────────────────────────────────────────────────┘ │ │
│ │                                                                           │ │
│ │  ┌────────────────────────────────────────────────────────────────────┐ │ │
│ │  │  Active Terminal Pane (visible)                                    │ │ │
│ │  │  ┌──────────────────────────────────────────────────────────────┐ │ │ │
│ │  │  │ xterm.js terminal                                            │ │ │ │
│ │  │  │                                                              │ │ │ │
│ │  │  │ user@server1:~$                                             │ │ │ │
│ │  │  │                                                              │ │ │ │
│ │  │  │                                                              │ │ │ │
│ │  │  │                                                              │ │ │ │
│ │  │  └──────────────────────────────────────────────────────────────┘ │ │ │
│ │  │  Session ID: abc123                                                │ │ │
│ │  │  LogCapture active                                                 │ │ │
│ │  └────────────────────────────────────────────────────────────────────┘ │ │
│ │                                                                           │ │
│ │  Hidden Terminal Panes (keep running in background)                      │ │
│ │    - Session def456 (server2)                                            │ │
│ │    - Session ghi789 (server1)                                            │ │
│ │    - Session jkl012 (server3)                                            │ │
│ └───────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘

                                    ↕ IPC Commands
                                    
┌─────────────────────────────────────────────────────────────────────────────┐
│                           Tauri Backend (Rust)                               │
│ ┌─────────────────────────────────────────────────────────────────────────┐ │
│ │  SessionState                                                            │ │
│ │  ┌────────────────────────────────────────────────────────────────────┐ │ │
│ │  │ transports: HashMap<SessionId, Transport>                          │ │ │
│ │  │   "abc123" → OpenSshTransport (server1, user1)                    │ │ │
│ │  │   "def456" → OpenSshTransport (server2, user2)                    │ │ │
│ │  │   "ghi789" → OpenSshTransport (server1, user1)  # 2nd connection │ │ │
│ │  │   "jkl012" → OpenSshTransport (server3, user3)                    │ │ │
│ │  └────────────────────────────────────────────────────────────────────┘ │ │
│ │  ┌────────────────────────────────────────────────────────────────────┐ │ │
│ │  │ loggers: HashMap<SessionId, Logger>                                │ │ │
│ │  │   "abc123" → SessionLogger (logs/server1_20241114-120000.log)    │ │ │
│ │  │   "def456" → SessionLogger (logs/server2_20241114-120005.log)    │ │ │
│ │  │   "ghi789" → SessionLogger (logs/server1_20241114-120010_2.log)  │ │ │
│ │  │   "jkl012" → SessionLogger (logs/server3_20241114-120015.log)    │ │ │
│ │  └────────────────────────────────────────────────────────────────────┘ │ │
│ └─────────────────────────────────────────────────────────────────────────┘ │
│                                                                               │
│ ┌─────────────────────────────────────────────────────────────────────────┐ │
│ │  Per Session: OpenSshTransport + PTY                                    │ │
│ │  ┌────────────────┐    ┌─────────────┐    ┌─────────────┐             │ │
│ │  │  PTY Reader    │───▶│ Event Bus   │───▶│  Frontend   │             │ │
│ │  │  Thread        │    │ session:data│    │  Terminal   │             │ │
│ │  └────────────────┘    └─────────────┘    └─────────────┘             │ │
│ │         ▲                                         │                      │ │
│ │         │                                         ▼                      │ │
│ │  ┌────────────────┐                      ┌─────────────┐               │ │
│ │  │  ConPTY +      │◀─────────────────────│session_write│               │ │
│ │  │  ssh.exe       │                      └─────────────┘               │ │
│ │  └────────────────┘                                                     │ │
│ └─────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Data Flow

### Opening a New Tab
```
User clicks connection → Frontend → session_open
                                         ↓
                                   New SessionId generated
                                         ↓
                         ┌───────────────┴──────────────┐
                         ↓                              ↓
                  Create Transport                Create Logger
                  Start ssh.exe                  Open log file
                         ↓                              ↓
                  Spawn PTY reader               Setup LogCapture
                         ↓                              ↓
                  Return SessionId ←───────────────────┘
                         ↓
                  Create Tab Component
                  Mount TerminalPane
```

### Tab Switching
```
User clicks Tab 2 → Set active = true for Tab 2
                    Set active = false for Tab 1
                         ↓
                Tab 2 TerminalPane becomes visible
                Tab 1 TerminalPane hidden (but keeps running)
                         ↓
                Tab 2 terminal calls syncSize()
                Resizes PTY for session_id_2
                         ↓
                LogCapture continues in background for all tabs
```

### Closing a Tab
```
User clicks [x] on Tab → onclose callback
                              ↓
                        Frontend cleanup
                              ↓
                        session_close(id)
                              ↓
                    ┌─────────┴─────────┐
                    ↓                   ↓
              Kill ssh.exe        logger.finish()
              Close PTY           Write end timestamp
                    ↓                   ↓
              Remove from         Close log file
              transports map      Remove from loggers map
                    ↓                   ↓
              Emit session:closed event
                              ↓
                    Remove tab from UI
```

## Key Components

### Frontend (Svelte)

```
Workspace.svelte
├── Connection Selector (when no tabs open)
├── Tab Bar
│   ├── Tab[] (one per active session)
│   │   ├── Label (connection name)
│   │   ├── Close button [x]
│   │   └── Active indicator
│   └── [+] New Tab button
└── Tab Content
    └── TerminalPane[] (one per session)
        ├── Visibility controlled by active prop
        ├── xterm.js terminal instance
        └── LogCapture (always active, even when hidden)
```

### State Management

```typescript
// Workspace state
{
  tabs: [
    { 
      id: "abc123",           // Session ID
      connId: "conn-server1", // Vault connection ID
      name: "server1",        // Display name
      active: true            // Currently visible
    },
    { 
      id: "def456", 
      connId: "conn-server2", 
      name: "server2", 
      active: false 
    }
  ],
  activeTabId: "abc123"
}
```

### Backend (Rust)

```rust
// SessionState (already exists, just holds multiple sessions)
pub struct SessionState {
    pub transports: Mutex<HashMap<String, Box<dyn Transport>>>,
    pub loggers: Mutex<HashMap<String, Arc<Mutex<SessionLogger>>>>,
}

// No backend changes needed - already supports multiple sessions!
```

## Features

### Tab Management
- **New Tab**: Click [+] or select connection → opens new session
- **Switch Tab**: Click tab → shows that terminal, resizes PTY
- **Close Tab**: Click [x] → closes session, kills ssh.exe, closes log
- **Multiple connections to same server**: Each gets unique session ID and log file

### Terminal Behavior
- **Hidden tabs keep running**: PTY output buffered by xterm.js
- **Switch tab**: Immediate display of current state
- **Resize**: Only active terminal triggers PTY resize
- **Logging**: All tabs log continuously regardless of visibility

### Session Isolation
- Each tab = independent SSH session
- Separate PTY, separate ssh.exe process
- Separate log file
- Can connect to same server multiple times

## Implementation Notes

### Why This Works with Current Backend
The backend already supports multiple sessions through the `HashMap` in `SessionState`. Each `session_open` call creates a new entry. The frontend just needs to:
1. Track multiple tabs
2. Show/hide TerminalPane based on active tab
3. Map tab clicks to session IDs

### Logging
- Each session gets its own log file
- LogCapture runs for all sessions (even hidden tabs)
- Log file created when session starts
- Closed when tab is closed

### Performance
- Hidden terminals don't render but xterm.js keeps buffer updated
- PTY reader threads run independently
- Only active terminal triggers resize operations
- Memory: ~10-20MB per terminal + xterm buffer

### Edge Cases
- **Last tab closed**: Show connection selector again
- **Duplicate connection**: Allowed, each gets unique session + log
- **Vault locked**: Close all tabs (existing `session_close_all`)
- **App closed**: All PTY watchdogs kill ssh.exe processes

## User Experience Flow

```
1. App starts → Unlock vault → Connection selector shown
                                        ↓
2. Select connection → First tab created, terminal opens
                                        ↓
3. Click [+] → Connection selector shown in modal/dropdown
                                        ↓
4. Select another connection → New tab created
                                        ↓
5. Switch between tabs → Instant display of each terminal
                                        ↓
6. Close tab [x] → Tab removed, session ends
                                        ↓
7. Close last tab → Back to connection selector
```

## Visual Design

```
┌─────────────────────────────────────────────────────────────┐
│ SSHTerm                                          [_] [□] [×] │
├─────────────────────────────────────────────────────────────┤
│ ┌───────────┬───────────┬───────────┬──────┐               │
│ │● server1  │  server2  │  server3  │  [+] │               │
│ │     [×]   │     [×]   │     [×]   │      │               │
│ └───────────┴───────────┴───────────┴──────┘               │
├─────────────────────────────────────────────────────────────┤
│                                                             │
│  user@server1:~$ █                                         │
│                                                             │
│                                                             │
│                                                             │
│                                                             │
│                                                             │
│                                                             │
└─────────────────────────────────────────────────────────────┘

● = Active tab indicator (colored dot or underline)
[×] = Close button per tab
[+] = New tab button
```

## Summary

M4 is primarily a **frontend-only feature** that leverages the existing backend's ability to handle multiple sessions. The main work is:

1. **Workspace component**: Tab bar + connection selector logic
2. **Tab state management**: Track sessions, active tab
3. **TerminalPane visibility**: Show/hide based on active tab
4. **Resize coordination**: Only active terminal resizes PTY

The backend already supports everything needed - we just expose it through a multi-tab UI.
