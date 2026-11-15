# Building Casemove

The app is Rust + eframe/egui (native, no webview). Remaining work, including
packaging and the platform checks that still need a real account, is listed in
[MISSING_FEATURES.md](MISSING_FEATURES.md).

## Prerequisites

Install the stable Rust toolchain selected by `rust-toolchain.toml`. On Windows,
install Visual Studio Build Tools with the Desktop development with C++ workload
and a Windows SDK.

## Build and run

From the repository root:

```powershell
cargo build --workspace
cargo run -p casemove-ui
```

Run the offline workspace tests and clippy with:

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets
```

The eframe app has not yet been packaged for distribution. Cross-platform
packaging and signing are tracked in `MISSING_FEATURES.md`.

## Inventory

The app connects to the CS2 Game Coordinator automatically after sign-in; there
is no separate connect step. Close running games first: the GC handshake marks
this account as playing CS2. The inventory reads a
welcome snapshot, then applies incremental SO create/update/remove messages
while the retained GC client is connected. Item enrichment currently includes
bundled names, wear, seed, stickers/patches, known collection metadata, music
kits, graffiti tint, storage counts, and trade-up eligibility. Inventory
columns are configurable and persisted locally.

The bundled schema is available immediately. A validated live schema cache is
used on later launches and refreshed at most once per 24 hours from the public
Skinledger VDF files. Steam-hosted item and sticker image URLs are indexed from
the public [CSGO-API dataset](https://github.com/ByMykel/CSGO-API); collection
images remain available from the bundled catalog. Failed refreshes retain the
last cached or bundled schema. Graffiti images use kit and tint IDs; standalone
stickers/patches use kit IDs, separately from case definition IDs. Older image
caches are invalidated automatically when these lookup rules change.
Offline tests cover VDF parsing, image URL
validation, and cache round-tripping. Storage content loads, moves, renames,
the withdraw flow, trade-ups, container opening, and CSV export are available
from the app.
