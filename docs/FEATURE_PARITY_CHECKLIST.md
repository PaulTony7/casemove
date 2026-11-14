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

- [ ] Incremental SO-cache create/update/remove handling and automatic UI updates.
- [ ] Sticker/patch property filter (lands with sticker metadata below).
- [ ] Collection, sticker, and storage-name sorts.
- [ ] Sticker/patch metadata, images, and applied-item details.
- [ ] Music-kit, graffiti/tint, agent, and other specialized-item enrichment.
- [ ] Collection metadata and trade-up eligibility.
- [ ] Complete image coverage and live schema refresh with bundled fallback.
- [ ] Configurable inventory columns.

## Storage Units

- [ ] Search, filter, and sort storage contents.
- [ ] Pagination or virtualization for storage contents.
- [ ] Load all storage units and aggregate their contents.
- [ ] Display storage counts, capacity, names, and values in a storage-management view.
- [ ] Rename storage units.
- [ ] Reconcile storage counts/cache after items move during loading.

## Transfers And Bulk Moves

- [ ] Transfer-to page and destination-unit selection.
- [ ] Transfer-from page and source-unit selection.
- [ ] Select individual/grouped items and requested quantities.
- [ ] Select all filtered items and clear selection.
- [ ] Validate ownership, moveability, and destination capacity before sending.
- [ ] Add one item to a storage unit with correlated GC acknowledgement.
- [ ] Remove one item from a storage unit with correlated GC acknowledgement.
- [ ] Handle timeouts and ambiguous results without blindly retrying mutations.
- [ ] Bulk-move queue, review modal, and progress display.
- [ ] Partial-failure reporting and inventory/storage reconciliation.
- [ ] Fast-mode behavior, pacing, and acknowledgement semantics.
- [ ] Safe interruption/sign-out during a move operation.
- [ ] Item-position operations; confirm user-facing legacy usage before porting.
- [ ] Equip operations; confirm user-facing legacy usage before porting.

## Settings And Persistence

- [ ] Settings page and typed settings get/set commands.
- [ ] Persist column, currency, price-source, and relevant workflow preferences.
- [ ] Steam-close prompt preference and developer-mode controls.
- [ ] Verify real OS-keyring and legacy-migration behavior on supported platforms.

## Pricing, Overview, And Reports

- [ ] Currency conversion and locale-aware formatting.
- [ ] Overview page with totals and breakdowns.
- [ ] Overview charts and grouping controls.
- [ ] CSV export covering inventory/storage, prices, quantities, wear, and stickers.

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
- [ ] Establish the wire meaning of SO unknown fields 4/5, or remove unsupported assumptions.
- [ ] Verify moves/removals during storage loading and notification ordering.
- [ ] Golden characterization fixtures for conversion, grouping, filters, sorts, and trade-ups.
- [ ] CI for workspace tests and Clippy checks.
- [ ] Startup, filtering, bulk-move, memory, listener-count, and installer-size metrics.
- [ ] Re-login/disconnect/reconnect soak tests with stable dispatcher/listener counts.
- [ ] Platform packaging/signing and final migration/parity sign-off before deleting legacy.

## Optional Expansion (Not Legacy Parity)

Legacy's Buff163/Float source choices are locked; additional functioning market
providers are expansion work rather than a missing working legacy feature.

- [ ] Additional market providers and provider selection.
- [ ] Provider-specific rate limiting, caching, and fallback behavior.
