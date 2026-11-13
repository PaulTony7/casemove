# Phase 0 Spike Results

## Spike B — Rust Steam login + CS2 GC session: ✅ PASS (2026-11-13)

Run: `spike-gc` (`steam-vent` 0.5.0 + `steam-vent-proto` csgo) against a real
account (same account used for the legacy baseline), console device-code guard.

```
[361]   cm server list discovered
        device code: [24460] logged in as PaulTony7 (steamid [U:1:1167460669])
[304]   GC welcome received
[0]     parsed 585 inventory items from SO cache
[25125] TOTAL 25125 ms   (dominated by interactive guard-code entry)
```

Verdict: **Rust core (no Node sidecar contingency)**. `steam-vent` covers
password/device-code login, CM connection, `GameCoordinator` hello/welcome, and
SO-cache inventory parsing. Remaining to prove in Phase 2 (covered by the
existing legacy behavior as spec): refresh-token login, casket ops
(add/remove/rename + `itemCustomizationNotification` acks), wallet/connection
events.

## Spike C — Tauri 2 + Leptos shell: ✅ PASS (2026-11-13)

`create-tauri-app -t leptos` scaffold → `cargo tauri build --no-bundle`
succeeded: release binary + trunk WASM output (`dist/`, 1.4 MB wasm).
Zero-JS frontend pipeline verified on this machine (WebKitGTK deps present).

## Spike A — crate selection

Primary: `steam-vent` (has `GCHandshake`, GC send/receive, packaged csgo
protobufs). `steam-client-rs` (node-steam-user-style API) kept as fallback;
`steam-auth-rs` (node-steam-session port) available for QR login later —
`steam-vent` has no QR support (legacy `startQRLogin` parity needs it).
