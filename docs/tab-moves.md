# Moving tabs between panes

Tabs (sessions) can be moved between panes of the tiled workspace without
reconnecting and without resetting the terminal: scrollback (5000 lines), the
current screen of a full-screen program, selection and highlight decorations all
survive a move.

## Behavior

| Action | Result |
| --- | --- |
| Drag a tab onto another pane's tab strip | Moves it there. The drop position (before/after the tab under the pointer) sets its order. |
| Drag a tab onto a tab of its own pane | Reorders it inside the pane. |
| Drag a tab onto the center of a pane | Moves it to the end of that pane's tabs. |
| Drag a tab onto the left / right / top / bottom quarter of a pane | Splits that pane and moves the tab into the new pane. |
| **Ctrl+Alt+Shift+Arrows** | Moves the active tab to the neighboring pane in that direction (nothing happens if there is none). Keyboard path for the drag actions. |
| Right-click a tab, or press **Shift+F10 / Menu** on a focused tab | Opens the tab context menu. |
| Close all but this tab | Closes the other tabs in the same pane; tabs in neighboring panes are unaffected. |
| Close all tabs | Closes every tab in the same pane and leaves that pane empty. |

Rules:

- The moved tab becomes the active tab of the target pane and keeps keyboard focus.
- The source pane keeps its remaining tabs; if it was left empty it stays as an
  empty pane (same as closing a tab) and can be removed with its × button.
- Dropping the only tab of a pane onto the edge of that same pane does nothing.
- Pane layout is workspace state and is not persisted.
- The move shortcut is swallowed by the terminal key handler, so it never reaches the
  remote shell as a modified-arrow sequence.
- The tab context menu can be opened with the pointer or keyboard. Escape closes it
  and returns focus to the invoking tab when it is still open.

## Design

- **Pure helpers (`src/lib/splitTree.ts`, covered by `tests/splitTree.test.mjs`)**:
  `paneOfSession`, `moveSession` (move/reorder; the source pane is kept even when
  empty), `moveSessionToNewPane` (needs a unique new pane ID), and `neighborPane`
  (adjacent pane from the normalized layout geometry).
- **Terminals belong to sessions, not panes.** `Workspace.svelte` renders one
  `TerminalPane` per tab in an off-screen parking area, and each one attaches its
  DOM to its pane's body element (`container` prop). A move only changes the
  layout tree; the terminal element is re-parented, then re-fitted, refreshed and
  re-focused. Before this, each terminal lived inside its pane's markup, so
  changing panes (or a layout preset) destroyed and recreated it.
- **Fitting only when attached.** `syncSize` does nothing while the terminal is
  not inside its pane. A parked or detached terminal has no meaningful size and
  fitting it would shrink the PTY.
- The parking area is laid out but invisible (`visibility: hidden`, off-screen,
  fixed size) so xterm can still measure characters before the first attach.
- Drag and drop uses HTML5 drag events (`dragDropEnabled` is already false in the
  Tauri window config for this reason). Drag data uses the custom type
  `application/x-sshterm-tab`.

## Test notes

Automated: pure tree helpers (move, reorder, split, neighbor lookup, invalid
inputs). Manual checklist for Windows:

1. Open three sessions, run `seq 1 3000` in one, move it to another pane: the
   scrollback is intact and the PTY size matches the new pane.
2. Run `vim`/`less`, move the tab: the screen is still there.
3. Move with Ctrl+Alt+Shift+Arrows in a 2x2 layout; check that nothing is typed
   into the remote shell.
4. Drop on each pane edge; move the last tab out of a pane; change layout presets
   with several tabs open (terminals are no longer recreated).
5. Close a moved tab and check the log file and the backend session are closed.

## SFTP workspace tabs

SFTP browsers participate in the same pane tab strip and use the same drag/drop and
keyboard move commands as SSH terminal tabs. A pane can therefore contain SSH and
SFTP tabs together, and separate panes can show different active tab types.

Each top-level SFTP workspace tab owns exactly one SFTP connection. There is no
second, nested SFTP session tab strip: opening another connection creates another
SFTP tab in the shared pane hierarchy. The workspace tab is labelled with the
connection name once the connection is selected.

An SFTP browser stays mounted while its tab moves. Its root element is re-parented
to the destination pane instead of recreating the component, so the remote SFTP
session, local/remote directory state, selections and transfer queue survive a
move. Switching to another tab hides the browser but does not close its session.
Closing the top-level SFTP tab (or vault lock/app shutdown) closes that session.

SFTP keyboard shortcuts are handled only by the currently focused workspace tab;
other visible SFTP panes do not process the same global key event.
