# Command & Event Contract

Single source of truth for the command/event surface between UI and core. Legacy defines
the same channels in three places (`src/main/main.ts`, `src/main/preload.js`,
renderer call sites); the rewrite defines each name exactly once (Rust command
signatures + serde types). This document is the strangler seam and the Phase 5
parity checklist.

Conventions:

- **Commands** are request/response: async methods on `app_core::Core` that the
  egui UI awaits on its runtime. One call, one typed
  reply, timeouts owned by the core (legacy races `*-reply` events against
  hand-rolled `setTimeout` in `preload.js`). The names below stay the contract
  identifiers even though there is no IPC transport anymore.
- **Events** are core→UI pushes: `app_core::AppEvent` values delivered on the
  event channel; the sink requests an egui repaint when one lands. The legacy `userEvents`
  mega-channel (`[statusCode, description, data]`, codes 1–4) is split into
  typed events.
- Status: `☐` not started · `◐` implemented in Rust · `☑` parity verified.

## Commands

### Session & login

| New command | Legacy channel(s) | Notes | Status |
|---|---|---|---|
| `steam_login(params: PasswordLoginParams) -> SteamProfile` | `login` + `login-reply` | Password + manual Guard code, shared-secret device TOTP, or mobile approval. Guard code and shared secret are mutually exclusive. Remember saves only a refresh token to keyring; optional notice reports save failure. Live parity pending | ◐ |
| `steam_login_saved(account) -> SteamProfile` | Saved-account login | Account name only in IPC; reads refresh token from native keyring inside cancellable login lifecycle. Live verification pending | ◐ |
| `steam_session() -> Option<SteamProfile>` | Renderer auth state | Account and SteamID only; restores UI state after a frontend reload. Connection-loss events pending | ◐ |
| `steam_start_qr_login(remember) -> SteamProfile` | `startQRLogin` + `login-reply` | QR challenge URL pushed as `login:qr_challenge`; sign-out cancels a pending login. Live verification pending | ◐ |
| `steam_login_webtoken(params: BrowserTokenLoginParams) -> SteamProfile` | Browser-token login | Accepts browser login JSON transiently; validates `logged_in`, account, token, and SteamID. Uses `steam-client-rs`; token is not returned or remembered. Library token-zeroization parity and live verification pending | ◐ |
| `steam_cancel_qr_login()` | `qrLogin:cancel` (main) / `cancelQRLogin` (preload) | Not ported as a separate command; `steam_sign_out()` cancels the shared pending-login lifecycle | ☑ |
| `steam_force_login()` | `forceLogin` | Retry GC attach after `playingElsewhere`/timeout race | ☐ |
| `steam_sign_out()` | `signOut` | Drops the retained connection or cancels pending login. Offline cancellation/relogin tests pass; live S6 gate pending | ◐ |

All login replies are `SteamProfile { account, steam_id, notice?, display_name?,
avatar_url?, wallet? }`. The three optional fields are filled by post-login
enrichment: `wallet` from the CM's `CMsgClientWalletInfoUpdate` (the
subscription is registered first, then steam-vent's unprocessed buffer is
drained for the post-logon push that arrived early; the first message is
waited on for ≤3 s, later ones forward as `wallet:changed`), identity from the
steamcommunity `?xml=1` profile with an HTML profile-page fallback (the XML
endpoint omits identity on profiles Steam considers unset; allowlisted avatar
hosts only). Enrichment is best-effort, never fails a login, and logs one
`casemove enrich:` line per login for diagnosis; the snapshot is retained for
`steam_session` after a frontend reload.
| `steam_retry_connection()` | `retryConnection` | `gamesPlayed([])` then `gamesPlayed([730])` | ☐ |
| `check_steam() -> bool` / `close_steam() -> bool` | `check-steam` / `close-steam` (the only legacy `ipcMain.handle`) | Kill local Steam client (Windows only in legacy) | ☐ |

### Inventory, storage units & items

| New command | Legacy channel(s) | Notes | Status |
|---|---|---|---|
| `refresh_inventory() -> InventorySnapshot` | `refreshInventory` | Read-only GC welcome snapshot with string item IDs; retained client, 30s timeout, session-generation isolation. Retained GC client also streams SO create/update/remove deltas to the UI; live-account verification pending | ◐ |
| `storage_contents(casket_id) -> Vec<Item>` | `getCasketContents` + `getCasketContent-reply` | Validates the ID against the loaded inventory, derives the expected count from attribute 270, and returns only notification-confirmed complete contents; errors return `Err` instead of `[0]` | ◐ |
| `storage_add_item(casket_id, item_id, fast) -> MoveAck` | `moveToStorageUnit` + `-reply` | Acks on `itemCustomizationNotification(CasketAdded)` unless `fast`; 10s timeout lives in core | ◐ |
| `storage_remove_item(casket_id, item_id, fast) -> MoveAck` | `removeFromStorageUnit` + `-reply` | Same pattern, `CasketRemoved` | ◐ |
| `storage_rename(item_id, new_name) -> MoveAck` | `renameStorageUnit` + `-reply` | `nameItem` (non-protobuf raw body) + `NameItem` notification | ◐ |
| `items_set_positions(dict)` | `setItemsPositions` | Raw `SetItemPositions` GC send, fire-and-forget | ☐ |
| `items_equip(item_id, class)` | `setItemEquipped` | `AdjustItemEquippedState`; exposed in legacy preload but unused by UI — confirm before porting | ☐ |
| `export_csv(data)` | `download` | Writes `~/Downloads/casemove.csv` with the legacy `downloadReport` header and JSON-quoted fields, formatted in the selected currency, then reveals it | ◐ |

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
| `currency_rate() -> { currency, rate }` | `getCurrency` + `-reply` | Display currency + USD→currency rate resolved in-process from the bundled `backup/currency.json` snapshot (`pricing::rate`); no live rate fetch | ◐ |
| `settings_get(key) -> Value` | `electron-store-get` + `-reply+randomKey` | Legacy appends a random key to the reply channel to dodge `once` collisions — `invoke` makes this unnecessary | ☐ |
| `settings_set(key, value)` | `electron-store-set` | | ☐ |
| `store_get_accounts() -> Vec<SavedAccount>` | `electron-store-getAccountDetails` + `-reply` | Ordered username/display-name/Steam-avatar metadata only; no secrets returned | ◐ |
| `store_delete_account(username) -> Vec<SavedAccount>` | `electron-store-deleteAccountDetails` | Removes keyring token and canonical account metadata; rollback restores the previous token if metadata deletion fails | ◐ |
| `store_set_account_position(username, index) -> Vec<SavedAccount>` | `electron-store-setAccountPosition` | Persists the canonical account order used by the saved-account sidebar | ◐ |
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
| `inventory:changed` | `userEvents` code 1 | `{ reason: itemRemoved\|itemChanged\|itemAcquired, item, itemId }`; updates the UI snapshot from retained-client SO create/update/remove messages. Live-account verification pending | ◐ |
| `steam:connection` | `userEvents` code 2 | `{ state: disconnected\|reconnected\|fatalError }` | ☐ |
| `gc:connection` | `userEvents` code 3 | `{ state: connected\|disconnected, reason? }` | ☐ |
| `wallet:changed` | `userEvents` code 4 | `WalletInfo { hasWallet, balance, currency }` — balance in whole currency units (raw cents ÷ 100), currency mapped from Steam's numeric codes like legacy | ☑ |
| `pricing:results` | `pricing` | Price map keyed `item_name+wear` | ☐ |
| `updater:status` | `updater` | `{ message }` | ☐ |
| `login:qr_challenge` | `qrLogin:show` | Steam QR challenge URL string; rendered locally into an SVG QR image | ◐ |

## Invariants (hard gates)

1. Every command/event name exists in exactly one place (this doc + the Rust
   signatures that implement it).
2. Command handlers are registered once at startup; session state is held
   behind shared state, not per-login closures. S6 gate: listener/event
   handler count is identical after any number of login/logout cycles.
3. Every row above must reach ☑ before the corresponding `legacy/` code is
   deleted (Phase 5).
