# Which pane gets a new connection

A new session is added as a tab of the **target pane**. The target is chosen in
this order:

1. The pane the connection was dropped on (drag from the connection tree).
2. The pane that asked for it: the `+` of a pane's tab strip, the open-connection
   button of an empty pane, or a freshly created empty pane.
3. The **focused pane** (outlined in the accent color): the pane of the active
   tab, or an empty pane the user focused.
4. The first pane.

## Behavior

| Action | Result |
| --- | --- |
| Drag a connection from the tree onto a pane's tab strip or center | Opens it as a tab in that pane. |
| Drag a connection onto the left / right / top / bottom quarter of a pane | Splits that pane and opens the connection in the new pane (same zones as moving a tab). If the connection fails to open, the new pane is removed. |
| Drag a connection onto the empty workspace (no tabs yet) | Opens it as the first tab. |

Folders cannot be dropped on panes, and tree rows are not draggable while a search is active.

| Create an empty pane (the split buttons) | The new pane is focused and the connection list opens at once, targeting it. Escape closes the list and leaves the empty pane focused. |
| Focus an empty pane (click it, or **Ctrl+Alt+Arrows** from anywhere) | It gets the focus outline. Opening a connection from the tree (Enter, double click, context menu) then lands in that pane. |
| **Ctrl+Alt+Arrows** | Focus the neighboring pane: its active terminal, or the open-connection button when it is empty. |
| **Ctrl+Alt+, / Ctrl+Alt+.** | Focus the previous / next pane (cycles through all panes). Works everywhere, see below. |
| Click anywhere inside a pane | Focuses that pane. |

Notes:

- Focus follows the active tab when it changes (open, close, switch). Only an
  empty pane keeps explicit focus until something else becomes active.
- The split buttons split the focused pane, empty or not.
- Ctrl+Alt+Arrows can be intercepted by Windows or a graphics driver (Intel/AMD/
  NVIDIA hotkeys rotate the screen with them) before the app receives the key. Ctrl+Alt+,
  and Ctrl+Alt+. exist for that reason: they use the physical comma/period keys
  (`event.code`, so they do not depend on the keyboard layout) and are not
  claimed by Windows. Ctrl+Alt+Shift+Arrows (tab move) is not affected.
- The terminal key handler swallows these shortcuts (and Ctrl+Alt+K, Ctrl+Alt+1-4)
  so they never reach the remote shell. AltGr combinations are never swallowed:
  AltGr is Ctrl+Alt on Windows and types characters such as @, | and ~.
- The empty pane also shows a hint: pick a connection from the tree, or drop one
  there.

## Implementation

- `focusedPaneId` in `Workspace.svelte` holds the focused pane; `currentPaneId()`
  validates it against the current layout and falls back to the active tab's pane,
  then the first pane. `connect(summary, paneOverride?)` uses the override, then
  the pending pane (picker started from a pane), then `currentPaneId()`.
- The tree sets a second drag payload, `application/x-sshterm-conn`, on connection
  rows (the existing `text/plain` payload still drives tree reordering). Panes
  detect it from `dataTransfer.types` during drag-over and read it on drop.
- `splitPaneById` (pure, tested) splits a pane by its stable ID, so empty panes
  can be split too.
- Moving tabs between panes: see [`tab-moves.md`](tab-moves.md).

## Manual test checklist

1. Drag a connection from the tree onto a pane with tabs, onto its tab strip, and
   onto an empty pane.
2. Press a split button: the list opens targeting the new pane; pick one.
3. Split again, press Escape, then open a connection from the tree: it opens in
   the empty pane.
4. Drag a connection onto each pane edge: the pane splits and the connection opens in the new pane.
5. Use Ctrl+Alt+, and Ctrl+Alt+. to cycle panes, and Ctrl+Alt+Arrows between panes (terminal, empty, terminal) and check that
   nothing is typed into the remote shell.
