# SSHTerm GPUI Kit prototype

An isolated workspace-shell prototype built with GPUI Kit 0.9.1. It uses the kit's
hierarchical connection tree, tab bar, buttons, inputs, theme support, and drag/drop
behaviors, alongside custom split-pane and command-console layouts.

## Included

- Searchable nested connection tree with keyboard navigation, expand/collapse, and drag-to-organize.
- Selectable and closable session tabs.
- Split-pane toggle with an action to place the active tab in the second pane.
- Command console layout with an active-tab target.
- Theme selector and visible terminal-pending status.

## Pending

- Terminal input/output is not working. The panes are placeholders and do not start PowerShell or SSH.
- Command sending is not connected to a terminal session.
- Connections are sample in-memory data, not loaded from or saved to the encrypted vault.

## Run

From the repository root:

```powershell
cargo run --manifest-path experiments/gpui-prototype/Cargo.toml
```
