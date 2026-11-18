# UI components (Bits UI)

sshterm keeps its own CSS and theme variables (`dark`, `light`, `nord` on `.window-shell`).
[Bits UI](https://bits-ui.com) is used only for behavior and accessibility (focus, keyboard,
ARIA, positioning); it ships no styles, so nothing about the look changes with it.

## Rules

- Install only `bits-ui`. No Tailwind, no styled kit.
- Bits renders overlays in a portal. Portal into `.window-shell` (`<X.Portal to=".window-shell">`)
  so the theme variables apply. A portal to `body` would lose them.
- Style overlay parts with global classes in `src/lib/ui/*.css` that use only the existing
  variables (`--surface-raised`, `--border`, `--accent`, `--danger`...). Scoped component CSS
  cannot reach elements Bits renders in a portal.
- Respect `prefers-reduced-motion`; enter animations are short (about 120 ms, ease-out) and
  start from the pointer/anchor (`--bits-*-transform-origin`).
- Do not call `preventDefault()` on an event Bits listens to (for example `contextmenu`);
  Bits ignores events that are already default-prevented.

## Migrated

- Connection tree context menu (`ConnectionTree.svelte`, `src/lib/ui/menu.css`): arrow keys,
  Home/End, typeahead, Escape, focus return and collision-aware positioning now come from
  `ContextMenu`. Right click on empty tree space offers New folder / New connection at root.

## Candidates

Dialogs (connection editor, import, lock screen), language select, tabs, tooltips.
