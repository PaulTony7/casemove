# Feature Parity Checklist

Snapshot: 2026-11-14. Compared against the legacy application and the current
Rust working tree. Only outstanding work is listed — completed work lives in
git history.

- Sections group related work; their order is **not a priority ranking**.
- Keep legacy workflows until replacement behavior and characterization tests
  are verified. Use [COMMANDS.md](COMMANDS.md) for IPC contracts and parity status,
  and [REWRITE_PLAN.md](REWRITE_PLAN.md) for migration phases.

## Application And Build

- Windows executable and installer packaging for the eframe app (the Tauri
  MSI/NSIS path was removed in the native-UI pivot).
- Custom Windows titlebar and minimize/maximize/close controls.
- Update checks, update status, and restart/download workflow.
- Verify macOS Intel and Apple Silicon packaging.
- Verify Linux packaging and native rendering behavior.

## Login And Accounts

- Steam-client-open prompt and close/continue handling (the preference persists
  and appears in Settings; the popup itself is missing).
- Automatic Steam/GC reconnection and accurate connection-loss status.

## Inventory And Item Data

- Verify live item/sticker image coverage and cached/bundled fallback.
- Complete agent-specific and remaining specialized-item enrichment.

The app loads the bundled schema immediately, uses a cached live schema when
available, and refreshes it at most once per 24 hours. Live VDF schema and
translation data come from Skinledger; image catalogs for skins, stickers,
agents, collectibles, graffiti, music kits, crates, and keys come from the
public CSGO-API dataset. Failed refreshes keep the last cached or bundled
schema.

**Recommended next feature:** Item/sticker image coverage and agent-specific
enrichment, then trade-ups.

## Operations

- Item-position operations; confirm user-facing legacy usage before porting.
- Equip operations; confirm user-facing legacy usage before porting.

## Preferences

- Developer-mode controls.

## Trade-Ups And Containers

- Verify a live trade-up: the submitted contract, the GC ack, and the
  resulting item appearing through inventory changes.
- Verify the picker against a real inventory (collections from the bundled
  `collections.json`, wear/StatTrak locks, storage-unit inputs).
- Container-opening operation; confirm intended legacy UI scope before porting
  (`open_container` payload is ported, no UI yet).

## Verification And Release Gates

- Confirm the ClientHello correction resolves the reported live inventory timeout.
- Verify OS-keyring save/read/delete and remembered-login reuse after restart with a real account.
- Verify shared-secret login, email-only challenges, clock skew, and code rollover against a real account.
- Compare real-account inventory IDs/counts and visible metadata against legacy.
- Verify empty and non-empty storage units through native IPC and the live GC.
- Verify live schema refresh, cached/bundled fallback, and item/sticker image rendering.
- Establish the wire meaning of SO unknown fields 4/5, or remove unsupported assumptions.
- Verify moves/removals during storage loading and notification ordering.
- Golden characterization fixtures for conversion, grouping, filters, and sorts.
- Startup, filtering, bulk-move, memory, and listener-count metrics.
- Re-login/disconnect/reconnect soak tests with stable dispatcher/listener counts.
- Platform packaging/signing and final migration/parity sign-off before deleting legacy.

## Optional Expansion (Not Legacy Parity)

Legacy's Buff163/Float source choices are locked; additional functioning market
providers are expansion work rather than a missing working legacy feature.

- Additional market providers and provider selection.
- Provider-specific rate limiting, caching, and fallback behavior.
