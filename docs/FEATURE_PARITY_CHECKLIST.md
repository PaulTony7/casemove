# Feature Parity Checklist

Snapshot: 2026-11-14. Compared against the legacy application and the current
Rust working tree. Only outstanding work is listed — completed parity rows
live in git history.

- `[ ]` means implementation or the specifically named verification remains.
- Sections group related work; their order is **not a priority ranking**.
- Keep legacy workflows until replacement behavior and characterization tests
  are verified. Use [COMMANDS.md](COMMANDS.md) for IPC contracts and parity status,
  and [REWRITE_PLAN.md](REWRITE_PLAN.md) for migration phases.

## Application And Build

- [ ] Windows executable and installer packaging for the eframe app (the
  Tauri MSI/NSIS path was removed in the native-UI pivot).
- [ ] Custom Windows titlebar and minimize/maximize/close controls.
- [ ] Update checks, update status, and restart/download workflow.
- [ ] Verify macOS Intel and Apple Silicon packaging.
- [ ] Verify Linux packaging and native rendering behavior.

## Login And Accounts

- [ ] Steam-client-open prompt and close/continue handling.
- [ ] Automatic Steam/GC reconnection and accurate connection-loss status.

## Inventory And Item Data

- [ ] Verify live item/sticker image coverage and cached/bundled fallback.
- [ ] Complete agent-specific and remaining specialized-item enrichment.

The app loads the bundled schema immediately, uses a cached live schema when
available, and refreshes it at most once per 24 hours. Live VDF schema and
translation data come from Skinledger; Steam-hosted image catalogs for skins,
stickers, agents, collectibles, graffiti, music kits, crates, and keys come from
the public CSGO-API dataset. Failed refreshes keep the last cached or bundled
schema. Live-account image rendering and offline-fallback behavior still need
verification; agent-specific enrichment remains incomplete.
Graffiti thumbnails are indexed by kit and tint, standalone stickers use their
sticker-kit images, and case images use inventory definition IDs. Offline
conversion tests and opt-in live catalog/download tests cover these paths;
live-account rendering is still unverified.

**Recommended next feature:** Item/sticker image coverage and agent-specific
enrichment, then trade-ups.

## Storage Units

- [x] Search, filter, and sort storage contents.
- [x] Pagination or virtualization for storage contents.
- [x] Display storage counts, capacity, names, and values in a storage-management view.
- [x] Rename storage units.
- [x] Reconcile storage counts/cache after items move during loading.

## Transfers And Bulk Moves

- [x] Transfer-to page and destination-unit selection.
- [x] Transfer-from page and source-unit selection.
- [x] Select individual/grouped items and requested quantities.
- [x] Select all filtered items and clear selection.
- [x] Validate ownership, moveability, and destination capacity before sending.
- [x] Add one item to a storage unit with correlated GC acknowledgement.
- [x] Remove one item from a storage unit with correlated GC acknowledgement.
- [x] Handle timeouts and ambiguous results without blindly retrying mutations.
- [x] Bulk-move queue, review modal, and progress display.
- [x] Partial-failure reporting and inventory/storage reconciliation.
- [x] Fast-mode behavior, pacing, and acknowledgement semantics.
- [x] Safe interruption/sign-out during a move operation.
- [ ] Item-position operations; confirm user-facing legacy usage before porting.
- [ ] Equip operations; confirm user-facing legacy usage before porting.

## Settings And Persistence

- [x] Settings page and typed settings get/set commands.
- [x] Persist column, currency, price-source, and relevant workflow preferences.
- [ ] Steam-close prompt preference and developer-mode controls. (Prompt preference persists and shows in Settings; the popup itself and devmode controls are still missing.)
- [ ] Verify real OS-keyring and legacy-migration behavior on supported platforms.

## Pricing, Overview, And Reports

- [x] Currency conversion and locale-aware formatting.
- [x] CSV export covering inventory/storage, prices, quantities, wear, and stickers.

## Trade-Ups And Containers

- [ ] Trade-up input picker and inventory/storage filters.
- [ ] Outcome calculation, probabilities, and resulting floats.
- [ ] Input costs, expected value, profit, and review UI.
- [ ] Execute a trade-up contract and reconcile the result.
- [ ] Container-opening operation; confirm intended legacy UI scope before porting.

## Verification And Release Gates

- [ ] Confirm the ClientHello correction resolves the reported live inventory timeout.
- [ ] Verify OS-keyring save/read/delete and remembered-login reuse after restart with a real account.
- [ ] Verify shared-secret login, email-only challenges, clock skew, and code rollover against a real account.
- [ ] Compare real-account inventory IDs/counts and visible metadata against legacy.
- [ ] Verify empty and non-empty storage units through native IPC and the live GC.
- [ ] Verify live schema refresh, cached/bundled fallback, and item/sticker image rendering.
- [ ] Establish the wire meaning of SO unknown fields 4/5, or remove unsupported assumptions.
- [ ] Verify moves/removals during storage loading and notification ordering.
- [ ] Golden characterization fixtures for conversion, grouping, filters, sorts, and trade-ups.
- [x] CI for workspace tests and Clippy checks.
- [ ] Startup, filtering, bulk-move, memory, listener-count, and installer-size metrics.
- [ ] Re-login/disconnect/reconnect soak tests with stable dispatcher/listener counts.
- [ ] Platform packaging/signing and final migration/parity sign-off before deleting legacy.

## Optional Expansion (Not Legacy Parity)

Legacy's Buff163/Float source choices are locked; additional functioning market
providers are expansion work rather than a missing working legacy feature.

- [ ] Additional market providers and provider selection.
- [ ] Provider-specific rate limiting, caching, and fallback behavior.
