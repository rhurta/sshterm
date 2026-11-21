# SKILL: SSHterm UI/UX Design System & Implementation Guide

## 1. Role & Tech Stack
You are an expert UI/UX developer specializing in desktop applications using **Rust** (Tauri backend) and **Svelte 5** (frontend). Your task is to build and refactor the UI for "SSHterm" following this strict design system.
- **Styling:** Tailwind CSS (use the provided CSS variables/tokens).
- **Resizable Layouts:** `paneforge` (Svelte port of react-resizable-panels).
- **Tree View:** `SvTree` (for virtualization and keyboard nav).
- **Command Palette/Modals:** `cmdk-sv`.
- **Hotkeys:** `@tanstack/svelte-hotkeys`.
- **UI Components:** `shadcn-svelte` (which natively uses `paneforge` for its Resizable component).

## 2. Visual Style & Design Tokens
**Theme:** "Modern Developer Dark" (Catppuccin-inspired)
- **Fonts:** UI: `Inter`, `Roboto`, or system sans-serif. Terminal: `JetBrains Mono`, `Fira Code`, or `Consolas`.
- **Colors (CSS Variables):**
  - `--bg-base`: `#1e1e2e` (Main background, terminal backgrounds)
  - `--bg-panel`: `#181825` (Sidebars, bottom panels, tab bars)
  - `--bg-surface`: `#313244` (Modals, dropdowns, input fields, active tabs)
  - `--text-primary`: `#cdd6f4` (Main text)
  - `--text-muted`: `#a6adc8` (Secondary text, placeholders, inactive icons)
  - `--accent`: `#89b4fa` (Active states, primary buttons, focus rings)
  - `--border`: `#45475a` (1px solid borders for panels and splits)
  - `--success`: `#a6e3a1` (Active connection status)
  - `--error`: `#f38ba8` (Errors, close buttons)
- **Border Radius:** `4px` for inputs/buttons, `6px` for panels/modals.
- **Spacing:** 4px grid system (4, 8, 12, 16, 24).

## 3. Global Layout Hierarchy
The application uses a strict, resizable flex/grid layout. The DOM structure from Left to Right, Top to Bottom is:
1. **Top Bar:** App title, settings button, window controls.
2. **Main Workspace Row:** 
   - **Tree Sidebar** (Far Left)
   - **Settings Pane** (Next to Tree)
   - **Vertical Toolbar** (Center-Left)
   - **Right Main Area** (Flex column)
     - **Terminal Panes** (Top)
     - **Command Console** (Bottom)

## 4. Component Specifications

### A. Tree Sidebar
- **Layout:** Resizable width (`min 200px`, `max 400px`).
- **Search Box:** Sticky at the top. **MUST autofocus** when the app opens.
- **Tree Nodes:** Folders and connections. Highlight matched text from search. Active connection gets a subtle accent background.

### B. Settings Pane
- **Layout:** Resizable width (`min 250px`, `max 450px`). Located between the Tree and the Toolbar.
- **Search Box:** Sticky at the top. **MUST autofocus** when the pane is focused/opened. Filters parameters live.
- **Grid Interface:** CSS Grid with `grid-template-columns: 100px 1fr`. 
  - Left Column (Parameter): Muted text, right-aligned or padded.
  - Right Column (Value): Primary text, looks like an input field (`bg-base`, subtle border).
  - Rows: Host IP, Port, Username, Auth Method, SSH Key, etc.

### C. Vertical Toolbar
- **Layout:** Fixed width (`48px`), full height of the workspace. Placed between the Settings Pane and the Terminal area.
- **Top Section Icons:**
  - Toggle Tree (Bars icon)
  - Toggle Settings (Gear icon)
  - Divider line
  - Tile Panels (Grid icon)
  - Split Panels (Columns icon)
- **Bottom Section Icons:**
  - Toggle Command Console (Terminal icon). Pinned to the bottom using `flex-1` spacer.
- **State:** Active states use `--bg-surface` and `--accent` icons. Inactive use `--text-muted`.

### D. Terminal Panes (Main Area)
- **Layout:** A grid or flex container holding multiple terminal panes.
- **Tabs:** Each pane has a tab bar at the top. Active tabs use `--bg-surface` and top border `--accent`. Inactive tabs use `--bg-panel` and `--text-muted`. Include a "+" button for new tabs.
- **Resizability:** The individual terminal panes MUST be resizable (both horizontally and vertically). Use nested `paneforge` groups or CSS `resize: both` on the pane containers.
- **Active State:** The currently focused terminal pane gets a 1px `--accent` border.
- **3-Pane Default:** The initial layout should show 3 panes (e.g., 2 on top, 1 spanning the bottom, or a 2x2 grid).

### E. Command Console (Bottom Panel)
- **Layout:** Located below the terminal panes, to the right of the toolbar. Height is resizable (drag handle on the top edge, `min 100px`, `max 400px`).
- **Two Sections (Split):**
  - **Left Section (Active Connections):** Resizable width. Contains a dropdown ("Send to: Active session") and a list of open connections.
  - **Right Section (Command Input):** Flex-grows to fill space. Contains a textarea and a "Send" button (styled with `--accent`).
  - **Split Resizing:** The divider between the Left and Right sections MUST be draggable to resize the width of the connections list.

## 5. Interaction & Accessibility Rules (CRITICAL)
- **Autofocus:** Every list, tree, or modal with a search box MUST have `autofocus` enabled on the search input when it becomes visible.
- **Keyboard Navigation:**
  - `Ctrl+K` / `Cmd+K`: Global search focus.
  - `Ctrl+J`: Toggle Command Console.
  - `ArrowUp/Down`: Navigate trees and lists.
  - `Enter`: Select/Connect. `Escape`: Close modal.
  - Ensure all interactive elements (buttons, inputs, tree nodes) have a visible `:focus-visible` state using `outline: 2px solid var(--accent)`.
- **Resizing Handles:** All draggable split handles must be at least `4px` wide/tall, with a hover state that changes the background to `--accent`.

## 6. Implementation Guidelines for AI
1. Use Svelte 5 syntax (runes like `$state`, `$derived`).
2. NEVER hardcode colors. Use the CSS variables defined in Section 2.
3. Structure the DOM exactly as defined in Section 3 (Layout Hierarchy).
4. Use `paneforge` for all structural resizing. Do not rely on static flexbox for the main window layout.
5. Ensure all modals and panes follow the Autofocus and Keyboard Navigation rules.