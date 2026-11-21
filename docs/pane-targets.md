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
| Drag a connection from the tree onto any pane (tab strip or body) | Opens it as a tab in that pane. Folders cannot be dropped on panes. While searching the tree, rows are not draggable. |
| Create an empty pane (the split buttons) | The new pane is focused and the connection list opens at once, targeting it. Escape closes the list and leaves the empty pane focused. |
| Focus an empty pane (click it, or **Ctrl+Alt+Arrows** from anywhere) | It gets the focus outline. Opening a connection from the tree (Enter, double click, context menu) then lands in that pane. |
| **Ctrl+Alt+Arrows** | Focus the neighboring pane: its active terminal, or the open-connection button when it is empty. |
| Click anywhere inside a pane | Focuses that pane. |

Notes:

- Focus follows the active tab when it changes (open, close, switch). Only an
  empty pane keeps explicit focus until something else becomes active.
- The split buttons split the focused pane, empty or not.
- Ctrl+Alt+Arrows (with or without Shift) are swallowed by the terminal key
  handler so they never reach the remote shell.
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
4. Use Ctrl+Alt+Arrows between panes (terminal, empty, terminal) and check that
   nothing is typed into the remote shell.
