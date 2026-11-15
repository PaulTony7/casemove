//! Steam CM connection state for the UI: a logon monitor that reports
//! disconnects, retries the inventory attach (which is what re-handshakes
//! with the Game Coordinator), and backs off until the CM is back. Legacy
//! surfaced `disconnectedFromGC`/`reconnected` and left retrying to a button.

use crate::inventory::{GcSlot, InventoryClient, prepare_slot};
use crate::{ConnState, Core};
use std::time::Duration;
use steam_core::SteamConnection;
use tokio_stream::StreamExt;

/// Retry delays while the inventory attach keeps failing (legacy's button
/// restarted `gamesPlayed`, which is the same handshake).
const RETRY_DELAYS: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

/// Starts the per-session monitor. It ends when the session generation
/// changes (sign-out or a new login).
pub fn spawn(core: &Core, generation: u64, connection: SteamConnection) {
    let Some(events) = connection.logon_stream() else {
        return;
    };
    let core = core.clone();
    tokio::spawn(async move {
        tokio::pin!(events);
        while let Some(online) = events.next().await {
            if core.session.profile().ok().flatten().map(|profile| profile.generation)
                != Some(generation)
            {
                return;
            }
            if online {
                core.set_conn(ConnState::Connected);
            } else {
                core.set_conn(ConnState::Lost);
                retry_loop(&core, generation).await;
            }
        }
        // The stream ended: the CM connection is gone for good.
        if core.session.profile().ok().flatten().map(|profile| profile.generation)
            == Some(generation)
        {
            core.set_conn(ConnState::Lost);
            retry_loop(&core, generation).await;
        }
    });
}

/// Manual retry (legacy `retryConnection`): one attach attempt on the
/// current session.
pub async fn reconnect(core: &Core, generation: u64) -> Result<(), String> {
    core.set_conn(ConnState::Retrying);
    match attempt_attach(core, generation).await {
        Ok(()) => {
            core.set_conn(ConnState::Connected);
            Ok(())
        }
        Err(error) => {
            core.set_conn(ConnState::RetryFailed(error.clone()));
            Err(error)
        }
    }
}

/// Re-attaches the inventory until it succeeds or the session ends, waiting
/// for the CM to come back between attempts.
async fn retry_loop(core: &Core, generation: u64) {
    for delay in RETRY_DELAYS {
        core.set_conn(ConnState::Retrying);
        tokio::time::sleep(delay).await;
        if !current_session(core, generation) {
            return;
        }
        match attempt_attach(core, generation).await {
            Ok(()) => {
                core.set_conn(ConnState::Connected);
                return;
            }
            Err(error) => core.set_conn(ConnState::RetryFailed(error)),
        }
    }
}

fn current_session(core: &Core, generation: u64) -> bool {
    core.session
        .profile()
        .ok()
        .flatten()
        .is_some_and(|profile| profile.generation == generation)
}

/// The same attach `refresh_inventory` performs on first load, shared so the
/// retry path cannot drift from it.
pub(crate) async fn attempt_attach(core: &Core, expected: u64) -> Result<(), String> {
    let inventory = core.inventory.clone();
    let events = core.events.clone();
    let schema = core.schema.clone();
    let session_handle = core.session.clone();
    let mut state = inventory
        .0
        .try_lock()
        .map_err(|_| "An inventory request is already in progress.".to_owned())?;
    let slot = state.as_mut().ok_or_else(|| {
        "Connect to CS2 and load inventory first.".to_owned()
    })?;
    if slot.generation > expected {
        return Err("The Steam session changed. Load inventory again.".to_owned());
    }
    // A stale slot (older session) is dropped; the same session keeps its
    // slot generation so the attach below re-runs instead of reusing it.
    if slot.generation != expected {
        slot.client = None;
        slot.items.clear();
    }
    slot.generation = expected;
    drop(state);
    session_handle
        .clone()
        .with_connection(move |connection, generation| async move {
            let mut state = inventory
                .0
                .try_lock()
                .map_err(|_| "An inventory request is already in progress.".to_owned())?;
            let _ = prepare_slot(&mut state, generation)?;
            let connected = if let Some(browser_client) = connection.browser_client() {
                InventoryClient::connect_browser(browser_client).await
            } else if let Some(steam_vent) = connection.steam_vent() {
                InventoryClient::connect(steam_vent).await
            } else {
                return Err("The Steam connection is unavailable.".to_owned());
            };
            let (mut client, items) = connected.map_err(|error| error.to_string())?;
            let changes = client.take_changes();
            session_handle
                .in_generation(generation, || {
                    *state = Some(GcSlot {
                        generation,
                        client: Some(client),
                        items: items.clone(),
                    });
                })
                .map_err(|error| error.to_string())?;
            if let Some(changes) = changes {
                crate::inventory::listen_inventory_changes(
                    inventory.clone(),
                    generation,
                    connection.steam_id(),
                    schema.clone(),
                    events.clone(),
                    changes,
                );
            }
            Ok(())
        })
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retry_delays_back_off_and_stay_bounded() {
        assert_eq!(RETRY_DELAYS.len(), 4);
        assert!(RETRY_DELAYS.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(RETRY_DELAYS[3] <= Duration::from_secs(60));
    }
}
