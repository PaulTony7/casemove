# Building Casemove

The app is Rust + eframe/egui (native, no webview). Remaining work, including
packaging and the platform checks that still need a real account, is listed in
[MISSING_FEATURES.md](MISSING_FEATURES.md).

## License and GPL-era provenance

The application source targets `GPL-3.0-or-later`, not an MIT-only release.
The original Casemove application was created by Nombers; see [NOTICE](../NOTICE)
for attribution, modification notices and warranty terms, and [LICENSE](../LICENSE)
for the GPL text. The settings page also displays these notices.
This is a source-guided Rust port, not a clean-room rewrite.

The retained legacy reference is commit
`4c7cfc920120203785ec579581cfb6be851e4ab6` (2024-02-12), immediately before
`279869027004fe16164a200e8e8a6cf0a433b67d` changed the upstream license on
2024-03-20. The reference's README explicitly grants GPL version 3 or later.
Later licensing does not revoke the grant for those earlier versions, but
does not establish GPL permission for later additions.

The 2026-11-16 audit compared all 171 files in the pre-removal legacy `src/`
tree against that reference: 151 were identical, 19 changed, and one was added.
The comparison was also repeated before the rewrite's 2026 performance
instrumentation to separate upstream changes from rewrite work.

| Legacy area | Post-cutoff difference and disposition |
|---|---|
| Trade-ups, classification, CSV export, wallet codes and GC payloads | Already present in the GPL reference; retained. |
| Item converter | Only source-URL changes and formatting; conversion rules already present in the GPL reference. Live source access still requires a separate provider/data rights review. |
| Pricing | Legacy price download URL was disabled; current live market adapters are new Rust work, not a copy of the disabled-download change. |
| Inventory grouping/filtering/sorting | Later differences were rewrite performance instrumentation; retained rules already present in the GPL reference. |
| Deprecation modal and notices | Added after the cutoff; not present in the Rust UI. |
| Fastmove, price-source selection, CSV buttons and trade-up navigation | Later legacy restrictions/removals are not reproduced in the Rust app. |
| Saved accounts | Later single-account display restriction is not reproduced; multiple accounts remain supported. |
| Settings/main changes | Primarily formatting, type annotations and feature restrictions; no transplanted post-cutoff implementation was identified in the reviewed Rust counterparts. |

Bundled `data/items_game.json` and `data/csgo_english.json` were replaced with
the exact GPL-reference blobs (`a0b0921b7db35a396bcdfce61e7f26f65e591c65`
and `c338c77e7de49181688640f9a9a1dc892a68ca96`). This removes the carried-over
later snapshots, reducing bundled item definitions from 1,744 to 1,704 and
translation keys from 34,263 to 32,777. The same schema keys are retained;
newer items may need live data or display the existing unknown-item fallback.
The bundled collections, prices and currency files already match the
GPL-reference blobs and were not changed.

This audit is not legal clearance. Upstream GPL licensing does not establish
ownership or redistribution permission for Valve/provider datasets, images or
other third-party material. Live downloads and caches need that review too.
The dependency metadata review covered 578 of 655 registry packages in the
then-current lockfile; this is not a target-specific binary license inventory.
Bundled fonts, MPL-licensed components and other dependencies retain their own
terms. A license grant for application code does not replace those terms.

Before distributing binaries, complete the third-party rights/notice review
and provide the matching complete Corresponding Source, including necessary
build/install scripts, under the applicable GPL distribution conditions.
Release archives include LICENSE and NOTICE, but those files alone do not
satisfy every dependency, asset or source-delivery obligation. Keep provenance
records; squashing history is not a substitute for this review.

Validation on 2026-11-16: all five bundled JSON files and LICENSE matched the
GPL-reference hashes. The focused offline tests for `item-schema`, `domain`,
`pricing`, `app-core` and `casemove-ui` passed (128 tests; five live-network
tests ignored). UI Clippy with warnings denied and `git diff --check` passed.
The pre-existing missing-icon/resource-compiler warning remains; no installer,
live Steam session or third-party licensing clearance was verified.

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

## Releases

`.github/workflows/release.yml` runs on `v*` tags (or manually) and attaches
builds to the GitHub release:

| Platform | Artefacts |
|---|---|
| Linux | `casemove-<version>-linux-x86_64.tar.gz` (FHS layout: `usr/bin`, icon, `.desktop`), `casemove_<version>_amd64.deb` |
| Windows | `casemove-<version>-windows-x64.zip` (`casemove.exe` with embedded icon and version info, no console window) |
| macOS | `casemove-<version>-macos-*.zip` containing `Casemove.app` |

Release builds run the same pinned toolchain as CI. macOS bundles are ad-hoc
signed (`codesign -s -`), so Gatekeeper still warns on first launch; Windows
SmartScreen does the same until a signing certificate is configured. The in-app
update check reads the latest release, so publishing a `v*` tag is also what
makes the update banner appear.

## Inventory

The app connects to the CS2 Game Coordinator automatically after sign-in; there
is no separate connect step. Close running games first: the GC handshake marks
this account as playing CS2. The inventory reads a
welcome snapshot, then applies incremental SO create/update/remove messages
while the retained GC client is connected. Item enrichment currently includes
bundled names, wear, seed, stickers/patches, known collection metadata, music
kits, graffiti tint, storage counts, and trade-up eligibility. Inventory
columns are configurable and persisted locally.

The GPL-era bundled schema is available immediately. A validated live schema cache is
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
