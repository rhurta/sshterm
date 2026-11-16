# sshterm — GPUI prototype

This is an isolated native-Rust UI experiment. It does **not** replace or modify the production Svelte + Tauri application. The prototype is self-contained under `gpui-prototype/` and currently uses sample connection data.

## Status and scope

- Native GPUI window with a compact connection tree and one terminal placeholder pane.
- Folders expand and collapse.
- Drag a connection onto a folder to move it into that folder.
- Drag an item onto another connection to insert it before that connection in the same parent list.
- Drag an item to the root drop zone to move it to the root.
- Folder-to-descendant and self-drops are rejected.
- No SSH session, persistence, authentication, or terminal emulator is connected yet.

## Windows prerequisites

1. Install the latest stable Rust toolchain with [rustup](https://rustup.rs/). The upstream GPUI README recommends current stable Rust.
2. Install Visual Studio 2022 or newer Build Tools with **Desktop development with C++**, the MSVC x64/x86 build tools, and the matching Spectre-mitigated libraries. For Visual Studio 2026 / MSVC 14.50+, select the current MSVC x64/x86 build tools and Spectre-mitigated libraries components.
3. Install a Windows 10/11 SDK (the upstream Zed Windows guide lists Windows 10 SDK 10.0.20348.0 or newer).
4. Install CMake and ensure `cmake.exe` is on `PATH`.
5. Use an x64 Native Tools / Developer PowerShell prompt if you installed only the standalone Visual Studio Build Tools.

See the upstream [Zed Windows build guide](https://github.com/zed-industries/zed/blob/main/docs/src/development/windows.md) for current toolchain details.

## Build and run (Windows)

From the repository root:

```powershell
# Stage 1: build and launch the minimal GPUI smoke-test window first.
cargo run --manifest-path gpui-prototype/Cargo.toml --bin gpui-smoke

# Stage 2: build and launch the connection-tree prototype.
cargo run --manifest-path gpui-prototype/Cargo.toml --bin sshterm-gpui

# Run the prototype's tree-model unit tests.
cargo test --manifest-path gpui-prototype/Cargo.toml
```

The first build downloads the pinned upstream Zed source and compiles GPUI and its native dependencies. The dependency is pinned to upstream Zed commit `125ca8672e928b19a62c2e4b761fb29fe2a34ae3` so an upstream API change does not silently alter this experiment. To intentionally update GPUI, change the `rev` in `Cargo.toml` after reviewing upstream API changes.

Windows uses GPUI's Win32 windowing backend and DirectWrite text backend; the upstream README says no `gpui_platform` features are required on Windows. Linux-only `wayland` / `x11` features are intentionally not enabled here.

## Architecture and decisions

- `gpui-smoke` is a minimal first-stage application. Run it before evaluating the larger UI; this separates toolchain / GPUI integration failures from prototype behavior.
- `sshterm-gpui` owns a small in-memory tree model and renders it directly with GPUI. There is no webview, JavaScript runtime, Svelte, React, Electron, or Tauri dependency in this crate.
- Drag payloads contain stable node IDs. Drop targets are either a folder (append as child), a connection (insert before it in the same sibling list), or the explicit root zone.
- The move guard checks the source subtree before mutation, preventing a folder from being moved into itself or any descendant.
- The existing backend already has pure tree operations in `src-tauri/src/store/tree.rs` and the `TreeNode` data model in `src-tauri/src/vault/model.rs`. This first prototype intentionally does not link the Tauri crate or duplicate backend dependencies. When persistence is added, extract the pure tree model/operations into a small shared Rust crate and consume it from both frontends.
- The terminal area is a visual placeholder. Keep the future session boundary separate from rendering: a session service should own SSH process lifecycle and stream terminal bytes/events to a terminal view model.

## Limitations

- Sample data only; edits are not persisted and are lost on exit.
- The current drag interaction is deliberately small: folder drops append; connection drops insert before the target; root drops append. There is no insertion indicator or fine-grained before/after selection yet.
- The terminal pane is static text, not a shell, PTY, SSH client, or emulator.
- No import, connection editor, credential vault, logging, localization, or production settings are wired in.
- GPUI is pre-1.0 and actively developed; APIs may break when the pinned upstream revision is changed.
- A remote GitHub edit cannot launch a Windows desktop window. The smoke binary and Windows CI compile job are provided, but interactive launch and drag/drop behavior still need to be exercised on a Windows machine.

## Adding real SSH and terminal support

1. Extract the existing backend's transport/session interfaces and pure tree data types into a framework-independent Rust crate. Avoid making GPUI depend on Tauri commands or the current application binary.
2. Define an application-level session controller that accepts a connection ID and emits connection state, stdout/stderr bytes, exit status, and errors.
3. For Windows, evaluate the existing OpenSSH + ConPTY path already used by `src-tauri/src/session` and `src-tauri/src/transport`. Reuse that implementation only after extracting it from Tauri-specific state and command wiring.
4. Integrate a real terminal emulator as a separate component, with explicit input/output channels and resize events. Keep terminal rendering independent from SSH transport so local shells and SSH can share the same terminal widget.
5. Add persistence by reusing the existing vault format through a shared crate, preserving its encryption and migration rules. Do not create a second incompatible credentials store.

## GPUI versus current Svelte + Tauri

| Area | GPUI prototype | Current Svelte + Tauri |
|---|---|---|
| Rendering / performance | Native Rust UI and GPU-accelerated GPUI rendering; avoids webview layout and JS UI runtime. Potentially lower UI overhead, but real performance must be measured with representative trees and terminal output. | Svelte renders in the platform webview; Tauri keeps the backend native. Webview overhead exists, but the UI is mature and browser tooling is strong. |
| Complexity | One Rust language and one native UI stack; no frontend/backend bridge for UI state. GPUI APIs and ecosystem are still pre-1.0 and evolving. | Two primary languages (TypeScript and Rust), a Tauri command/event boundary, and frontend build tooling. More layers, but established ecosystem and familiar web UI patterns. |
| Maintainability | Could simplify cross-layer refactors and keep tree logic close to UI. Smaller ecosystem, upstream API churn, and need to build or source native controls increase maintenance risk. | Existing architecture already covers connection management, vault, sessions, imports, logs, and UI. Web components are easy to iterate, but UI and backend contracts must stay synchronized. |
| Windows | GPUI has a native Win32 platform backend and DirectWrite text. Requires a working MSVC/CMake toolchain and should be tested on the target Windows versions. | Tauri already has a working Windows build/CI path in this repository and uses WebView2; fewer changes are needed to preserve the current shipping path. |

This is a qualitative architecture comparison, not a benchmark. Compare startup time, idle memory, CPU during terminal output, rendering latency, executable size, and drag/drop behavior on the same Windows machine before considering a migration.

## Upstream GPUI references

- [GPUI README / getting started](https://github.com/zed-industries/zed/tree/main/crates/gpui)
- [Official GPUI hello-world example](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/hello_world.rs)
- [Official GPUI drag-and-drop example](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/drag_drop.rs)
- [Official GPUI examples index](https://github.com/zed-industries/zed/blob/main/crates/gpui/examples/README.md)
