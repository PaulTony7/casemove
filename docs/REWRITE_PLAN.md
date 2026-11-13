# Casemove Rewrite Plan

Incremental (strangler) rewrite of Casemove from Electron/React into a Tauri 2 + Rust
core + Leptos UI application, with no JavaScript in the new codebase.

Motivation: maintainability/modularity, reliability bug fixes, modernized stack,
performance, and first-class support for Steam + third-party market APIs.

## Current Progress (2026-11-13)

- The Windows Tauri app builds and packages; it no longer opens the greeting
  template. The Leptos signed-out screen follows the legacy charcoal palette,
  Casemove branding, navigation, Steam login methods, and saved-account sidebar.
- Saved-account metadata is loaded through Rust commands. Selecting an account
  fills the username; removal is confirmed and persisted. Passwords and stored
  credential blobs are never returned to the frontend.
- First launch imports legacy preferences and account metadata from the native
  OS configuration directory, without modifying the legacy store. Existing Rust
  settings are preserved. Imported accounts require fresh authentication.
- The regular login form now calls `steam-vent` for password authentication with
  a supplied email/device Guard code or Steam mobile approval. A single native
  session owner retains the connection, bounds login to 120 seconds, rejects
  overlapping logins, and cancels pending authentication on sign-out. The UI
  displays session identity and supports cancellation and sign-out.
- Owned native password/code/token buffers are zeroized on drop. Remember for
  later persists refresh tokens only in the OS keyring, with secret-free account
  metadata and rollback on cancellation or metadata failure. Saved-account token
  lookup is bounded by the login timeout and cancellation. Keyring/real-account
  verification remains pending. Shared-secret login now validates Base64 input
  before networking and generates device-only TOTP codes locally with zeroizing
  secret ownership. Frontend credential buffers are cleared before awaiting IPC;
  shared secrets are not persisted. QR and browser-token login remain unimplemented.
- Read-only inventory now uses a retained CS2 GC client and a bounded welcome
  cache parser. Connecting is explicit because it marks the Steam account as
  playing CS2. Requests time out after 30 seconds; sign-out cancels in-flight
  work and invalidates only that session's client. Failed refreshes preserve the
  last UI snapshot and reuse the existing dispatcher. A failed initial attach
  requires sign-out/re-login: this avoids repeatedly creating detached dispatch
  tasks on one Steam connection in `steam-vent` 0.5.0.
- The inventory table supports search, name/rarity/float sorting, storage-unit
  filtering, and 50-row pagination. Names and paint finishes resolve from the
  bundled legacy schema; unknown items remain visible. Exact hashed thumbnails
  are used where the bundled catalog has one; other items show a fallback icon.
  Asset IDs remain strings across IPC. Reward-preview objects are excluded as
  in the legacy converter. Wear/seed, nametags, rarity, and StatTrak
  presence are shown; full sticker/music/graffiti enrichment is not yet ported.
- This is **not login or inventory parity**. Protocol/lifecycle/schema tests are
  offline; browser checks use a synthetic IPC test double. A live-account login
  and inventory load have not been verified for this production integration.
  Storage-unit contents, transfers, incremental SO-cache updates, connection-loss
  events, and other authenticated pages remain unimplemented. The native
  titlebar is retained; cross-platform packaging is not yet verified.
- Next vertical slice: verify the production flow with a real account, then add
  storage-unit contents and incremental inventory/connection events. The Phase 0 report in
  `docs/metrics/spike-b.md` records the earlier real-account spike; do not treat
  that as end-to-end verification of the production UI. Keep legacy code until
  its matching workflow and characterization tests pass.

## Strategy

- **Dual-app strangler**: the new Tauri app lives in this repo and grows
  feature-by-feature to parity; the Electron app stays shippable the whole time.
  Each parity milestone deletes the corresponding legacy code. Zero JavaScript in
  the new app.
- **Same repo, `legacy/` folder**: the existing Electron project moves wholesale
  into `legacy/` (paths intact, still buildable). New code is added alongside;
  shared data files become fixtures both apps can read.
- The strangler seam is a single typed **command/event contract** (replacing the
  29 IPC channels + the `userEvents` mega-channel). This contract doubles as the
  parity checklist.

## Target architecture

```
casemove/
├── legacy/                  # current Electron project (moved wholesale, still buildable)
├── src/                     # Leptos frontend (Rust -> WASM, trunk, zero JS)
├── src-tauri/               # Tauri shell: window, tray, command dispatch
├── crates/
│   ├── app-commands/        # typed command + event contract (the strangler seam)
│   ├── steam-core/          # login strategies, session, CM events
│   ├── gc-cs2/              # GC session, SO cache, casket ops  ← critical path
│   ├── item-schema/         # port of items/index.js inventoryConverter
│   ├── pricing/             # PriceProvider trait: Steam, Buff163, Skinport, … + FX
│   └── domain/              # trade-up math, filters/sort, item model (pure, tested)
├── data/                    # shared fixtures: prices.json, items_game.json, collections.json
├── docs/                    # this plan, metrics baselines, parity checklist
└── .github/workflows/       # cargo clippy/test + legacy lint
```

## Phases

### Phase 0 — Risk retirement (throwaway spikes)

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
3. **Spike C: Tauri 2 + Leptos + trunk shell.** Custom titlebar (legacy uses
   `frame: false` on Windows), dev loop, packaging on Win/mac/Linux
   (WebKitGTK is the Linux risk).

### Phase 1 — Foundation

- Move current project to `legacy/`, scaffold Cargo workspace, CI.
- Write the command/event contract from the existing 29 IPC channels + 7 routes
  + README feature list (this is the spec).
- Settings store on OS keyring (fixes `encryptionKey: 'this_only_obfuscates'`),
  one-time migration reader for the legacy `electron-store` JSON (saved
  accounts, prefs).

### Phase 2 — Rust core (dependency order)

1. `item-schema` — port `inventoryConverter` (613 lines, pure); bundled JSON
   first, live schema later.
2. `pricing` — `PriceProvider` trait; bundled snapshot as fallback provider
   (replaces the dead `url = ''` path with real Steam Market/Buff/Skinport
   calls), FX rates.
3. `steam-core` + `gc-cs2` — login flows, GC session, inventory events.
   Register all commands **once** at startup with session state behind
   `Arc<Mutex<_>>` — structurally fixes the re-login listener-duplication leak.
4. Storage-unit ops (`addToCasket` / `removeFromCasket` / `nameItem` /
   positions / equip) with correct `itemCustomizationNotification` correlation
   and fast-mode semantics.
5. Trade-up math + crate-open/craft payloads (ported from `tradeup.tsx` /
   ByteBuffer logic).
6. Typed events replacing the `[1|2|3|4, …]` tuples.

### Phase 3 — UI parity, page by page

Each page ends with its legacy counterpart deletable:

Login (saved accounts, QR) → Inventory → **Storage units from/to + move modal
(the core feature)** → Settings → Overview/charts → Trade-up.

Filters/sort ported from `inventoryFunctions.tsx` (494 lines, 22 importers)
with golden tests against legacy outputs; virtualized tables (legacy renders
every row).

### Phase 4 — Multi-API expansion

Steam Web API (market prices, item schema), additional market providers behind
the trait, rate-limiting/caching.

### Phase 5 — Cutover

Parity checklist sign-off, data migration on first run, Tauri bundler replaces
`electron-builder` (sign/notarize equivalents), delete `legacy/`.

## Risks

| Risk | Mitigation |
|---|---|
| No Rust equivalent of `globaloffensive` — GC session layer must be reimplemented | Spike B first; `globaloffensive` is MIT so its source is a legitimate reference; Node sidecar contingency behind the contract |
| Pre-1.0 Steam crates (`steam-vent`, `steam-client-rs`, `steam-auth-rs`) | Adapter traits + pinned versions + sidecar contingency |
| CS2 protobuf drift | Automated proto sync from `SteamDatabase/GameTracking-CS2` (as `node-cs2` does) |
| Linux webview variance (WebKitGTK) | Spike C packaging test on Linux early |
| Two apps drifting during transition | Parity checklist + shared golden fixtures |

## Testing

- **Characterization tests before deletion**: legacy code becomes the spec —
  freeze outputs of item conversion, filtering/sorting, trade-up math, and
  price lookup as golden fixtures in `data/`; assert Rust matches.
- `criterion` micro-benchmarks for pure core logic.
- The existing `App.test.tsx` asserts nothing; it is not carried over.

## Metrics

### Baseline first (before `legacy/` is touched)

Add lightweight timing probes to the legacy app around the same points the new
app instruments, run the scripted benchmark scenario suite, and record results
in `docs/metrics-baseline.md`. The same scenarios run against the Tauri app at
every milestone, side by side.

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
- Filter/sort p95 (S4): < 16 ms in WASM (under one frame)
- Bulk-move throughput (S5): ≥ legacy
- Steady-state RSS (S6): flat growth; ≤ 50% of legacy
- Installer size: record baseline (Electron ~150–200 MB) → Tauri target ~15–30 MB

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
