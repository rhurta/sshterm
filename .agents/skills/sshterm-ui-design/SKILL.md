---
name: sshterm-ui
description: UI/UX rules and layout spec for sshterm (Tauri 2 + Svelte 5). Use for any new or changed screen, component, theme, shortcut or layout work in the frontend.
---

# SSHterm UI/UX Design System & Implementation Guide

## 0. How to use this file

- This file is the source of truth for look, layout and keyboard behavior. If code and this file disagree, either fix the code or change this file in the same commit; do not leave them out of sync.
- Features come first, polish later. Apply sections 5 (interaction) and 6 (states) to everything you touch; apply sections 2-4 (visuals) fully only when you are on a UI task.
- Marked **[Decision]** items are open choices. Do not silently pick one; ask.

## 1. Stack and constraints

Tauri 2 + Rust backend, Svelte 5 (runes: `$state`, `$derived`, `$effect`) + TypeScript, xterm.js terminals. The app is **portable and offline**: exe + `config.toml` + vault next to each other, so no CDN assets, no runtime downloads.

| Need | Use | Do not use |
| --- | --- | --- |
| Styling | Plain CSS + theme variables (section 2) | Tailwind, shadcn-svelte (it requires Tailwind), any styled kit |
| Behavior/a11y of menus, dialogs, selects, tabs, tooltips | `bits-ui` (behavior only, no styles) | cmdk-sv, other component kits |
| Pane layout | The existing split tree (`splitTree.ts`, tested) for terminal panes. `paneforge` only for the outer shell (sidebar / settings / console) **[Decision]** | CSS `resize: both` for structural layout |
| Tree | The existing `ConnectionTree.svelte` with roving focus. Add virtualization only if lists exceed a few hundred rows **[Decision]** | Swapping in `SvTree` just for keyboard nav (we already have it) |
| Hotkeys | The existing key handlers (`Workspace.svelte`, `APP_SHORTCUT_KEYS` in `TerminalPane.svelte`) | `@tanstack/svelte-hotkeys` (would split key handling in two places) |
| Fonts | Bundle woff2 files with the app, with system fallbacks | Google Fonts links |
| Icons | One inline-SVG set, `currentColor`, 16/20 px | Mixed icon packs |

Every new dependency needs a reason in the PR: what behavior it gives that we cannot get with what is installed.

## 2. Visual style and tokens

**Themes (7):** `dark` (default), `light`, `nord`, and the four Catppuccin flavors: `mocha`, `macchiato`, `frappe` (dark) and `latte` (light). The theme is stored in `config.toml` (outside the vault, so the lock screen uses it) and selected in the title bar and in Settings > Appearance. Variables are defined on `.window-shell[data-theme=...]` in `src/routes/+page.svelte`; overlays rendered in a portal must portal into `.window-shell` or they lose the variables.

**Adding or changing a theme touches six places, all in one commit:** the `AppTheme` enum in `src-tauri/src/config.rs` (serde lowercase) and its roundtrip test; the `AppTheme` union, `THEMES` list and terminal palette in `src/lib/theme.ts` (+ `tests/theme.test.mjs`); the CSS block in `+page.svelte`; the `theme.<id>` strings in both languages. Never rename a theme id: it is persisted.

**Catppuccin mapping** (official swatches, https://catppuccin.com/palette): base -> app background, mantle -> `--surface`, surface0 -> `--surface-raised`, surface1 -> `--surface-hover` and `--border`, surface2 -> `--border-strong`, crust -> `--surface-deep`, text / subtext1 / subtext0 -> `--text` / `--text-secondary` / `--muted`, blue -> `--accent`, red / yellow / green -> `--danger` / `--warning` / `--success`. Their `*-bg`, `*-soft` and `*-border` tints are `color-mix` of the accent over the base. Terminal ANSI: black = surface1, white = subtext1, magenta = pink, cyan = teal, cursor = rosewater. **Latte** uses slightly darker muted, warning and success than the official swatches so small text keeps 4.5:1; keep that when editing it.

**Never hardcode a color** in components, including the xterm theme. Use these existing variables (define any new one in every theme at once):

| Role | Variable | Dark value (reference) |
| --- | --- | --- |
| Window / terminal background | `--app-bg` | `#1e1e2e` |
| Sidebars, tab bar, bottom panel | `--surface` | `#181825` |
| Modals, dropdowns, inputs, active tab | `--surface-raised` | `#313244` |
| Hover rows | `--surface-hover` | theme-specific |
| Recessed fields (value cells, editors) | `--surface-deep` | theme-specific |
| Main text | `--text` | `#cdd6f4` |
| Secondary text | `--text-secondary` | theme-specific |
| Placeholders, inactive icons | `--muted` | `#a6adc8` |
| Borders / strong borders (splits, focus-adjacent) | `--border` / `--border-strong` | `#45475a` / theme-specific |
| Active states, primary buttons, focus ring | `--accent` (text on it: `--accent-contrast`) | `#89b4fa` |
| Connected / OK | `--success`, `--success-soft`, `--success-border` | `#a6e3a1` |
| Errors, destructive | `--danger`, `--danger-bg` | `#f38ba8` |
| Warnings (host key, partial failure) | `--warning`, `--warning-bg`, `--warning-border` | theme-specific |

The dark values above are the reference palette; the real values live in the theme definitions and win.

**Surface variables are derived, not set per theme.** Each theme defines the five solid surfaces as `--c-app-bg`, `--c-surface`, `--c-surface-raised`, `--c-surface-hover`, `--c-surface-deep`; `.window-shell` derives `--app-bg`, `--surface`, ... from them with `color-mix(..., transparent)` and the `--panel-alpha` / `--raised-alpha` percentages. Components only ever use the derived names. Never put a solid color in a component for a surface, or it will not become translucent.

### Background image and transparency
- Settings > Appearance: background image path (PNG, JPG, WebP, GIF or BMP, up to 20 MB; a relative path starts at the exe folder), interface opacity and terminal opacity (20-100 %, default 85). Stored in `config.toml` `[appearance]`, outside the vault.
- The backend reads the image and returns a `data:` URL (`appearance_background_data`); no asset protocol or CDN is involved, so the app stays portable. An invalid image is rejected before anything is saved, and a missing image never blocks startup.
- Layers inside `.window-shell`: a solid `--c-app-bg` base with the image (`cover`, centered), then `.shell-content` above it. **Opacity applies only while something is behind the UI**: an image or a see-through window; otherwise the UI is fully opaque.
- Interface opacity drives `--panel-alpha`. Raised surfaces (dialogs, menus, active tabs) never go below 85 % (`--raised-alpha`) so overlays stay readable. The terminal uses its own value: xterm gets an rgba background and `allowTransparency: true`.
- Text colors are never made translucent. If a theme/image combination hurts legibility, lower the transparency rather than dimming text.
- **See-through window:** Settings > Appearance > Window transparency (Off / See-through / Mica on Windows 11), stored as `window_backdrop`. The window is created in `setup` from `tauri.conf.json` (`create: false`) so transparency follows the setting, which therefore applies **after a restart**; the UI reads the value actually in use (`window_backdrop_active`) and sets `data-see-through` on `.window-shell`. In that mode the shell and `.bg-layer` have no solid base; opacity applies over the desktop exactly as over an image, and the desktop shows only while no image is set. Do not add solid backgrounds to the shell, html or body. Acrylic/Blur are not offered (poor resize/drag performance). The window shadow is off in this mode (it draws a light border).

- **Fonts:** UI uses the Windows-native stack `Segoe UI Variable Text`, `Segoe UI`, then system sans. Do not reference web fonts because the app is portable/offline and currently bundles no UI font files. Terminal uses `JetBrains Mono`, then `Cascadia Mono`, then `Consolas`.
- **Terminal colors:** derive the xterm theme (background, foreground, cursor, selection, 16 ANSI colors) per app theme from one function. Light theme needs its own ANSI palette; do not reuse the dark one.
- **Radius:** 4 px inputs/buttons, 6 px panels/modals.
- **Spacing:** 4 px grid (4, 8, 12, 16, 24). Default control height 28 px, list row height 28 px, tab height 32 px.
- **Typography scale (single source of truth):** use the tokens on `.window-shell`; do not assign ad-hoc font sizes in components.
  - `--ui-font-display: 28px` — lock-screen / welcome hero only.
  - `--ui-font-title: 16px` — dialog titles and primary panel titles; weight 600.
  - `--ui-font-section: 14px` — section headings, title-bar brand and prominent labels; weight 600.
  - `--ui-font-body: 13px` — normal body copy and descriptive text; weight 400.
  - `--ui-font-control: 12px` — inputs, buttons, select controls and form labels.
  - `--ui-font-dense: 12px` — connection tree, tabs, properties grid and compact data.
  - `--ui-font-meta: 11px` — hints, captions, secondary metadata and status details. Never go below 11 px.
  - Terminal content remains 12 px monospace and follows its own terminal font settings. Icon glyph sizing is independent of text sizing.
  - Body and control weights: regular 400; interactive labels 500; headings 600. Do not use uppercase styling as a substitute for hierarchy.
- **Contrast:** text on any surface at least 4.5:1, non-text UI (borders of inputs, icons) at least 3:1, in every theme. Check `--muted` on `--surface` in light, nord and latte especially.
- **Color is never the only signal.** Connection status = color + icon/shape + accessible text (`aria-label`/title). Errors carry an icon and a message, not just red.
- **Motion:** 120 ms ease-out for enter transitions, none for resize. Honor `prefers-reduced-motion`.
- **Scrollbars:** thin, themed with variables; thumb uses `--border-strong`.

## 3. Global layout

Strict, resizable layout. Top to bottom, left to right:

1. **Title bar:** app name, live tunnel status, settings, window controls. It must use the real Tauri window controls (or a custom bar wired to them), never decorative dots.
2. **Main row:**
   - **Tree sidebar**
   - **Settings pane** (connection parameters of the selected connection)
   - **Vertical toolbar**
   - **Right area** (column): terminal panes on top, command console at the bottom.

**[Decision] Toolbar position.** The toolbar sits to the right of the two panels, so when the tree or settings pane is hidden the toolbar (and the toggle button you just clicked) moves left under the cursor. The usual fix is an activity-bar at the far left (toolbar, then tree, settings, terminals). Recommended; both layouts follow the same rules below.

Size limits (persist in `config.toml`, restore on start):

| Region | Default | Min | Max | Notes |
| --- | --- | --- | --- | --- |
| Tree sidebar | 280 | 200 | 400 | collapsible via toolbar and shortcut |
| Settings pane | 320 | 250 | 450 | collapsible |
| Toolbar | 48 fixed | | | |
| Command console height | 160 | 100 | 400 | collapsible |
| Terminal pane | | 250 x 150 | | below this, refuse the split with a visible message |
| Window | | 900 x 560 | | set in `tauri.conf.json` |

Resize handles: hit area at least 4 px (a 1 px visible line is fine), `col-resize` / `row-resize` cursor, background `--accent` on hover and while dragging, `role="separator"` with `aria-orientation` and value, and resizable by keyboard (arrow keys when focused, Home/End for min/max). Double-click resets to default size.

## 4. Components

### A. Tree sidebar
- Sticky search box on top. Typing filters immediately and highlights the matched text. Search covers name, host, and user.
- Nodes: folders and connections. Folder expand/collapse state persists. Active (open) connection has a subtle `--accent` tinted background plus a status dot with icon/text alternative.
- ARIA: `role="tree"`, rows `role="treeitem"` with `aria-expanded`, `aria-level`, `aria-selected`.
- Context menu (Bits UI `ContextMenu`) is the only way mouse users reach rename, delete, duplicate, new folder; each action also has a key path (section 5).
- Empty states: no connections ("New connection" and "Import from WinSSHTerm" actions), no search results (show the query and a clear-search action).

### B. Settings pane
- Sticky search box, filters parameters live.
- Grid `grid-template-columns: minmax(96px, max-content) 1fr`: label column in `--muted`, value column as an input-looking cell (`--surface-deep`, 1 px `--border`). Labels are real `<label>`s bound to their inputs.
- Rows: host, port, username, auth method, key, jump hosts, etc. Values are editable in place; secret fields are masked with a reveal toggle and are never logged.
- Validation: inline, under the field, on blur and on save. Port is 1-65535, host is non-empty. Show a dirty marker and Save/Revert when edited.
- Search that matches nothing shows an empty state, not a blank grid.

### C. Vertical toolbar
- Fixed 48 px, full height. Buttons 32 x 32 with 20 px icons, each with a tooltip that includes its shortcut.
- Top: toggle tree, toggle settings, divider, tile panels, split panels. Bottom (pinned with a `flex: 1` spacer): toggle command console.
- Toggles use `aria-pressed`. Active = `--surface-raised` background with `--accent` icon; inactive = `--muted`.

### D. Terminal panes
- Each pane has a tab bar: active tab `--surface-raised` with a 2 px top border `--accent`; inactive tab `--surface` and `--muted`. "+" button for a new tab. Tabs show connection name, status dot, close button (`--danger` on hover). Middle-click closes; drag reorders (keyboard alternative required).
- The focused pane has a 1 px `--accent` border. Panes resize in both axes through the split tree handles.
- **Moving tabs between panes** is supported and must keep the terminal alive (scrollback, running full-screen programs). Terminals are created once per session and re-attached to their pane; never put a terminal's lifecycle inside a pane's markup. Drag a tab onto another pane's tab strip (reorders when dropped between tabs), onto a pane's center (move), or onto a pane edge (split and move). Keyboard: Ctrl+Alt+Shift+Arrows. Details in `docs/tab-moves.md`.
- **Shortcut conflicts:** Windows and GPU drivers claim some Ctrl+Alt combinations (Ctrl+Alt+Arrows can rotate the screen; Ctrl+Alt+Tab, Ctrl+Alt+Del are reserved). Every shortcut that matters needs a second binding on a safe key, and AltGr combinations (Ctrl+Alt on Windows, used to type @ | ~ on Spanish layouts) must never be swallowed.
- **Pane targets:** a new connection goes to the dropped-on pane, the pane that asked for it, or the focused pane (which can be an empty one). New empty panes open the connection list immediately. Rules in `docs/pane-targets.md`.
- **Start state:** the workspace opens with **one empty pane**, focused, showing a hint and an open-connection button; the first connection opened from the tree or dropped on it lands there. The workspace never goes back to a "no panes" screen: closing the last tab leaves an empty pane. Layout presets (Ctrl+Alt+1-4) arrange open tabs and do nothing while there are none; the split buttons make empty panes.
- Per-session states are rendered inside the pane (see section 6): connecting, authenticating, host-key prompt, connected, disconnected (with Reconnect), failed (with the ssh error text).

### E. Command console
- Below the terminals, right of the toolbar. Top-edge drag handle, 100-400 px, collapsible.
- Left section: "Send to" selector (active session, all, selected panes) and the list of open connections with checkboxes; width resizable by a draggable divider. Right section: multi-line textarea and a Send button (`--accent`).
- Ctrl+Enter sends. Sending to more than one session shows a visible "broadcast to N sessions" indicator and asks for confirmation the first time per run.
- **Scripts:** Save script… (disabled while the editor is empty) and Load script… sit next to Clear. They open the same fixed-size dialog (`ScriptsDialog.svelte`) in two modes: a folder tree with a search field, and a preview (load) or name field (save). Loading fills the editor and never sends. Save mode focuses the name field; load mode focuses the search. **Favorites** row: favorite scripts are buttons in a single sideways-scrolling line below the console actions; click loads into the editor (asking Replace/Append when it has text), never sends; the row is hidden when empty. Library rules in `docs/scripts.md`.

### F. Dialogs and overlays
- **Fixed, proportional size.** Large dialogs (connection properties, settings) have a constant size that does not depend on their content: width `clamp(380px, 46vw, 820px)`, height `clamp(360px, 72-76vh, 820px)`, never larger than 96 % of the window. Only the body scrolls; the title, tabs/search and action buttons stay in place. Switching sections or filtering must never resize the dialog.
- **Connection properties dialog:** title, section tabs (General, Authentication, ...), scrollable body, Save/Cancel pinned at the bottom.
- **Settings dialog:** a search field at the very top, **focused when the dialog opens**. It filters rows live; a match on a section title shows the whole section, a match on a label shows only that row; no match shows an empty-state message. Escape clears the search first and closes the dialog on the second press. Sections: language, appearance, keyboard shortcuts, application behavior, terminal. New settings must be registered in the section table in `SettingsDialog.svelte` so search finds them.
- **Password fields** always use `PasswordInput.svelte`: a small eye button inside the field at its right end toggles visibility (`aria-pressed`, label "Show/Hide password", keyboard reachable, off by default, reset when the dialog closes). It styles through `--field-bg`, `--field-pad` and `--field-font-size`. Never use a bare `type="password"`.
- Built with Bits UI `Dialog`, portaled into `.window-shell`. Focus moves inside on open (to the first field, or to the search box for pickers) and returns to the opener on close. Focus is trapped. Escape closes, except while a destructive operation is mid-flight.
- Lock screen (vault) is full-window, shows no connection data, and focuses the password field.
- Destructive confirmations use a custom dialog. The WebView2 `confirm()`/`alert()` do not work here.

## 5. Interaction and accessibility (critical)

**Keyboard model.** Every screen must be fully usable without a mouse.

- **App shortcuts use Ctrl+Alt+key only.** Plain keys and Ctrl+letter belong to the terminal and the remote shell (`Ctrl+K`, `Ctrl+J`, `Ctrl+C`, `Ctrl+L`... are real shell/readline commands). Do not use `Ctrl+K` or `Ctrl+J` for app actions, even though many apps do.
- Register every new shortcut in two places: the window handler and `APP_SHORTCUT_KEYS` in `TerminalPane.svelte` (so xterm swallows it and it never reaches the remote shell). Add it to the keymap table below in the same commit.
- Shortcuts must not depend on the keyboard layout: use `event.code` for physical keys where the letter differs between layouts (the user works in Spanish and English).

| Keys | Action |
| --- | --- |
| Ctrl+Alt+Arrows | Move focus to the neighboring pane (can be taken by Windows/graphics-driver hotkeys) |
| Ctrl+Alt+, / Ctrl+Alt+. | Focus the previous / next pane (always available) |
| Ctrl+Alt+PageUp / PageDown | Previous / next tab in the focused pane |
| Ctrl+Alt+F | Focus the connection search box (also from a terminal) |
| Ctrl+Alt+K | Toggle the command console |
| Ctrl+Alt+1-4 | Layout presets |
| Ctrl+Alt+Shift+Arrows | Move the active tab to the neighboring pane |
| Enter, F2, Shift+F10 / Menu key | Open, rename, context menu on the focused tree row |

Add rows here for anything new (for example toggle tree, toggle settings, focus settings search); pick the Ctrl+Alt key when you implement it.

**Lists, trees, and search.**
- Any list or picker with a search box focuses that box when it opens. Down moves into the results, Up from the first result returns to the box, Enter on the box picks the first match, Escape clears the query first, and closes on the second press.
- Roving focus (one tab stop per list/tree): Up/Down/Home/End, Right/Left to expand/collapse, type-ahead where there is no search box.
- **Autofocus is explicit, not the HTML attribute.** At app start the tree search box takes focus. When a panel is opened by shortcut or toolbar, focus its search box. Never steal focus from a terminal that has it because a panel re-rendered, and do not autofocus two things at once.
- Escape inside a terminal goes to the terminal (vim, less). Escape only closes UI that is actually open and focused.

**Focus visibility.** Every interactive element has a visible `:focus-visible` style, `outline: 2px solid var(--accent); outline-offset: 2px`, readable in every theme. Never `outline: none` without a replacement. No `!important`.

**Semantics.** Buttons are `<button>`, icon-only buttons have `aria-label`, toggles have `aria-pressed`, dialogs have `aria-labelledby`, live status changes (tunnel up/down, session lost) are announced through a polite live region.

**Pointer.** Minimum target 28 x 28 px (toolbar 32). Context menus open on right click and on Shift+F10 / Menu key. Do not `preventDefault()` the `contextmenu` event on elements Bits listens to; it ignores already-prevented events.

**Localization.** All strings go through the i18n layer (English and Spanish today). No hardcoded text, and leave room for strings about 40% longer.

## 6. States every component must define

Design and implement these, not just the happy path:

- Empty (nothing yet, with the next action), loading (skeleton or spinner after 200 ms, not before), error (what failed, what to try, the raw ssh message in an expandable detail), disabled (with a reason in the tooltip).
- Session lifecycle: connecting, awaiting password/host-key decision, connected, disconnected, reconnecting, failed.
- Tunnel lifecycle: starting, active, failed (per forward), stopping, stopped. Failed forwards stay failed; do not flip back to active.
- Vault: locked, unlocking, unlock failed, locked by inactivity.
- Destructive actions: confirm, or support undo for a few seconds.

## 7. Implementation rules

1. Svelte 5 runes. Components are small; shared state lives in stores or in `Workspace.svelte`, not duplicated.
2. No hardcoded colors, sizes off the 4 px grid, or magic z-index numbers: use a small named scale (`--z-panel`, `--z-overlay`, `--z-menu`, `--z-toast`).
3. Overlay styling lives in global classes under `src/lib/ui/*.css` (scoped CSS cannot reach portaled elements) and uses only the theme variables.
4. Follow the DOM order of section 3 so the tab order matches the visual order.
5. Persist layout, panel sizes, collapsed state, the chosen theme and the appearance settings in `config.toml`; never write UI state into the vault.
6. Check every theme family before calling a UI task done: at least `dark`, `light`, `mocha` and `latte`, plus `nord`, `macchiato` and `frappe` when you touch colors.

## 8. Definition of done for a UI change

- [ ] Works with keyboard only: reach it, operate it, leave it, focus returns to where it came from.
- [ ] New shortcuts are in both handlers and in the keymap table.
- [ ] Visible focus, correct roles/labels, status not conveyed by color alone.
- [ ] Empty, loading and error states exist.
- [ ] Checked in dark, light, nord and the Catppuccin flavors (at least mocha and latte), with and without a background image, with contrast acceptable.
- [ ] Any new surface uses the derived variables, so it stays translucent over a background image or a see-through window.
- [ ] Dialogs keep their fixed size; any new password field uses `PasswordInput`.
- [ ] Strings go through i18n (English and Spanish).
- [ ] Works at the minimum window size and with long names/hosts (truncate with a tooltip).
- [ ] `npm run check` passes and CI is green.


## 9. Refined workspace shell

- The connection tree, connection properties pane and command console use the same shared `.workspace-panel-header` / `.panel-title` treatment: 40 px height, 12 px horizontal inset, 14 px semibold title, vertically centered, sentence case, and a 1 px bottom border. Their leading edges and title baselines align; do not add panel-specific top padding or override header typography. Vertical separators use the same `--border` token.
- The connection tree is followed by a selected-connection properties pane. The pane is resizable from 250 to 450 px and uses a sticky search field plus a 100 px label / flexible value grid. It shows every saved connection parameter (including authentication state without revealing secrets, jump hosts, all forwards, custom colors, SFTP options and highlighting). Edit opens the complete existing connection editor inline in the pane, reusing its validation and save path. Secret values are never displayed as plaintext.
- The 48 px vertical toolbar follows the left panels and spans the full workspace height, including beside the command console. Toolbar hit targets are square 32 x 32 px with centered 20 px inline SVG icons and localized hover titles/ARIA labels. Tree, properties and command console visibility are toggles. Persist all three visibility choices and command-console height in portable `config.toml` outside the encrypted vault, and restore them at startup.
- The command console is available with zero terminal sessions; its editor stays visible and sending is disabled until a valid target exists.
- Connection-tree top actions use square icon-only buttons with localized hover descriptions.
- The tunnel status control is rendered only while at least one independent tunnel is starting, active or stopping. The titlebar action group remains neutral and must not inherit the tunnel success background.
- Preserve the split-tree and terminal parking architecture. Never remount terminal components when moving tabs. Do not attach drag handlers to the new side panels or toolbar; terminal tab drag/drop zones and their MIME payloads remain unchanged.
- Active tabs use a top accent edge aligned to the tab bar, and focused panes use a 1 px inset outline. Do not add transforms or drag-wrapper spacing that changes tab hit boxes.
