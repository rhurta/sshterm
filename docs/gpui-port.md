# Porting sshterm's UI to GPUI Kit

Status: **planning + phase 1 (shell layout)** on branch `experiment/gpui-connection-tree`,
code in `gpui-prototype/`. The production app (Svelte + Tauri) is unchanged and remains the
reference for behavior; see its `docs/` and `.agents/skills/sshterm-ui-design/SKILL.md`
on `main` for the layout rules this port must keep.

## Goal and rules

1. **Same functionality and same global layout** as the production app.
2. **Use GPUI Kit components.** Before writing anything new, in this order:
   1. use the component as it is (theme tokens, sizes, variants);
   2. configure or skin it (`DockSkin`, `PanelStyle`, theme JSON);
   3. **extend** it: wrap it, compose it, or add the missing method in a small helper in our crate
      (and note it as an upstream candidate in the table below);
   4. only then write a new component, and say why in this document.
3. Keep rendering separate from behavior: tree operations, session control and persistence live
   in framework-independent Rust (see "Shared model" below), not in view code.
4. The prototype pins `gpui-kit = "0.9.1"` (latest published is 0.11.0). Upgrading is a separate,
   deliberate step: re-run the Windows CI and re-read the release notes.

## Global layout (from the production app)

```
+--------------------------------------------------------------------------------+
| Title bar: app name · tunnels status · theme · settings · lock · window buttons |
+----------+-------------+----+----------------------------------------------------+
| Tree     | Properties  | R  | Pane area (tab groups, splits)                      |
| sidebar  | pane        | a  |                                                      |
|  search  |  (selected  | i  |                                                      |
|  tree    |  connection)| l  +----------------------------------------------------+
|          |             |    | Command console (collapsible, resizable)           |
+----------+-------------+----+----------------------------------------------------+
```

Left to right: **tree sidebar**, **properties pane**, **vertical toolbar (rail)**, **main area**.
The main area is a column: **pane area** on top, **command console** at the bottom.

| Region | Default | Min | Max | Visibility |
| --- | --- | --- | --- | --- |
| Tree sidebar | 280 px | 200 | 400 | toggle button |
| Properties pane | 320 px | 250 | 450 | toggle button |
| Rail | 48 px, fixed | | | always |
| Console | 160 px | 100 | 400 | toggle button, bottom of the rail |
| Pane | | 250 x 150 | | |
| Window | | 900 x 560 | | |

Rail buttons, top to bottom (same order as production):

1. Toggle tree · 2. Toggle properties · separator ·
3. New tab · 4. Layout: single · 5. left/right · 6. top/bottom · 7. grid · separator ·
8. Split left/right · 9. Split top/bottom · (spacer) · 10. Toggle command console.

Toggles show a pressed state; every button has a tooltip with its shortcut. Keyboard rules from the
production guide still apply: app shortcuts are Ctrl+Alt based, every list has a focused search
field, focus is always visible.

## Component mapping

"As is" = used directly. "Extend" = built from existing components with a small helper.
"New" = nothing suitable exists.

| Production feature | GPUI Kit component | Plan | Notes |
| --- | --- | --- | --- |
| Window frame, title bar | `TitleBar` | as is | `TitleBar::window_options()` already integrates platform controls. |
| Theme selector | `Button` + theme registry (later `Select`) | as is | Five bundled packs include the four Catppuccin flavors; see "Themes". |
| Tunnels status, settings, lock buttons | `Button` (ghost, icon), `Badge`/`Tag` | as is | |
| Shell: tree / properties / rail / main | `h_resizable` + `resizable_panel().visible().size().size_range()` | as is | `.fixed()` keeps the rail at 48 px; `.visible(false)` implements the toggles; min/max come from `size_range`. |
| Main: panes over console | `v_resizable` | as is | Console panel `.size(160).size_range(100..400)`. |
| Connection tree + search | `Tree`, `TreeState`, `ListItem`, `Input` | as is (done in the first prototype) | Drag and drop between folders is attached to the rows; filtering rebuilds the items. |
| Tree context menu | `Menu` / popup menu | as is | Later. |
| Properties pane | `Label`, `Input`, `Select`, `h_form()` / `field()` | extend | A `PropertyGrid` helper builds the label/value rows from `Form`; the selected connection feeds the inputs. |
| Vertical toolbar | `Button` (ghost, toggle/selected), `Tooltip`, `Separator` | **extend** | `Toolbar` is horizontal only (fixed `items_center` flex row). `ToolRail` composes the same controls vertically; propose `Toolbar::vertical()` upstream. |
| Panes with tabs, splits, drag tab between panes, split by dropping on an edge | `DockArea` + `DockSkin` + `DockLayout::{h_split, v_split, tabs}` | as is | Replaces the hand-written split tree and terminal re-parenting. `add_panel`, `move_panel`, `split_at`, zoom and `dump`/`load` exist. |
| Layout presets (single, left/right, top/bottom, grid) | `DockLayout` builders | extend | Each preset is a function that returns a `DockLayout` and re-homes the existing panels. |
| Pane focus / move-tab shortcuts | GPUI actions and key bindings | extend | Ctrl+Alt+Arrows, Ctrl+Alt+Shift+Arrows, Ctrl+Alt+, / . bound to `DockArea` operations. |
| Command console editor | `Textarea` + `TextareaState` | as is | Ctrl+Enter sends; `InputEvent::PressEnter` is available. |
| Console targets | `Select`, `Checkbox`, `List` | as is | Later. |
| Settings dialog | `Dialog`, `Settings` / `Form`, `Switch`, `Slider`, `Select`, `Input` | as is | Later; search field on top. |
| Connection editor dialog | `Dialog`, `Form`, `Input`, `Select`, `TabBar` | as is | Later. |
| Password reveal | `Input::mask_toggle()` | as is | The eye button exists in the component. |
| Themes (dark, light, nord, Catppuccin x4) | `ThemeRegistry` JSON themes | extend | Catppuccin packs are bundled; Nord and the production dark/light need theme JSON files. |
| Background image, transparency | gpui `WindowBackgroundAppearance` (Opaque, Transparent, Blurred) + an image element | extend | Native transparency is supported by the framework, unlike the webview; design when we get there. |
| Scripts library dialog | `Dialog`, `Tree`, `Input`, `Textarea` | as is | The tree/filter logic is reused from this prototype. |
| SFTP browser | `DataTable` / `Table` (virtualized), `Tree`, `Progress` | as is | Later. |
| Notifications | `Notification` | as is | |
| **Terminal emulator** | none | **new** | GPUI Kit has no terminal widget. A terminal view over `alacritty_terminal` (or the ConPTY path) with explicit input/output/resize channels is the one component we must write. |

## Phase 1: shell layout (this step)

Delivered in `gpui-prototype/`:

- Title bar with app name, tunnels badge, theme button, settings and lock buttons.
- Tree sidebar: search field + `Tree` + drag-to-move + root drop zone (from the first prototype).
- Properties pane for the selected connection (read-only grid for now).
- Vertical rail with all ten buttons, toggle states and tooltips.
- Pane area built on `DockArea`; double-clicking a connection adds a terminal-placeholder tab (a connection
  already open is just selected). The layout buttons re-home all open tabs into one, left/right, top/bottom or
  grid groups through `DockLayout`; the split buttons currently apply the matching two-group preset (splitting the
  focused group needs `split_at` and a way to find the active group's node: next step).
- Command console: `Textarea`, target label, Send button, resizable height, toggle from the rail.
- All regions inside one `h_resizable` / `v_resizable` structure with the production sizes.

Not in phase 1: persistence, real sessions, terminal emulation, dialogs, scripts, SFTP, keyboard shortcuts,
editing in the properties pane, console targets and Ctrl+Enter.

### Verification status

- `cargo check --bin sshterm-gpui` passes on Linux (Rust 1.100) with no warnings; the pure modules
  (`model.rs`, `layout.rs`) pass their unit tests.
- **Not run:** nothing has been launched. The things most likely to need adjusting on Windows: panel sizes in
  the resizable groups (the flexible panels have no explicit size), the dock's tab chrome inside our theme, the
  rail's pressed state, and double-click detection on tree rows. The Windows CI builds and tests it.

## Later phases

1. **Shared model crate.** Extract the pure tree operations (`src-tauri/src/store/tree.rs`), the
   connection model and the vault (`SealedFile`) into a framework-independent crate used by Tauri
   and GPUI. Never create a second credentials format.
2. **Persistence and editing:** connection editor, properties editing, import, settings, themes.
3. **Sessions and terminal:** session controller (OpenSSH + ConPTY as in `src-tauri/src/session`),
   terminal view, logging, broadcast console.
4. **Scripts, SFTP, tunnels**, background image / transparency.
5. **Evaluate** startup time, idle memory, CPU under terminal output, executable size, keyboard
   and drag/drop behavior against the Svelte build on the same Windows machine, then decide.

## Themes

The production themes map to GPUI Kit themes like this: `mocha`, `macchiato`, `frappe`, `latte` →
the bundled Catppuccin pack (Mocha, Macchiato, Frappe, Latte); `dark`/`light` → kit "Default Dark/Light"
or custom JSON; `nord` → custom JSON (not in the bundled packs). Theme JSON files live in
`gpui-prototype/themes` and keep their upstream licenses.

## Building and checking

- Windows (reference platform, CI: `.github/workflows/gpui-windows.yml`): see
  `gpui-prototype/README.md`.
- Linux is only useful for compile checks: `cargo check --manifest-path gpui-prototype/Cargo.toml`
  needs cmake, clang, and the X11/Wayland/xkbcommon/fontconfig/Vulkan development packages, and a
  Rust new enough for the Linux-only `oo7` dependency (1.92); with older Rust use
  `--ignore-rust-version`. Interactive behavior (drag and drop, focus, shortcuts) must be tried on
  Windows.

## Open questions

- Dock chrome (tab bar, zoom, collapse buttons) versus production: the first run on Windows decides
  how much `PanelStyle` configuration is enough.
- Whether to upgrade from 0.9.1 to 0.11.x before building more (new components, API changes).
- Accessibility and IME behavior of GPUI text input in the terminal view.

## References

- [Tree](https://gpui-kit.com/component/tree/), [Dock](https://gpui-kit.com/component/dock),
  [GPUI Kit docs](https://gpui-kit.com/docs/), [API](https://docs.rs/gpui-kit/), [source](https://github.com/longbridge/gpui-kit).
