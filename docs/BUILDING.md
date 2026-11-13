# Building Casemove

The current app uses Rust, Leptos, Trunk, and Tauri 2. The Electron app under
`legacy/` has a separate build process.

## Prerequisites

Install Rust through rustup. The repository's `rust-toolchain.toml` selects the
stable toolchain and installs the `wasm32-unknown-unknown` target automatically.
On Windows, install Visual Studio Build Tools with the Desktop development with
C++ workload and a Windows SDK. The desktop app requires the WebView2 runtime.

Install the frontend and desktop build tools:

```powershell
cargo install trunk --version 0.22.1 --locked
cargo install tauri-cli --version 2.12.4 --locked
```

## Build

From the repository root:

```powershell
cargo tauri build
```

This builds the WASM frontend and release desktop app, then packages installers.
On Windows, the executable is `target/release/casemove.exe`; MSI and NSIS
installers are under `target/release/bundle/`. Tauri downloads its WiX and NSIS
packaging tools on first use, so the first build needs internet access.

For native workspace compilation without frontend packaging:

```powershell
cargo build --workspace
```

## Development

```powershell
cargo tauri dev
```

Tauri starts Trunk automatically at `http://localhost:1420`.

For a frontend-only preview, run `trunk serve --port 1421`. The browser does not
have the Tauri IPC bridge, so saved accounts are only loaded in the desktop app.
Steam authentication is not yet implemented; sign-in is deliberately disabled.
On first desktop launch, legacy account metadata and preferences are imported
into `settings.json` in Tauri's app configuration directory. Legacy files are
left unchanged and encrypted credentials are not copied.