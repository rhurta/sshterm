# Appearance: themes, background image, transparency

## Themes

Seven themes, selectable in the title bar and in Settings > Appearance, saved in
`config.toml` (`theme = "mocha"`), outside the vault so the lock screen uses them:

| Id | Name | Kind |
| --- | --- | --- |
| `dark` (default), `light`, `nord` | Dark, Light, Nord | existing themes, unchanged |
| `mocha`, `macchiato`, `frappe` | Catppuccin Mocha / Macchiato / Frappé | dark |
| `latte` | Catppuccin Latte | light |

Catppuccin flavors use the official palette, including the 16 terminal ANSI colors
(black = surface1, white = subtext1, magenta = pink, cyan = teal, cursor = rosewater).
Latte's muted, warning and success text colors are a little darker than the official
swatches to keep 4.5:1 contrast.

To add a theme: the `AppTheme` enum (`config.rs`, with a roundtrip test), the union,
`THEMES` and the palette in `src/lib/theme.ts` (with `tests/theme.test.mjs`), the CSS
block in `src/routes/+page.svelte`, and the `theme.<id>` strings in both languages.

## Background image and transparency

Settings > Appearance:

- **Background image:** path to a PNG, JPG, WebP, GIF or BMP file, up to 20 MB. A
  relative path starts at the folder of `sshterm.exe`, so a portable install can keep
  the image beside the exe. Press Apply (or Enter); Remove clears it. A file that
  is missing, too large or in an unsupported format is rejected with a message and
  the previous setting stays.
- **Interface opacity** and **terminal opacity** (20-100 %, default 85): how much of
  the image shows through the panels and the terminal background. They apply only
  while an image is set or the window is see-through. Dialogs and menus never go below 85 % so they stay readable.
  Sliders save when released.

Stored in `config.toml`:

```toml
[appearance]
background_image = "wallpaper.jpg"
panel_opacity = 85
terminal_opacity = 85
```

## See-through window

Settings > Appearance > **Window transparency** lets the desktop show through the
window:

| Value | Result |
| --- | --- |
| Off (default) | Opaque window. |
| See-through | Transparent window: the desktop shows through the translucent surfaces. |
| Mica (Windows 11) | Transparent window with the Windows 11 Mica material behind it. Older Windows versions ignore the effect and behave like See-through. |

```toml
[appearance]
window_backdrop = "mica"   # "off" | "transparent" | "mica"
```

Rules and limits:

- Transparency is chosen when the window is created, so **a change applies after
  restarting sshterm**. Settings shows a notice while the saved value differs from the
  one in use.
- The desktop is visible only while no background image is set (an image covers it).
  The interface and terminal opacity sliders (20-100 %) control how much shows
  through, exactly as they do over an image. Dialogs and menus stay at least 85 % opaque.
- The window shadow is turned off for transparent windows: an undecorated window with
  a shadow gets a 1px light border on Windows, which would outline the transparent
  window. Rounded corners therefore are not applied.
- Acrylic and Blur are deliberately not offered: Tauri documents poor performance when
  resizing or dragging with them on several Windows builds.
- If the WebView2 runtime paints transparent web views opaque (reported for some
  builds), the window simply stays opaque; nothing else breaks. Report it with the
  WebView2 version.

## Implementation

- The backend command `appearance_background_data` reads the file and returns a
  `data:` URL (no asset protocol, no network). Loading is validated before saving, and
  a missing image at startup is ignored.
- Each theme defines the solid surfaces as `--c-app-bg`, `--c-surface`,
  `--c-surface-raised`, `--c-surface-hover` and `--c-surface-deep`. `.window-shell`
  derives `--app-bg`, `--surface`, ... with `color-mix(..., transparent)` using
  `--panel-alpha` and `--raised-alpha`, so every component becomes translucent without
  changes. Components must use the derived names only.
- The main window is declared with `create: false` in `tauri.conf.json` and built in
  `setup` (`build_main_window` in `lib.rs`) from that same config, so transparency
  (`transparent(true)`, `shadow(false)`, and Mica through `set_effects`) can follow the
  saved setting. The backdrop in use is exposed by `window_backdrop_active`, which the
  UI reads to know whether the window is really transparent (`data-see-through`).
- The terminal gets an rgba background through `getTerminalTheme(theme, opacity)` and
  xterm's `allowTransparency`.

## Dialogs and password fields

- The connection properties dialog and the Settings dialog have a fixed size
  proportional to the window (46 % wide, 72-76 % tall, clamped) and scroll inside.
- Settings has a search field at the top, focused when it opens. It filters rows live
  (a section title match shows the whole section); Escape clears it, then closes.
- Password fields (lock screen, connection password, key passphrase) have an eye
  button at the right end that shows or hides the text.
