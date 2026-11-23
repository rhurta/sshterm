# SSHTerm GPUI prototype

An isolated GPUI shell prototype for the connection tree, terminal tabs, split panes,
and command console. Search, add-connection form validation, tab selection, split-pane
toggle, and console visibility are interactive.

## Pending

- Terminal input and output are not working yet. The panes are deliberate placeholders;
  they do not start PowerShell or SSH and the command console does not send commands.
- Connections are temporary UI data and are not loaded from or saved to SSHTerm's vault.

## Run

From the repository root:

```powershell
cargo run --manifest-path experiments/gpui-prototype/Cargo.toml
```

The current build is for reviewing the overall desktop layout and interaction flow while
terminal I/O is investigated separately.
