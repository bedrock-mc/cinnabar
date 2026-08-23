use std::collections::VecDeque;

use bevy::prelude::Resource;
use client_world::{CommittedCameraEvent, WorldStream};

/// Maximum retained server camera instructions.
///
/// Camera commands are rare, so this ceiling only guards against a hostile or
/// broken server flooding the queue between render frames.
pub const MAX_SERVER_CAMERA_INSTRUCTIONS: usize = 256;

/// Bounded ordered admission state for server-authored camera instructions.
///
/// This resource preserves packet order and nothing else: no system applies
/// these instructions to the view camera in this tranche. A future renderer
/// consumer reads the retained sequence and owns all application semantics.
///
/// A session-generation change or a dimension change clears the retained
/// instructions whenever that identity mismatch is next observed, including
/// idle drains with no incoming camera traffic; lifetime counters survive so
/// overflow evidence is never lost.
#[derive(Debug, Default, Resource)]
pub struct ServerCameraInstructions {
    entries: VecDeque<CommittedCameraEvent>,
    admitted_total: u64,
    dropped_oldest_total: u64,
    resets: u64,
    identity: Option<(u64, i32)>,
}

impl ServerCameraInstructions {
    /// Rebinds the stored `(session, dimension)` identity, clearing retained
    /// instructions on any mismatch.
    ///
    /// Bounded accepted window: identity is sampled once per drain from the
    /// caller's current world state rather than per packet. Camera events
    /// committed into the same poll batch as a dimension switch may therefore
    /// be admitted under the new identity instead of being cleared as
    /// prior-dimension state; correcting that requires cross-packet
    /// reordering this surface deliberately does not perform, and the window
    /// closes at the next drain after the switch.
    fn refresh_identity(&mut self, session_generation: u64, dimension: i32) {
        if let Some(previous) = self.identity
            && previous != (session_generation, dimension)
        {
            self.entries.clear();
            self.resets = self.resets.saturating_add(1);
        }
        self.identity = Some((session_generation, dimension));
    }

    /// Admits committed camera events under one `(session, dimension)` identity.
    ///
    /// An identity change clears the retained instructions before admitting the
    /// new events; the first admission binds the identity without counting a
    /// reset. Overflow drops the oldest entry instead of rejecting newer,
    /// authoritative instructions.
    pub fn admit(
        &mut self,
        session_generation: u64,
        dimension: i32,
        events: impl IntoIterator<Item = CommittedCameraEvent>,
    ) {
        self.refresh_identity(session_generation, dimension);
        let mut admitted = 0_u64;
        for event in events {
            while self.entries.len() >= MAX_SERVER_CAMERA_INSTRUCTIONS {
                self.entries.pop_front();
                self.dropped_oldest_total = self.dropped_oldest_total.saturating_add(1);
            }
            self.entries.push_back(event);
            admitted = admitted.saturating_add(1);
        }
        self.admitted_total = self.admitted_total.saturating_add(admitted);
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[must_use]
    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, CommittedCameraEvent> {
        self.entries.iter()
    }

    /// Cumulative admitted instruction count across resets.
    #[must_use]
    pub const fn admitted_total(&self) -> u64 {
        self.admitted_total
    }

    /// Cumulative oldest-entry drops from capacity overflow.
    #[must_use]
    pub const fn dropped_oldest_total(&self) -> u64 {
        self.dropped_oldest_total
    }

    /// Cumulative session/dimension identity changes observed at drain time.
    #[must_use]
    pub const fn resets(&self) -> u64 {
        self.resets
    }

    /// Drops every retained instruction without touching lifetime counters.
    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Moves committed camera events from the world stream into the bounded state,
/// refreshing the stored identity on every call so an identity change clears
/// retained instructions even when no camera batch arrived.
pub(crate) fn drain_committed_camera(
    stream: &mut WorldStream,
    session_generation: u64,
    dimension: i32,
    state: &mut ServerCameraInstructions,
) {
    let events = stream.take_committed_camera();
    if events.is_empty() {
        state.refresh_identity(session_generation, dimension);
    } else {
        state.admit(session_generation, dimension, events);
    }
}

#[cfg(test)]
#[path = "server_camera_tests.rs"]
mod server_camera_tests;
