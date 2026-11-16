# Casemove Rewrite Plan

Incremental (strangler) rewrite of Casemove from Electron/React into a Rust
core + native egui (eframe) UI application, with no JavaScript in the new
codebase.

Motivation: maintainability/modularity, reliability bug fixes, modernized stack,
performance, and first-class support for Steam + third-party market APIs.

## Licensing baseline (2026-11-16)

The retained legacy reference is GPL-era commit
`4c7cfc920120203785ec579581cfb6be851e4ab6`, not later upstream revisions
licensed CC BY-NC-ND. The two later bundled schema/translation snapshots have
been restored to that baseline. Application source now targets GPL version 3
or later; third-party rights and complete release obligations remain under
review. This is a source-guided port, not a clean-room rewrite. See
[BUILDING.md](BUILDING.md#license-and-gpl-era-provenance) and [NOTICE](../NOTICE).

## Current Progress (2026-11-14)

The Rust rewrite has reached feature parity on every page; the remaining work
is packaging and live-account verification, tracked in
[FEATURE_PARITY_CHECKLIST.md](FEATURE_PARITY_CHECKLIST.md).

- **Shell**: eframe + egui, native (no webview, WASM, or JavaScript). Pages:
  Overview, Inventory, Storage, Trade-ups, Settings, plus the login screen and
  saved-account sidebar. Charcoal theme, local QR rendering, off-thread image
  loading into a texture cache.
- **Login**: password with Guard code, shared-secret device TOTP, mobile
  approval, QR (`steam-auth-rs`), browser-token (`steam-client-rs`), and
  remembered refresh tokens in the OS keyring. Credential buffers zeroize; the
  legacy `electron-store` encryption key is gone. Legacy preferences and account
  metadata are imported once without touching the legacy store.
- **Connection**: sign-in attaches to the CS2 GC automatically. The retained GC
  client streams SO create/update/remove deltas; a CM logon monitor reports
  `ClientLoggedOff`/`ClientLogOnResponse` as a connection state and retries the
  attach with 5/15/30/60 s backoff, alongside a manual Retry connection button.
- **Inventory**: search, filters, sorts, grouping, and 50-row pagination;
  configurable persisted columns. Bundled schema immediately, live Skinledger VDF
  and CSGO-API image catalogs cached for 24 h, with bundled fallback.
- **Storage**: contents, bulk moves, withdraw, rename, fast mode, and container
  opening; notification-correlated acks with timeouts owned by the core.
- **Trade-ups**: picker with wear/StatTrak locks and storage-unit inputs,
  outcomes/EV from the bundled `collections.json`, review dialog, and the
  hand-ported craft payload.
- **Overview, Settings, export**: value/curve overview, currency conversion from
  the bundled snapshot, Steam-close prompt, dev-mode toggle, update check, and
  the legacy `downloadReport` CSV export.
- **Open gates**: see [MISSING_FEATURES.md](MISSING_FEATURES.md) — live-account
  verification, the SO field 4/5 wire question, metrics, and packaging. The old
  Electron app was removed on 2026-11-14.

## Strategy

- **Dual-app strangler** (finished): the Electron app stayed shippable while the
  Rust app grew feature-by-feature beside it. It was deleted on 2026-11-14 once
  every page was ported; the remaining work is in `docs/MISSING_FEATURES.md`.
  Zero JavaScript remains in the repository.
- **Shared data**: the old app's bundled snapshots (prices, currency,
  collections, item schema and translations) live in `data/`.
- The strangler seam is a single typed **command/event contract** (replacing the
  29 IPC channels + the `userEvents` mega-channel). This contract doubles as the
  parity checklist.

## Target architecture

```
casemove/
├── src/                     # egui frontend (native eframe app, no JS, no WASM)
├── crates/
│   ├── app-core/            # command layer: Core methods over shared state
│   ├── app-commands/        # typed command + event contract (the strangler seam)
│   ├── steam-core/          # login strategies, session, CM events
│   ├── gc-cs2/              # GC session, SO cache, casket ops  ← critical path
│   ├── item-schema/         # schema, translations, and image catalogs
│   ├── pricing/             # PriceProvider trait: Steam, Buff163, Skinport, … + FX
│   └── domain/              # trade-up math, classification, item model (pure, tested)
├── data/                    # bundled snapshots: prices, currency, collections, item schema
├── docs/                    # this plan, missing features, metrics, command contract
└── .github/workflows/       # cargo clippy/test
```

## Phases

### Phase 0 — Risk retirement (throwaway spikes) — done

1. **Spike A: Rust Steam login.** Evaluate `steam-vent` (0.5.0, has
   `GCHandshake`), `steam-client-rs` (0.2.0, mirrors node-steam-user incl.
   `send_to_gc`), and `steam-auth-rs` (port of node-steam-session covering all
   5 login strategies: clientjstoken / refresh-token / TOTP / guard / password).
   Deliverable: pick one primary crate behind an adapter.
2. **Spike B (go/no-go): GC session.** Connect to the CS2 GC,
   `CMsgClientHello` → `ClientWelcome`, subscribe to the SO cache, parse
   inventory into typed `SOEconItem`s. Cross-check against `globaloffensive`
   output for the same account. **Contingency if this fails: a thin Node
   sidecar running `globaloffensive` behind the same `app-commands` interface —
   the contract makes the swap invisible to the UI.**
3. **Spike C: UI shell.** Tauri 2 + Leptos was evaluated and replaced by
   native eframe/egui (see Current Progress).

### Phase 1 — Foundation

- ~~Move current project to `legacy/`~~ (done in Phase 1; removed 2026-11-14),
  scaffold Cargo workspace, CI.
- Write the command/event contract from the existing 29 IPC channels + 7 routes
  + README feature list (this is the spec).
- Settings store on OS keyring (fixes `encryptionKey: 'this_only_obfuscates'`),
  one-time import of the legacy `electron-store` JSON (saved accounts, prefs).

### Phase 2 — Rust core (dependency order) — done

1. `item-schema` — port `inventoryConverter` (613 lines, pure); bundled JSON
   first, live schema later.
2. `pricing` — `PriceProvider` trait; bundled snapshot as fallback provider
   (replaces the dead `url = ''` path with real Steam Market/Buff/Skinport
   calls), FX rates.
3. `steam-core` + `gc-cs2` — login flows, GC session, inventory events.
   Session state behind `Arc<Mutex<_>>`, not per-login closures, so re-login
   cannot duplicate listeners.
4. Storage-unit ops (`addToCasket` / `removeFromCasket` / `nameItem` /
   positions / equip) with correct `itemCustomizationNotification` correlation
   and fast-mode semantics.
5. Trade-up math + crate-open/craft payloads (ported from `tradeup.tsx` /
   ByteBuffer logic).
6. Typed events replacing the `[1|2|3|4, …]` tuples.

### Phase 3 — UI parity, page by page

Login (saved accounts, QR) → Inventory → **Storage units from/to + move modal
(the core feature)** → Settings → Overview/charts → Trade-up. Each page ends
with its legacy counterpart deletable.

Filters/sort ported from `inventoryFunctions.tsx` (494 lines, 22 importers)
with golden tests against legacy outputs; virtualized tables (legacy renders
every row).

### Phase 4 — Multi-API expansion

Steam Web API (market prices, item schema), additional market providers behind
the trait, rate-limiting/caching.

### Phase 5 — Cutover

Parity checklist sign-off, a native packager (cargo-dist/cargo-packager),
sign/notarize equivalents for `electron-builder`.

## Risks

| Risk | Mitigation |
|---|---|
| No Rust equivalent of `globaloffensive` — GC session layer must be reimplemented | Spike B first; `globaloffensive` is MIT so its source is a legitimate reference; Node sidecar contingency behind the contract |
| Pre-1.0 Steam crates (`steam-vent`, `steam-client-rs`, `steam-auth-rs`) | Adapter traits + pinned versions + sidecar contingency |
| CS2 protobuf drift | Automated proto sync from `SteamDatabase/GameTracking-CS2` (as `node-cs2` does) |
| ~~Linux webview variance (WebKitGTK)~~ | Retired: the UI is native egui, no webview |
| Two apps drifting during transition | Parity checklist + shared golden fixtures |

## Testing

- **Characterization tests**: the old code was the spec — item conversion,
  filtering/sorting, trade-up math, and price lookup are frozen as literal
  expectations (`src/inventory_golden.rs`, crate tests). With the old app gone,
  treat these goldens as the reference when the behavior is questioned.
- `criterion` micro-benchmarks for pure core logic.
- The existing `App.test.tsx` asserts nothing; it is not carried over.

## Metrics

### Baseline

The pre-rewrite baseline is recorded in `docs/metrics/baseline-legacy.json`.
The scenarios below run against the Rust app alone now.

Shared benchmark scenarios (identical script for both apps):

| # | Scenario | Measures |
|---|---|---|
| S1 | Cold start → login screen visible | startup latency |
| S2 | Refresh-token login → inventory rendered | login + GC session + first paint |
| S3 | Load all storage units | `getCasketContents` round-trips |
| S4 | Filter + sort a 1,000-item inventory × 50 iterations | interaction latency (p50/p95) |
| S5 | Bulk-move 50 items, fast mode | throughput (items/s) |
| S6 | 30-min soak: 10× re-login, 5× disconnect/reconnect | leaks, memory growth, failures |

### Metric categories & initial targets

**Performance**

- Cold start: ≤ 50% of baseline (no Chromium boot)
- Login→inventory (S2): no regression at minimum; target ≤ baseline
- Filter/sort p95 (S4): < 16 ms on the UI thread (under one frame)
- Bulk-move throughput (S5): ≥ legacy
- Steady-state RSS (S6): flat growth; ≤ 50% of legacy
- Installer size: record baseline (Electron ~150–200 MB) → native target ~15–30 MB

**Reliability**

- Listener count after 10× re-login (S6): **exactly the startup count**
  (legacy duplicates 12 handlers per re-login; hard gate)
- Move failure rate: failures/total tracked per run; 0 unexplained
- GC session re-establish time after disconnect: record, then budget

**Code quality**

- Golden-test coverage: 100% of the ported pure-function set
- Contract integrity: command/event names defined in exactly one place
  (legacy defines channels in 3 places)
- Dead code: 0; CI fails on unused exports (legacy has ~10 dead files)
- `cargo clippy -D warnings` from Phase 1 onward

### Where metrics gate the phases

- **Phase 0**: record login latency + GC-session establishment vs legacy —
  feeds the Spike B go/no-go
- **Phase 2**: `criterion` benchmarks for item conversion and pricing lookup;
  assert ≥ legacy throughput
- **Phase 3**: each page lands with its scenario numbers (S4 for
  inventory/storage pages)
- **Phase 5 cutover gate**: all S1–S6 scenarios pass targets vs baseline,
  plus the listener-count and failure-rate hard gates

### Enforcement

- Scenario runner is one command per app; output as JSON committed under
  `docs/metrics/` so changes over time are diffable
- CI runs micro-benchmarks + unit/golden tests and fails on budget regression;
  full S1–S6 stays manual/weekly (requires real Steam accounts)
