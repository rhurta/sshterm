# M6 — Tiling, command console and broadcast

This document is the implementation plan for M6. It deliberately separates workspace layout, command composition and command delivery so each stage can be tested independently.

## Goals

- Show multiple live SSH sessions in one workspace using horizontal, vertical and 2×2 tiling.
- Keep session tabs inside each tiled pane. Opening a connection adds a tab to the currently focused pane; switching a pane tab does not affect other panes.
- Provide a persistent command console below the terminal area, inspired by SecureCRT's command window.
- Compose and edit one or many command lines before sending. Enter inserts a newline; it never executes by itself.
- Send the composed text to the active session, explicitly selected sessions, or all eligible open sessions.
- Keep the editor and dispatch path reusable for named, saved command sequences in a future milestone; saved commands themselves are out of scope for M6.

## UX and interaction decisions

### Workspace layout

- The main workspace consists of a terminal layout region and an optional command-console region.
- Terminal layouts are represented as a small split tree, not a fixed grid: a leaf owns one session ID; a split owns an axis, ratio and two child nodes.
- Supported presets: single pane, left/right, top/bottom and 2×2. Splitters are draggable and keyboard accessible.
- A session can appear in only one leaf. Closing its last tab leaves the pane empty; the user can explicitly remove an empty pane, which collapses its parent split.
- Existing tabs remain the session switcher. Selecting a tab focuses its leaf; if it is not currently tiled, the single-pane layout switches to it rather than silently duplicating it.
- xterm FitAddon and PTY resize must run after a leaf becomes visible and after its dimensions settle. Hidden leaves must not resize their PTYs to zero or stale dimensions.
- Make the focused pane immediately recognizable without adding a text label: use a thin accent outline around the pane and a small bright dot on its active session tab. The pane cue follows the active terminal session; the command-console list separately marks the current target set.
- Do not persist the split tree or add a vault migration in M6. Persist only command-console visibility and height in the non-secret config after interaction is validated.

### Command console

- The console is a dedicated bottom pane with a multiline editor, draggable height divider, target selector, Send and Clear actions.
- Enter inserts a newline. Ctrl+Enter sends. The Send button is always available.
- The editor preserves its contents after sending; clearing is explicit.
- Pasting multi-line text into the editor does not execute it. The existing terminal multi-line-paste confirmation remains specific to pasting into a terminal.
- Sending is a separate action and requires a visible target summary and confirmation when the payload contains multiple lines or targets more than one session.
- The console is available with one or many terminal panes and can be collapsed without losing its draft.

### Broadcast safety

- Initial targets: active session, explicitly selected sessions, or all open sessions.
- Show target names/count immediately above the editor and re-evaluate eligibility at send time. The searchable connection list is in a fixed left column beside the editor.
- Add the protected flag to the credential-free connection summary. Protected connections are excluded from broadcast by default.
- Provide a deliberate opt-in to include protected sessions; never silently include them.
- Do not infer success from IPC dispatch. Report how many sessions accepted the write request and surface per-session failures.
- Send the composed bytes exactly as entered. The user controls command terminators; do not silently append a newline or execute a partial last line.
- Preserve per-session ordering. A broadcast is a sequence of writes to each selected PTY, not a shell-level transaction; commands can succeed on some hosts and fail on others.
- No automatic retries: repeating a command may be destructive.

## Implementation phases

### M6.1 — Tiling foundation

**Core tiling and pane-tab UX implemented.** The focused pane now has a subtle accent outline and a small bright dot on its active tab instead of a prominent text label. The workspace renders a recursive split tree with single, left/right, top/bottom and 2×2 presets. Splitters are draggable and keyboard accessible. Each pane has its own session tab strip; opening a connection adds it to the focused pane, and switching tabs is isolated to that pane. Existing TerminalPane instances remain keyed by session ID so switching pane tabs does not recreate live xterm sessions. Pure split-tree helpers cover insertion, tab removal, collapse, pane tab grouping, balanced presets, geometry and path-addressed ratio updates; tests run in Windows CI. The layout presets and explicit horizontal/vertical split actions are in a vertical side toolbar. Splitting creates an empty pane; its in-pane plus action opens the connection picker for that pane. Closing the last tab leaves its pane empty. Terminal viewport scrollbars explicitly mirror the connection-tree scrollbar treatment. The pane connection picker includes a live search field; the connection tree and ProxyJump selector also have search.
### M6.1 — Remaining work

- Automated UI-level tests for pane tab switching, close behavior, focus and resize remain follow-up work. Pure split-tree tests run in Windows CI; interactive checks have been performed during development.

### M6.1 — Tiling foundation (original checklist)

1. Introduce a typed frontend split-tree model and pure helpers for insert, remove, collapse and traversal.
2. Add layout presets and active-leaf focus while preserving the existing session tabs.
3. Render each visible leaf as an independent TerminalPane; keep one terminal instance per live session.
4. Add draggable splitters and debounce FitAddon/PTY resize after layout changes.
5. Test tree operations, close behavior, tab switching, focus and repeated resizing.

Acceptance: one, two and four live sessions can be arranged, resized and closed without losing session state or clipping terminal rows.

### M6.2 — Command console

**Command-console composition and dispatch implemented.** A reusable CommandConsole component is mounted independently of the tiled pane tree. Its toolbar toggle hides or shows the console without discarding the in-memory draft. The bottom pane has a draggable, keyboard-adjustable height divider, multiline editor, active-session target summary, Send and Clear. Enter inserts a newline; Ctrl+Enter or Send requests dispatch. Multi-line payloads require a second explicit confirmation. Dispatch uses the existing session_write command with the exact UTF-8 bytes encoded as base64; no newline is appended, and the UI reports IPC dispatch rather than claiming remote command completion. Drafts remain in frontend memory and are not persisted.

Follow-up:
- Component-level UI tests for send gating and payload preservation are not currently part of the test setup.
- Core console composition, paste, explicit send and hide/show behavior were exercised in the Windows app. Broadcast target-specific acceptance is tracked under M6.3.

Selected/all target modes are covered in M6.3.

Acceptance: multiline drafts can be composed, pasted, edited and sent explicitly; Enter alone never sends.

### M6.3 — Broadcast targeting and safeguards

**Target selection and safe dispatch implemented; pure target-resolution tests run in Windows CI.**

- The credential-free Rust `ConnectionSummary` now exposes the saved `protected` flag, and each open frontend session retains that flag in its tab metadata.
- The console supports Active session, Selected sessions and All open sessions.
- Selected mode defaults to the active session when first entered; users can add/remove open sessions individually.
- Protected connections are excluded from Selected/All by default. An explicit "Include protected connections" opt-in is shown when protected sessions are open. Active-session mode remains a deliberate single-session action.
- The target count and names are displayed above the editor, including the number of protected sessions excluded.
- Multi-line sends and sends to more than one session require a second confirmation that names the target set and explains that each host receives the bytes independently.
- Targets are resolved again at dispatch time. Writes are sent sequentially, preserve exact UTF-8 bytes, do not append a newline, and are never retried automatically.
- The console reports the number of accepted write requests and each per-session dispatch failure. It does not claim remote command completion.
- Pure target-resolution tests cover active, selected, all, protected opt-in/exclusion, stale IDs and duplicate IDs; Windows CI runs these tests.

Follow-up:
- Explicit Windows UI acceptance is still required for all three target modes, protected opt-in, multi-line/multi-session confirmation and partial failures.
- There is no integration-level dispatch test because the current app test setup has no session-write test seam.

Acceptance: target resolution is tested for stale/closed sessions and protected exclusion; the UI makes the exact target set explicit and reports partial dispatch failures.

### M6.4 — Persistence and polish

**Implemented in commit `e6fa4bc`.**

- Command-console visibility and height persist in the non-secret `command_console` section of `config.toml`; missing values in older config files use defaults (visible, 180 px). Stored height is clamped to 110–900 px.
- Keyboard shortcuts: Ctrl+Alt+K toggles the console; Ctrl+Alt+1–4 select single, left/right, top/bottom and 2×2 layouts; Ctrl+Enter requests a send.
- The console resize separator supports keyboard adjustment and exposes an accessible name/value. Layout and console controls expose translated accessible labels.
- Command-console UI strings and workspace tool labels are provided in English and Spanish.
- README, roadmap and the main specification are updated in the documentation follow-up commit.

No command text or broadcast draft is stored in config.

## Out of scope for M6

- Saved scripts, command libraries, variables, loops, conditions or a scripting language.
- Remote execution status parsing or transactional execution across hosts.
- Automatically appending Enter/newline, retrying commands, or hiding partial failures.
- Syntax highlighting (tracked separately as M6b).

## Security and operational notes

Broadcast writes directly to existing interactive PTYs. It does not bypass SSH authentication, but it can execute privileged or destructive commands on multiple hosts. The UI must make the target set unambiguous, keep protected sessions opt-in, require explicit Send, and never retry automatically. The console draft is ordinary frontend memory and must not be written into the encrypted vault or diagnostic logs.


### M6.3 usability follow-up

- Selected-session targeting uses a searchable, vertically scrollable list of workspace sessions rather than a wrapping checkbox strip. Disconnected retained tabs are visible but cannot be selected for dispatch.
- Pane tabs show compact Jump Host route and configured-forward indicators again. The forward badge is a configured count, not a live end-to-end tunnel health probe.
- Optional `keep_disconnected_sessions` setting retains the final terminal screen after the SSH process exits, marks the tab disconnected, disables input, and removes it from broadcast targets. The user closes the retained tab manually after inspecting its output.
