# Feature Parity Checklist

Snapshot: 2026-11-13. Compared against the legacy application and the current
Rust working tree, including the pending storage-contents integration.

- `[x]` means an initial Rust implementation exists. It does **not** mean full
  legacy parity, live Steam verification, or that the change is committed.
- `[ ]` means implementation or the specifically named verification remains.
- Sections group related work; their order is **not a priority ranking**.
- Keep legacy workflows until replacement behavior and characterization tests
  are verified. Use [COMMANDS.md](COMMANDS.md) for IPC contracts and parity status,
  and [REWRITE_PLAN.md](REWRITE_PLAN.md) for migration phases.

## Application And Build

- [x] Rust workspace with Tauri 2 and Leptos frontend.
- [x] Casemove branding, legacy-style dark shell, and responsive navigation.
- [x] Windows executable and MSI/NSIS packaging.
- [x] Documented build tools and declared WASM target.
- [ ] Custom Windows titlebar and minimize/maximize/close controls.
- [ ] Update checks, update status, and restart/download workflow.
- [ ] Verify macOS Intel and Apple Silicon packaging.
- [ ] Verify Linux packaging and webview behavior.

## Login And Accounts

- [x] Password login through the native Steam client adapter.
- [x] Supplied email/device Steam Guard code and mobile-approval handling.
- [x] Login progress, actionable errors, timeout, and cancellation.
- [x] Retained session, sign-out, and rejection of late replies after re-login.
- [x] Clear password/code input fields on submission; zeroize owned native credentials.
- [x] List, select, and remove saved-account metadata; selection fills the username.
- [x] Native keyring wrapper integrated with opt-in remembered login.
- [x] Remember login using refresh tokens stored in the OS keyring (initial implementation; live verification pending).
- [x] Sign in using a saved account without entering password/Guard again (initial implementation; live verification pending).
- [x] Shared-secret/TOTP login with validated Base64 input and local device-code generation (live verification pending).
- [x] QR challenge, approval, cancellation, and retry workflow (initial implementation; live verification pending).
- [x] Browser-token login and input validation (initial implementation; live verification pending).
- [x] Profile avatars and display names fetched post-login from steamcommunity XML (allowlisted hosts, including the current fastly CDN); falls back to the account name for limited profiles.
- [x] Wallet balance from `CMsgClientWalletInfoUpdate` (raw cents ÷ 100, legacy currency-code map) in login replies and `wallet:changed` events (initial implementation; live verification pending).
- [x] Saved-account ordering controls. Legacy account-limit behavior remains to verify.
- [ ] Steam-client-open prompt and close/continue handling.
- [ ] Automatic Steam/GC reconnection and accurate connection-loss status.

## Inventory And Item Data

- [x] Explicit CS2 GC connection and bounded inventory-snapshot requests.
- [x] Legacy-compatible explicit ClientHello fields for attach and refresh.
- [x] Typed economy-cache decoding with corruption, duplicate-ID, and size checks.
- [x] Preserve 64-bit item IDs as strings across IPC.
- [x] Session-scoped GC ownership and cancellation of inventory work on sign-out.
- [x] Exclude reward-preview objects from owned inventory.
- [x] Bundled weapon/paint names, prefab inheritance, and unknown-item fallbacks.
- [x] Exact thumbnails from the bundled image catalog with fallback icons.
- [x] Display nametags, rarity, wear/float, paint seed, quantity, and StatTrak presence.
- [x] Inventory search by name, nametag, and item ID.
- [x] Basic position, name, rarity, and float sorting.
- [x] Storage-unit-only filter and 50-row inventory pagination.
- [x] Refresh errors preserve the last displayed snapshot.
- [ ] Incremental SO-cache create/update/remove handling and automatic UI updates.
- [ ] Group equivalent items, retain their individual IDs, and show combined quantities.
- [ ] Category and subcategory classification/filtering.
- [ ] Rarity-selection and item-property filters, including tradehold and stickers.
- [ ] Quantity, price, category, collection, sticker, tradehold, and storage-name sorts.
- [ ] Match legacy name normalization, sort direction, and tie-breaking behavior.
- [ ] Full StatTrak, Souvenir, and knife-name formatting.
- [ ] Sticker/patch metadata, images, and applied-item details.
- [ ] Music-kit, graffiti/tint, agent, and other specialized-item enrichment.
- [ ] Collection metadata and trade-up eligibility.
- [ ] Tradehold/unlock dates, origin, moveability, and equipped state.
- [ ] Complete image coverage and live schema refresh with bundled fallback.
- [ ] Configurable inventory columns.

## Storage Units

- [x] Open a storage unit's contents from its inventory row.
- [x] Validate storage IDs against the current session inventory.
- [x] Read expected contents count from attribute 270.
- [x] Collect matching GC objects and wait for notification/count completion.
- [x] Distinguish empty units, incomplete results, invalid requests, and timeouts.
- [x] Reject overlapping native inventory/contents requests and discard stale UI replies.
- [x] Display read-only contents and return to inventory.
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

- [x] Persistent JSON settings store with dot-path access.
- [x] Legacy preferences/account-metadata migration without copying encrypted secrets.
- [x] Preserve existing Rust settings and avoid reimporting deleted profiles.
- [ ] Settings page and typed settings get/set commands.
- [ ] Persist column, currency, price-source, and relevant workflow preferences.
- [ ] Steam-close prompt preference and developer-mode controls.
- [x] Keyring cleanup when a remembered account is removed, with rollback on metadata-write failure.
- [ ] Verify real OS-keyring and legacy-migration behavior on supported platforms.

## Pricing, Overview, And Reports

- [ ] Steam Market price lookup, cache, fallback, and unavailable-price handling.
- [ ] Currency conversion and locale-aware formatting.
- [ ] Per-item, grouped, inventory, and storage-unit valuations.
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

- [x] Offline tests for implemented protocol, lifecycle, migration, and schema slices.
- [x] Current workspace test and warnings-as-errors Clippy checks pass.
- [x] WASM/frontend build and Windows packaging checked.
- [x] Browser IPC-double checks for the initial inventory UI; not native IPC proof.
- [ ] Confirm the ClientHello correction resolves the reported live inventory timeout.
- [ ] Verify OS-keyring save/read/delete and remembered-login reuse after restart with a real account.
- [ ] Verify shared-secret login, email-only challenges, clock skew, and code rollover against a real account.
- [ ] Compare real-account inventory IDs/counts and visible metadata against legacy.
- [ ] Verify empty and non-empty storage units through native IPC and the live GC.
- [ ] Establish the wire meaning of SO unknown fields 4/5, or remove unsupported assumptions.
- [ ] Verify moves/removals during storage loading and notification ordering.
- [ ] Review and commit the pending storage-contents slice independently.
- [ ] Golden characterization fixtures for conversion, grouping, filters, sorts, and trade-ups.
- [ ] CI for workspace tests, Clippy, and WASM checks.
- [ ] Startup, filtering, bulk-move, memory, listener-count, and installer-size metrics.
- [ ] Re-login/disconnect/reconnect soak tests with stable dispatcher/listener counts.
- [ ] Platform packaging/signing and final migration/parity sign-off before deleting legacy.

## Optional Expansion (Not Legacy Parity)

Legacy's Buff163/Float source choices are locked; additional functioning market
providers are expansion work rather than a missing working legacy feature.

- [ ] Additional market providers and provider selection.
- [ ] Provider-specific rate limiting, caching, and fallback behavior.