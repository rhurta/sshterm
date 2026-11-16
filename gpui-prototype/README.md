# GPUI smoke test — stage 1

This first stage is intentionally only a minimal native GPUI window. It does not touch the existing Svelte + Tauri application.

## Windows prerequisites

- Latest stable Rust via [rustup](https://rustup.rs/).
- Visual Studio 2022+ or Build Tools with the **Desktop development with C++** workload, MSVC x64/x86 build tools, Spectre-mitigated libraries, and Windows 10/11 SDK.
- CMake on `PATH`.
- Use a Visual Studio Developer PowerShell / x64 Native Tools prompt when using standalone Build Tools.

See the upstream [Zed Windows build guide](https://github.com/zed-industries/zed/blob/main/docs/src/development/windows.md).

## Build and launch

From the repository root:

```powershell
cargo run --manifest-path gpui-prototype/Cargo.toml --bin gpui-smoke
```

This downloads the official GPUI crates from Zed commit `125ca8672e928b19a62c2e4b761fb29fe2a34ae3` and opens a native 640×360 window. GPUI's upstream README documents `gpui_platform::application()` and notes that Windows needs no optional `gpui_platform` features (Win32 + DirectWrite are used).

The GitHub Actions workflow compiles this smoke target on Windows. A hosted CI runner can verify compilation but cannot provide an interactive desktop session; confirm the window launch locally before moving to stage 2.
