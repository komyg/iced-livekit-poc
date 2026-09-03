//! Types shared across the meeting room's panels.

use std::collections::HashMap;
use std::fmt;

/// Stands in for a participant identity on a message addressed to the room.
/// No real identity can collide with it, so it doubles as the "everyone"
/// entry in the recipient picker.
pub const EVERYONE_ID: &str = "__everyone__";

/// Who else is in the call: participant identity → display label.
///
/// Identity is the stable key `LiveKit` routes on; the label is what a person
/// reads, and falls back to the identity when a participant has no name. Only
/// ever holds remotes — adding ourselves would put us in our own chat
/// recipient list.
pub type Roster = HashMap<String, String>;

/// Who a chat message is addressed to: one participant, or the whole room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recipient {
    pub id: String,
    pub label: String,
}

impl Recipient {
    pub fn everyone() -> Self {
        Self {
            id: EVERYONE_ID.to_owned(),
            label: "Send to Everyone".to_owned(),
        }
    }

    pub fn is_everyone(&self) -> bool {
        self.id == EVERYONE_ID
    }
}

impl fmt::Display for Recipient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
}
