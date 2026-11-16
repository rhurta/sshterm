# sshterm — GPUI Kit prototype

This is an isolated native-Rust UI experiment. It does **not** replace or modify the production Svelte + Tauri application. The prototype is self-contained under `gpui-prototype/` and currently uses sample connection data.

## Status and scope

- Native GPUI Kit window with its reusable `Tree` component and one terminal placeholder pane.
- The Kit tree supplies hierarchical rendering, expand/collapse, selection, keyboard navigation, and virtualized rows.
- Drag an item onto a folder to move it into that folder.
- Drag an item onto another connection to insert it before that connection in the same parent list.
- Drag an item to the root drop zone to move it to the root.
- Folder-to-descendant and self-drops are rejected.
- Empty folders retain a disabled “Empty folder” child so the component continues to recognize them as folders/drop targets.
- No SSH session, persistence, authentication, or terminal emulator is connected yet.

## Windows prerequisites

1. Install the latest stable Rust toolchain with [rustup](https://rustup.rs/).
2. Install Visual Studio 2022 or newer Build Tools with **Desktop development with C++**, the MSVC x64/x86 build tools, and the matching Spectre-mitigated libraries.
3. Install a Windows 10/11 SDK.
4. Install CMake and ensure `cmake.exe` is on `PATH`.
5. Use an x64 Native Tools / Developer PowerShell prompt if you installed only the standalone Visual Studio Build Tools.

See the upstream [Zed Windows build guide](https://github.com/zed-industries/zed/blob/main/docs/src/development/windows.md) for current native toolchain details.

## Build and run (Windows)

From the repository root:

```powershell
# Stage 1: build and launch the minimal GPUI Kit smoke-test window.
cargo run --manifest-path gpui-prototype/Cargo.toml --bin gpui-smoke

# Stage 2: build and launch the connection-tree prototype.
cargo run --manifest-path gpui-prototype/Cargo.toml --bin sshterm-gpui

# Run the prototype's tree-model unit tests.
cargo test --manifest-path gpui-prototype/Cargo.toml
```

The prototype depends on the published `gpui-kit = "0.9.1"` facade. GPUI Kit pins a matching set of GPUI, GPUI Base, GPUI Component, and asset crates, avoiding a separately maintained set of incompatible GPUI dependency revisions.

## Architecture and decisions

- `gpui-smoke` is a minimal first-stage application. Run it before evaluating the larger UI; this separates native toolchain / GPUI integration failures from prototype behavior.
- `sshterm-gpui` owns a small in-memory connection model and delegates tree rendering, keyboard navigation, selection, and expand/collapse to GPUI Kit's `Tree` component.
- Drag payloads contain stable numeric node IDs. Drag sources and drop handlers are attached to the component's `ListItem` rows. The application model remains the source of truth for moves: folder drops append as children, connection drops insert before the target sibling, and the root drop zone appends at the root.
- The move guard checks the source subtree before mutation, preventing a folder from being moved into itself or any descendant.
- The Kit Tree infers folders from the presence of children. To keep an emptied folder droppable, the prototype inserts a disabled placeholder child.
- The existing backend already has pure tree operations in `src-tauri/src/store/tree.rs` and the `TreeNode` data model in `src-tauri/src/vault/model.rs`. This first prototype intentionally does not link the Tauri crate or duplicate backend dependencies. When persistence is added, extract the pure tree model/operations into a small shared Rust crate and consume it from both frontends.
- The terminal area is a visual placeholder. Keep the future session boundary separate from rendering: a session service should own SSH process lifecycle and stream terminal bytes/events to a terminal view model.

## Limitations

- Sample data only; edits are not persisted and are lost on exit.
- Folder drops append; connection drops insert before the target; root drops append. Fine-grained before/after insertion indicators are not implemented.
- Drag/drop behavior is attached to Kit's rendered rows rather than supplied as a built-in Tree feature.
- The terminal pane is static text, not a shell, PTY, SSH client, or emulator.
- No import, connection editor, credential vault, logging, localization, or production settings are wired in.
- GPUI Kit is actively developed; review its release notes when upgrading the pinned crate version.
- A remote GitHub edit cannot launch a Windows desktop window. The Windows CI build verifies compilation, but interactive launch and drag/drop behavior still need to be exercised on a Windows machine.

## Adding real SSH and terminal support

1. Extract the existing backend's transport/session interfaces and pure tree data types into a framework-independent Rust crate. Avoid making GPUI depend on Tauri commands or the current application binary.
2. Define an application-level session controller that accepts a connection ID and emits connection state, stdout/stderr bytes, exit status, and errors.
3. For Windows, evaluate the existing OpenSSH + ConPTY path already used by `src-tauri/src/session` and `src-tauri/src/transport`. Reuse that implementation only after extracting it from Tauri-specific state and command wiring.
4. Integrate a real terminal emulator as a separate component, with explicit input/output channels and resize events. Keep terminal rendering independent from SSH transport so local shells and SSH can share the same terminal widget.
5. Add persistence by reusing the existing vault format through a shared crate, preserving its encryption and migration rules. Do not create a second incompatible credentials store.

## GPUI Kit versus current Svelte + Tauri

| Area | GPUI Kit prototype | Current Svelte + Tauri |
|---|---|---|
| Rendering / performance | Native Rust UI and GPU-accelerated GPUI rendering; avoids webview layout and JS UI runtime. Potentially lower UI overhead, but real performance must be measured with representative trees and terminal output. | Svelte renders in the platform webview; Tauri keeps the backend native. Webview overhead exists, but the UI is mature and browser tooling is strong. |
| Complexity | One Rust language and a reusable component toolkit; no webview bridge for UI state. GPUI APIs and ecosystem are still evolving. | Two primary languages (TypeScript and Rust), a Tauri command/event boundary, and frontend build tooling. More layers, but established ecosystem and familiar web UI patterns. |
| Maintainability | Reusable Tree, ListItem, keyboard and accessibility behavior reduce custom UI code. The application still owns connection semantics and drag/drop rules. | Existing architecture already covers connection management, vault, sessions, imports, logs, and UI. Web components are easy to iterate, but UI and backend contracts must stay synchronized. |
| Windows | GPUI Kit supports Windows and provides native controls. Requires a working MSVC/CMake toolchain and should be tested on target Windows versions. | Tauri already has a working Windows build/CI path in this repository and uses WebView2; fewer changes are needed to preserve the current shipping path. |

This is a qualitative architecture comparison, not a benchmark. Compare startup time, idle memory, CPU during terminal output, rendering latency, executable size, keyboard navigation, and drag/drop behavior on the same Windows machine before considering a migration.

## GPUI Kit references

- [Tree component documentation](https://gpui-kit.com/component/tree/)
- [GPUI Kit installation and getting started](https://gpui-kit.com/docs/)
- [GPUI Kit crate documentation](https://docs.rs/gpui-kit/latest/gpui_kit/)
- [GPUI Kit source repository](https://github.com/longbridge/gpui-kit)
