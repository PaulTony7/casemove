# Feature Parity Checklist

Snapshot: 2026-11-14. Compared against the legacy application and the current
Rust working tree. Only outstanding work is listed — completed work lives in
git history.

- Sections group related work; their order is **not a priority ranking**.
- Keep legacy workflows until replacement behavior and characterization tests
  are verified. Use [COMMANDS.md](COMMANDS.md) for the command/event surface and
  [REWRITE_PLAN.md](REWRITE_PLAN.md) for migration phases.

## Application And Build

- Windows executable and installer packaging for the eframe app.
- Custom Windows titlebar and minimize/maximize/close controls.
- Self-update download/install (the status strip compares the build with
  the latest GitHub release and opens the release page; no in-app installer).
- Verify macOS Intel and Apple Silicon packaging.
- Verify Linux packaging and native rendering behavior.

## Login And Accounts

- Verify the Steam-close prompt on Windows.
- Verify the connection monitor against a real session (CM logoff, automatic
  retry, and the Retry connection button).

## Inventory And Item Data

- Verify live item/sticker/image coverage and cached/bundled fallback. A few
  CSGO-API entries point at `raw.githubusercontent.com`; they are skipped on
  purpose because images load only from allowlisted Steam CDN hosts.

## Trade-Ups And Containers

- Verify a live trade-up: the submitted contract, the GC ack, and the
  resulting item appearing through inventory changes.
- Verify the picker against a real inventory (wear/StatTrak locks and
  storage-unit inputs).
- Verify container opening (Open action with key selection, validation, and the
  unlocked item arriving through inventory changes).

## Verification And Release Gates

- Confirm the ClientHello correction resolves the reported live inventory timeout.
- Verify OS-keyring save/read/delete and remembered-login reuse after restart with a real account.
- Verify shared-secret login, email-only challenges, clock skew, and code rollover against a real account.
- Compare real-account inventory IDs/counts and visible metadata against legacy.
- Verify empty and non-empty storage units through the live GC.
- Establish the wire meaning of SO unknown fields 4/5, or remove unsupported assumptions.
- Verify moves/removals during storage loading and notification ordering.
- Decide whether storage units differing only in contained count should merge
  in the inventory table (the grouping key, unlike legacy's `combineInventory`,
  omits the count and item id, so they share a row today).
- Startup, filtering, bulk-move, memory, and listener-count metrics.
- Re-login/disconnect/reconnect soak tests with stable dispatcher/listener counts.
- Platform packaging/signing and final migration/parity sign-off before deleting legacy.

## Optional Expansion (Not Legacy Parity)

Additional functioning market providers are expansion work rather than a
missing legacy feature (legacy's Buff163/Float sources select `url = ''`).

- Additional market providers and provider selection.
- Provider-specific rate limiting, caching, and fallback behavior.
