# Verification Checklist

The items in [MISSING_FEATURES.md](MISSING_FEATURES.md) under **Unverified**
cannot be closed by automated tests — they need a real Steam account, real
money-adjacent actions, or a specific OS. This file turns them into steps you
can run and tick off.

Mark each step **pass**, **fail** (link the symptom), or **blocked** (note why).
When a section is fully passed, delete it from the Unverified list in
`MISSING_FEATURES.md`. Anything that fails becomes a bug, not a checklist item.

`safe` steps touch no inventory, money, or account state and can be run at any
time. `risky` steps move items, submit contracts, or open containers.

---

## 0. Setup (once)

- [ ] Build the version under test and record `git rev-parse HEAD`.
- [ ] Have a Steam account with a real CS2 inventory, including at least one
      storage unit, one tradeable skin, and one non-tradeable item.
- [ ] Note the inventory's item count and total value in Steam itself, to
      compare later.
- [ ] Keep a second account or a second machine available for the
      connection-monitor steps.

---

## 1. Steam-close prompt (Windows)

Needs: Windows, Steam installed and running.

- [ ] **safe** Steam open → type a login → the "close Steam" prompt appears.
- [ ] **safe** "Close and login" terminates Steam and the login proceeds.
- [ ] **safe** "Login without closing" logs in while Steam stays running.
- [ ] **safe** Restart the app and sign in with a **saved** account; the same
      prompt appears (not skipped).
- [ ] **safe** Tick "Dont show this again", then sign in again: no prompt, and
      the choice survives a restart.
- [ ] **safe** Steam running but refusing to terminate (e.g. mid-update):
      the app reports that Steam is still running instead of claiming success.
- [ ] **safe** Steam closed → login proceeds with no prompt.

## 2. Keyring (all three platforms)

- [ ] **safe** Sign in with "Remember for later"; restart the app.
- [ ] **safe** The saved account signs in without retyping the password.
- [ ] **safe** Delete the saved account; confirm nothing is left behind (on
      Windows, check Credential Manager; on Linux, `secret-tool search service casemove`;
      on macOS, Keychain Access).
- [ ] **safe** Lock the keyring (or run headless on Linux) and start the app:
      saved logins fail with a visible error and the app does not crash.
- [ ] **safe** Remove the keyring entry behind the app's back, then try the
      saved account: a clear error, not a hang.

## 3. Login edge cases

- [ ] **safe** Shared-secret TOTP login end to end.
- [ ] **safe** QR login end to end (scan, approve in the Steam app).
- [ ] **safe** Email-only challenge: the code arrives and is accepted.
- [ ] **safe** Request a code, wait past the 30 s window, then submit:
      the failure message is clear and retrying works.
- [ ] **safe** Set the system clock back 10 minutes and log in: report what
      happens (this may legitimately fail; record the message).

## 4. Connection monitor

Needs: a second machine or a second session that can displace the first.

- [ ] **safe** Sign in on machine A, then sign in on machine B with the same
      account: A shows the disconnect state rather than silently freezing.
- [ ] **safe** A's reconnect backoff fires at roughly 5/15/30/60 s (watch the
      status line).
- [ ] **safe** The "Retry connection" button during backoff reconnects at once.
- [ ] **safe** Disconnect the network for one minute, reconnect: the app
      recovers without a restart.
- [ ] **safe** During a 10-minute disconnect/reconnect soak, listener counts
      stay flat (the app logs no growing task/stream count).

## 5. Inventory and images

- [ ] **safe** Sign in and load the inventory: item count matches Steam.
- [ ] **safe** Every row resolves a name, wear, and image (screenshot the gaps).
- [ ] **safe** Clear the image cache directory, restart: images refetch.
- [ ] **safe** Block the image host (hosts file or firewall): rows fall back to
      the placeholder without freezing the UI.
- [ ] **safe** Compare ids, counts, and visible metadata against Steam's own
      inventory page; note every difference.

## 6. Price sources

- [ ] **safe** Switch the source from the inventory bar and from Settings: the
      choice persists across restart and reloads prices from that market.
- [ ] **safe** For 5 items, compare the Steam, Skinport, and CSFloat figures with
      the sites themselves (CSFloat quotes its lowest listing, the others a
      median sale price).
- [ ] **safe** With the network blocked, choosing a market leaves the previous
      prices on screen instead of blanking them.

## 7. Storage units

- [ ] **safe** Load storage units: every unit appears with its contents count.
- [ ] **risky** Move one item into a storage unit from within Steam while the
      app's storage view is open: the app reflects the change without a
      restart.
- [ ] **risky** Remove an item from a unit while its contents are still
      loading: no duplicate row, no lost row.
- [ ] **safe** Empty unit: shows as empty, not as an error.
- [ ] **safe** Storage view pagination and sorting behave with a unit near the
      1,000-item cap.

## 8. Trade-ups (risky)

Real contracts consume items permanently. Use the cheapest valid contract.

- [ ] **risky** Build a 10-item contract, check the wear/StatTrak locks against
      the real inventory.
- [ ] **risky** Submit it, confirm the GC acknowledgement appears.
- [ ] **risky** The resulting item appears through inventory changes without a
      manual refresh.
- [ ] **risky** Submit a contract that the GC rejects: the error is shown and
      the picker is not left locked.

## 9. Container opening (risky)

- [ ] **risky** Open one container with a key: the item arrives through
      inventory changes.
- [ ] **risky** Cancel key selection: nothing is consumed.
- [ ] **risky** Try to open with no keys in the inventory: the app says so
      rather than failing silently.

## 10. Performance and soak

- [ ] **safe** Record startup-to-inventory-ready time and peak memory with a
      real inventory.
- [ ] **safe** Record filter/sort interaction latency with the full inventory.
- [ ] **safe** Record a bulk move of ~50 items and its completion time.
- [ ] **safe** After a 2-hour soak with periodic disconnects, listener counts
      and memory are stable.
