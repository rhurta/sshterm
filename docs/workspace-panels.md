# Workspace panels and connection properties

This document describes the connection workspace's side panels and the visual conventions shared by the tree, properties pane and command console.

## Side-panel visibility

The connection tree and selected-connection properties pane can be shown or hidden independently. Their visibility is stored outside the encrypted vault in the portable application's `config.toml`:

```toml
[workspace_panels]
tree_visible = true
properties_visible = true
```

Both panels are visible by default. Missing settings in an older `config.toml` use these defaults, so adding the preference does not require a migration. These preferences are non-secret and are available before the vault is unlocked.

The command console has its own independent persisted visibility and preferred height under `[command_console]`. Command drafts are intentionally kept in frontend memory only; they are not saved in configuration, the vault or logs.

## Connection properties pane

When a connection is selected, the properties pane displays its saved values in a compact property/value grid. The view includes the connection identity and endpoint, authentication method and credential state, jump-host route and enabled/disabled hops, keepalive, and the configured connection options such as SFTP and port forwarding.

Choose Edit to switch the pane into the inline connection editor. It uses the same editor and validation path as the full connection editor, so the complete saved connection configuration can be reviewed and changed without opening a separate modal. Save applies the changes; Cancel returns to the property grid.

**Secrets are masked by default.** For password-authenticated connections, the grid includes a Password row with an eye control on the right. The saved password is revealed only while the eye is enabled, and the value is masked again when another connection is selected. Key passphrases remain represented as saved/not-saved state and are never revealed in the grid. Credentials can still be updated through the secure authentication fields in the editor.

With no connection selected, the pane displays its empty state. Loading and retrieval errors are shown in the pane rather than silently presenting stale values.

## Shared panel headers

The Connections tree, Properties pane and Command Console use the shared `workspace-panel-header` styling from `src/routes/+page.svelte`:

- Fixed 40 px height and flex basis, so all three panel titles align on the same horizontal line.
- 12 px horizontal inset and vertically centered contents.
- One-pixel themed bottom border.
- A 14 px semibold, sentence-case title using `--ui-font-section`; no uppercase transformation or component-specific title sizing.
- Header actions use the same compact action footprint so adding controls does not change the header geometry.

The Command Console body uses the same 12 px horizontal inset as the side-panel headings. The console's outer top border remains the separator from the terminal workspace; its header divider is the same as the side panels.

## Typography and spacing

Use the typography tokens defined on `.window-shell` in `src/routes/+page.svelte`, as documented in `.agents/skills/sshterm-ui-design/SKILL.md`:

| Token | Size | Use |
|---|---:|---|
| `--ui-font-display` | 28 px | Lock-screen / welcome hero only |
| `--ui-font-title` | 16 px | Dialog titles and primary panel titles |
| `--ui-font-section` | 14 px | Section headings and prominent panel labels |
| `--ui-font-body` | 13 px | Normal body and descriptive text |
| `--ui-font-control` | 12 px | Inputs, buttons, selects and form labels |
| `--ui-font-dense` | 12 px | Connection tree, tabs, properties grid and compact data |
| `--ui-font-meta` | 11 px | Hints, captions, secondary metadata and status details |

Use the existing 4 px spacing grid and theme variables. Do not introduce ad-hoc font sizes, hardcoded surface colors or uppercase styling as a substitute for hierarchy. Keep text at or above 11 px.

## Drag-and-drop and keyboard behavior

The panel refinement does not change the existing connection or tab drag-and-drop behavior:

- Connections can still be dragged from the tree onto a pane or pane edge.
- Tabs can still be reordered or moved between panes without recreating the terminal session; screen state and scrollback remain attached to the session.
- Panel visibility changes do not rewrite pane layout or session ownership.

The workspace remains keyboard-first. Search fields in lists and pickers receive focus when opened, and focus must not be stolen from a terminal merely because a panel rerenders. Keep visible focus indicators and accessible labels on icon-only actions.

## Implementation references

- Panel visibility settings: `WorkspacePanelSettings` in `src-tauri/src/config.rs`.
- Shared panel header and typography tokens: `src/routes/+page.svelte`.
- Connection property grid and inline editing: `src/lib/ConnectionPropertiesPane.svelte` and `src/lib/ConnectionEditor.svelte`.
- Design rules and accessibility requirements: `.agents/skills/sshterm-ui-design/SKILL.md`.
