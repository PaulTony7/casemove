# Command & Event Contract

Single source of truth for the IPC surface between UI and core. Legacy defines
the same channels in three places (`src/main/main.ts`, `src/main/preload.js`,
renderer call sites); the rewrite defines each name exactly once (Rust command
signatures + serde types). This document is the strangler seam and the Phase 5
parity checklist.

Conventions:

- **Commands** are request/response (`invoke` in Tauri). One call, one typed
  reply, timeouts owned by the core (legacy races `*-reply` events against
  hand-rolled `setTimeout` in `preload.js`).
- **Events** are core→UI pushes (`emit` in Tauri). The legacy `userEvents`
  mega-channel (`[statusCode, description, data]`, codes 1–4) is split into
  typed events.
- Status: `☐` not started · `◐` implemented in Rust · `☑` parity verified.

## Commands

### Session & login

| New command | Legacy channel(s) | Notes | Status |
|---|---|---|---|
| `steam_login(params: LoginParams) -> LoginResult` | `login` + `login-reply` | `LoginParams { account, password?, remember, guard_code?, shared_secret?, clientjstoken?, refresh_token? }` covers all 5 legacy strategies | ☐ |
| `steam_start_qr_login(remember) -> LoginResult` | `startQRLogin` + `login-reply` | QR challenge pushed as `login:qr_challenge` event | ☐ |
| `steam_cancel_qr_login()` | `qrLogin:cancel` (main) / `cancelQRLogin` (preload) | **Legacy bug**: preload sends `cancelQRLogin`, main listens `qrLogin:cancel` — never fires. Fixed by one name | ☐ |
| `steam_force_login()` | `forceLogin` | Retry GC attach after `playingElsewhere`/timeout race | ☐ |
| `steam_sign_out()` | `signOut` | Tears down session; must leave listener counts unchanged (S6 gate) | ☐ |
| `steam_retry_connection()` | `retryConnection` | `gamesPlayed([])` then `gamesPlayed([730])` | ☐ |
| `check_steam() -> bool` / `close_steam() -> bool` | `check-steam` / `close-steam` (the only legacy `ipcMain.handle`) | Kill local Steam client (Windows only in legacy) | ☐ |

### Inventory, storage units & items

| New command | Legacy channel(s) | Notes | Status |
|---|---|---|---|
| `refresh_inventory()` | `refreshInventory` | Legacy re-attaches GC inventory listeners; new core does this without listener churn | ☐ |
| `storage_contents(casket_id) -> Vec<Item>` | `getCasketContents` + `getCasketContent-reply` | Converts + trade-up enriches; error path returns `Err` instead of `[0]` | ☐ |
| `storage_add_item(casket_id, item_id, fast) -> MoveAck` | `moveToStorageUnit` + `-reply` | Acks on `itemCustomizationNotification(CasketAdded)` unless `fast`; 10s timeout lives in core | ☐ |
| `storage_remove_item(casket_id, item_id, fast) -> MoveAck` | `removeFromStorageUnit` + `-reply` | Same pattern, `CasketRemoved` | ☐ |
| `storage_rename(item_id, new_name) -> MoveAck` | `renameStorageUnit` + `-reply` | `nameItem` + `NameItem` notification | ☐ |
| `items_set_positions(dict)` | `setItemsPositions` | Raw `SetItemPositions` GC send, fire-and-forget | ☐ |
| `items_equip(item_id, class)` | `setItemEquipped` | `AdjustItemEquippedState`; exposed in legacy preload but unused by UI — confirm before porting | ☐ |
| `export_csv(data)` | `download` | Legacy writes `~/Downloads/casemove.csv` from inside `createWindow` (re-registered per window) | ☐ |

### Trade-ups & containers

| New command | Legacy channel(s) | Notes | Status |
|---|---|---|---|
| `tradeup_possible_outcomes(items) -> Outcomes` | `getTradeUpPossible` + `-reply` | Prices enriched via `pricing:results` event, as legacy does | ☐ |
| `tradeup_process(ids, rarity)` | `processTradeOrder` | Hand-ported `ByteBuffer` craft payload (`Language.Craft`) | ☐ |
| `open_container(ids)` | `openContainer` | `Language.UnlockCrate` payload | ☐ |

### Pricing, currency & settings

| New command | Legacy channel(s) | Notes | Status |
|---|---|---|---|
| `pricing_request(items)` | `getPrice` | Results arrive via `pricing:results` event (same flow as legacy `pricingEmitter`) | ☐ |
| `currency_rate() -> { currency, rate }` | `getCurrency` + `-reply` | | ☐ |
| `settings_get(key) -> Value` | `electron-store-get` + `-reply+randomKey` | Legacy appends a random key to the reply channel to dodge `once` collisions — `invoke` makes this unnecessary | ☐ |
| `settings_set(key, value)` | `electron-store-set` | | ☐ |
| `store_get_accounts() -> Vec<SavedAccount>` | `electron-store-getAccountDetails` + `-reply` | Ordered username/display-name metadata only; no secrets returned. Avatar/position parity pending | ◐ |
| `store_delete_account(username) -> Vec<SavedAccount>` | `electron-store-deleteAccountDetails` | Removes profile metadata and returns the updated list; keyring cleanup awaits login integration | ◐ |
| `store_set_account_position(account, index)` | `electron-store-setAccountPosition` | | ☐ |
| `check_update() -> UpdateInfo` | `needUpdate` + `needUpdate-reply` | Legacy caches a broken `gitHub` flag and replies `[false,…]` on error | ☐ |

### Window

| New command | Legacy channel(s) | Notes | Status |
|---|---|---|---|
| `window_action(minimize\|maximize\|close)` | `windowsActions` | Only needed if the custom frameless titlebar is kept (legacy: `frame: false` on win32) | ☐ |

Dropped from legacy: `ipc-example`/`myPing` (ERB boilerplate), whitelist
entries never sent (`errorMain`, `checkSteam`, `closeSteam`, `login`,
`refreshInventory` as `on` channels), `forceLoginReply` (redundant with
`invoke`), `userEvents()` promise wrapper (replaced by event listeners).

## Events (core → UI)

| New event | Legacy channel | Payload | Status |
|---|---|---|---|
| `inventory:changed` | `userEvents` code 1 | `{ reason: itemRemoved\|itemChanged\|itemAcquired, item, items }` (legacy tuple `[1, desc, [item, enriched]]`) | ☐ |
| `steam:connection` | `userEvents` code 2 | `{ state: disconnected\|reconnected\|fatalError }` | ☐ |
| `gc:connection` | `userEvents` code 3 | `{ state: connected\|disconnected, reason? }` | ☐ |
| `wallet:changed` | `userEvents` code 4 | `{ has_wallet, currency, balance }` | ☐ |
| `pricing:results` | `pricing` | Price map keyed `item_name+wear` | ☐ |
| `updater:status` | `updater` | `{ message }` | ☐ |
| `login:qr_challenge` | `qrLogin:show` | QR challenge payload | ☐ |

## Invariants (hard gates)

1. Every command/event name exists in exactly one place (this doc + the Rust
   signatures that implement it).
2. Command handlers are registered once at startup; session state is held
   behind shared state, not per-login closures. S6 gate: listener/event
   handler count is identical after any number of login/logout cycles.
3. Every row above must reach ☑ before the corresponding `legacy/` code is
   deleted (Phase 5).
