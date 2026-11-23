# SSHTerm GPUI prototype

An isolated desktop prototype for the connection sidebar and a single live terminal pane.
It uses GPUI Component controls for the search, connection editor, and actions, plus
`gpui-terminal` over `portable-pty` for a real terminal session.

## Run

From the repository root:

```powershell
cargo run --manifest-path experiments/gpui-prototype/Cargo.toml
```

The app opens a local PowerShell session. Add an SSH connection in the sidebar and select
it to start `ssh` inside the terminal. Authentication is handled by the machine's OpenSSH
configuration, keys, agent, or interactive terminal prompt. Connections exist only for the
current prototype run; this does not read or modify SSHTerm's encrypted vault.

This is a feasibility prototype. The terminal component currently lacks mouse text
selection and scrollback navigation, and the sidebar is not yet connected to the app's
saved connection tree.
