# Missing Features And Platform Differences

Snapshot: 2026-11-14, written when `legacy/` (the Electron app) was removed from
the repository, so this file replaces it as the remaining-work record. It lists
what is **not** done, not what is done — implementation history is in git.

Status legend: **Not built** (no code) · **Unverified** (code exists, never run
against a real account or platform) · **Decision needed** (behavior differs
from the old app on purpose or by accident).

## Not Built

- **Windows executable and installer.** The eframe app has no packager yet;
  `cargo run -p casemove-ui` is the only way to start it.
- **Custom Windows titlebar** (minimize/maximize/close drawn by the app). The
  window uses native decorations.
- **In-app self-update.** The status strip compares `CARGO_PKG_VERSION` with the
  latest GitHub release and opens the release page; it never downloads or
  installs. Live update checks also need a published `v*` release, which does
  not exist yet, so the banner cannot appear today.
- **macOS and Linux packaging**, both Intel/Apple Silicon and generic Linux
  (AppImage/deb), including signing and notarization.
- **Buff163 and Float prices.** Buff163 answers every data endpoint with
  `"请先登录"` (login required) and redirects its market pages to login, so
  there is no anonymous source to read; Float is likewise account-gated. Only
  the bundled snapshot's `buff163` column remains. Adding either would need a
  user-supplied session cookie that breaks whenever the site changes.
- **Item positions and equip operations.** The old app exposed them but never
  called them; neither is ported.

## Unverified (needs a real Steam account or platform)

Run these with [VERIFICATION.md](VERIFICATION.md), which turns each item below
into tickable steps (safe vs risky, per platform).

- **Steam-close prompt on Windows.** Prompt, "Close and login", "Login without
  closing", and the "Dont show this again" latch are ported and tested offline,
  but process detection and terminating Steam have not run on Windows.
- **Connection monitor.** CM logoff detection, the 5/15/30/60 s reconnect
  backoff, and the Retry connection button have not seen a real disconnect.
- **Live inventory reachability.** The ClientHello correction is not confirmed
  against the reported live inventory timeout.
- **Item and sticker images.** Live catalog coverage and the cached/bundled
  fallback are exercised only by offline tests.
- **Keyring.** Save/read/delete of remembered logins, and reuse after restart,
  is untested against the OS keyring on all three platforms.
- **Login edge cases.** Shared-secret TOTP, email-only challenges, clock skew,
  and code rollover at the 30 s boundary.
- **Storage units.** Empty and non-empty units through the live GC, moves or
  removals that land while contents are still loading, and notification
  ordering.
- **Trade-ups.** A submitted contract, the GC ack, and the resulting item
  arriving through inventory changes; picker locks (wear, StatTrak) against a
  real inventory.
- **Container opening.** Key selection and the unlocked item arriving through
  inventory changes.
- **Real-account inventory parity.** Item ids, counts, and visible metadata
  have not been compared against the old app's output (it is no longer in the
  repository; compare against a saved export or Steam itself).
- **Performance and soak.** Startup, filtering, bulk-move, and memory numbers,
  plus re-login/disconnect soak runs with stable listener counts.

## Decision Needed

- **SO unknown fields 4/5.** The inventory decoder reads the added/removed item
  lists from preserved unknown protobuf fields because the bundled proto omits
  them (`crates/gc-cs2/src/lib.rs`, `parse_inventory`). If the numbering is
  wrong, incremental updates silently drop items. Either confirm the field
  numbers from a captured `CMsgSOCacheSubscribed` and pin them with a test, or
  replace incremental decoding with a full refresh on each change notification.
- **Storage-unit grouping.** Units that differ only in contained count merge
  into one inventory row; the old `combineInventory` kept them separate. Change
  the grouping key or accept the merge.
- **Image host allowlist.** Images load only from Steam CDN hosts. A few
  CSGO-API catalog entries point at `raw.githubusercontent.com` and are skipped;
  widening the allowlist is a one-line change but loosens the policy that also
  protects avatar loading.
- **Search scope.** Search matches name, wear, custom name (as the old app did)
  plus collection and sticker names (an addition). Confirm the extension is
  wanted.
- **Unreachable storage sort.** `sort_rows("storage")` has no UI control;
  either add the column or delete the branch.

## Platform Notes

- Remembered logins depend on the OS keyring; a headless or locked keyring makes
  saved logins fail with a visible error rather than crashing.
- Steam process detection uses `tasklist` (Windows), `/proc` (Linux), and
  `pgrep` (macOS); the old app only detected Steam on Windows and reported "not
  running" elsewhere.
- The CSV export writes `~/Downloads/casemove.csv`, the same path and columns as
  the old `downloadReport`.
