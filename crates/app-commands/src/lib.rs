//! Typed command and event contract between UI and core.
//! See docs/COMMANDS.md — the single source of truth for channel names.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedAccount {
    pub username: String,
    pub display_name: String,
}
