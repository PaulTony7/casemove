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
have the Tauri IPC bridge, so saved accounts and Steam login are only available
in the desktop app. Regular login accepts a username/password and an optional
Steam Guard code; Steam may instead request approval in its mobile app. Login
times out after 120 seconds and can be cancelled. Password and code fields clear
on submission. Remember for later stores only the Steam refresh token in the
OS keyring; account JSON and IPC replies contain metadata, not tokens. Secure
storage failures leave the Steam session signed in and display a warning.
Use the Sign in with saved login icon beside a remembered account to connect
without re-entering a password. Migrated metadata without a new keyring token
still requires password login; missing or rejected tokens produce an error.
Removing a remembered account also removes its keyring token. Shared-secret,
QR, and browser-token login are not implemented yet. Real-account remembered
login and OS-keyring verification remain manual checks.

After signing in, open Inventory and choose Connect to CS2. Close running games
first: the GC handshake marks this account as playing CS2. The read-only snapshot
includes item names, wear, seed, nametags, and IDs; search/filter/sort and paging
operate locally. Refresh has a 30-second GC timeout and reuses the established
GC dispatcher. If the initial connection fails, sign out and sign back in before
reconnecting; do not repeatedly create coordinators on one Steam session.
Sign-out cancels requests and clears the displayed snapshot. Storage
unit contents, transfers, and automatic live inventory updates are not ported.

On first desktop launch, legacy account metadata and preferences are imported
into `settings.json` in Tauri's app configuration directory. Legacy files are
left unchanged and encrypted credentials are not copied.

Offline module tests: `cargo test -p steam-core -p gc-cs2 -p item-schema`.
They do not contact Steam or require credentials. The production desktop login
and inventory integration still needs a manual real-account check; enter
credentials directly in the desktop app, not in test fixtures or logs.