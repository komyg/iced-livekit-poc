//! Types shared across the meeting room's panels.

use std::fmt;

use livekit::Room;
use rust_i18n::t;

/// Stands in for a participant identity on a message addressed to the room.
/// No real identity can collide with it, so it doubles as the "everyone"
/// entry in the recipient picker.
pub const EVERYONE_ID: &str = "__everyone__";

/// One person in the call, as the UI sees them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// The stable key `LiveKit` routes on.
    pub identity: String,
    pub label: String,
}

impl Member {
    pub fn new(name: String, identity: String) -> Self {
        Self {
            label: participant_label(name, identity.clone()),
            identity,
        }
    }
}

/// What to show for a participant: their name, or their identity when they
/// joined without one.
pub fn participant_label(name: String, identity: String) -> String {
    if name.is_empty() { identity } else { name }
}

/// Everyone in the call, including ourselves.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Roster {
    local: Option<Member>,
    remotes: Vec<Member>,
}

/// Ourselves first, then the remotes.
pub type RosterIter<'a> =
    std::iter::Chain<std::option::Iter<'a, Member>, std::slice::Iter<'a, Member>>;

impl Roster {
    pub const fn new() -> Self {
        Self {
            local: None,
            remotes: Vec::new(),
        }
    }

    /// Snapshots `LiveKit`'s participant handles into plain data the UI can
    /// hold.
    pub fn snapshot(room: &Room) -> Self {
        let mut roster: Self = room
            .remote_participants()
            .into_values()
            .map(|participant| Member::new(participant.name(), participant.identity().0))
            .collect();

        let local = room.local_participant();
        roster.set_local(Member::new(local.name(), local.identity().0));

        roster
    }

    pub fn set_local(&mut self, member: Member) {
        // In case a snapshot listed us among the remotes.
        self.remotes
            .retain(|remote| remote.identity != member.identity);
        self.local = Some(member);
    }

    pub const fn local(&self) -> Option<&Member> {
        self.local.as_ref()
    }

    pub fn is_local(&self, identity: &str) -> bool {
        self.local
            .as_ref()
            .is_some_and(|local| local.identity == identity)
    }

    /// Adds a remote, or renames whoever already has this identity.
    pub fn upsert(&mut self, member: Member) {
        if self.is_local(&member.identity) {
            self.local = Some(member);
            return;
        }

        match self
            .remotes
            .iter_mut()
            .find(|existing| existing.identity == member.identity)
        {
            Some(existing) => *existing = member,
            None => self.remotes.push(member),
        }
    }

    /// Remotes only: we never leave our own call.
    pub fn remove(&mut self, identity: &str) {
        self.remotes.retain(|member| member.identity != identity);
    }

    pub fn contains(&self, identity: &str) -> bool {
        self.is_local(identity)
            || self
                .remotes
                .iter()
                .any(|member| member.identity == identity)
    }

    pub fn remotes(&self) -> std::slice::Iter<'_, Member> {
        self.remotes.iter()
    }

    pub fn iter(&self) -> RosterIter<'_> {
        self.local.iter().chain(self.remotes.iter())
    }
}

impl<'a> IntoIterator for &'a Roster {
    type Item = &'a Member;
    type IntoIter = RosterIter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Collects remotes; set ourselves with [`Roster::set_local`] afterwards.
impl FromIterator<Member> for Roster {
    fn from_iter<I: IntoIterator<Item = Member>>(members: I) -> Self {
        let mut roster = Self::new();
        for member in members {
            roster.upsert(member);
        }
        roster
    }
}

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
            label: t!("chat.everyone").into_owned(),
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
