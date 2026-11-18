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

## Keyboard (applies to every new or changed UI)

Every screen must be fully usable without a mouse. Checklist for new work:

- **Lists get a search box on top, focused when the list opens**; typing filters at once,
  Down moves into the results, Up from the first result returns to the box, Enter on the
  box picks the first match, Escape closes or clears.
- **Roving focus in lists and trees**: Up/Down/Home/End; Right/Left expand/collapse folders.
- **App-level shortcuts use Ctrl+Alt+key** (never plain keys or Ctrl+letter, which terminal
  programs need). The terminal key handler must also swallow them so they are not sent to
  the remote shell (`APP_SHORTCUT_KEYS` in `TerminalPane.svelte`). Add new ones to both places.
- Dialogs: focus moves inside on open and returns to the opener on close; Escape closes.
- Every action reachable by mouse (context menu, buttons) has a key path; context menus open
  with Shift+F10 or the Menu key.

Current keymap:

| Keys | Action |
| --- | --- |
| Ctrl+Alt+Arrows | Move focus to the neighboring pane in a split layout |
| Ctrl+Alt+PageUp / PageDown | Previous / next tab in the focused pane |
| Ctrl+Alt+F | Focus the connection search box (also from a terminal) |
| Ctrl+Alt+K | Toggle the command console |
| Ctrl+Alt+1–4 | Layout presets |
| Down (in search) | Move into the tree results |
| Up/Down/Home/End, Right/Left | Navigate the tree; expand/collapse folders |
| Enter, F2, Shift+F10 | Open, rename, context menu on the focused tree row |

## Migrated

- Connection tree context menu (`ConnectionTree.svelte`, `src/lib/ui/menu.css`): arrow keys,
  Home/End, typeahead, Escape, focus return and collision-aware positioning now come from
  `ContextMenu`. Right click on empty tree space offers New folder / New connection at root.

## Candidates

Dialogs (connection editor, import, lock screen), language select, tabs, tooltips.
