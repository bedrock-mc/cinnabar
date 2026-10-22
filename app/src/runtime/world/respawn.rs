//! Searching withholds input; each ready control completes its own handshake.

use client_world::CommittedControlEvent;
use protocol::Packet;

use client_interaction::movement::{LocalPhysicsController, MovementTicker};
use client_session::PacketSendError;

#[derive(Debug, Default)]
pub(crate) struct RespawnLifecycle {
    session: Option<u64>,
    last_sequence: Option<u64>,
    pending_control_sequence: Option<u64>,
    searching: bool,
    completion_actor: Option<u64>,
}

impl RespawnLifecycle {
    pub(crate) fn synchronize_session(&mut self, session: Option<u64>) {
        if self.session != session {
            *self = Self {
                session,
                ..Self::default()
            };
        }
    }

    /// Input remains withheld until the native completion precedes it in the queue.
    pub(crate) fn input_held(&self) -> bool {
        self.searching
            || self.completion_actor.is_some()
            || self
                .pending_control_sequence
                .is_some_and(|pending| self.last_sequence.is_none_or(|consumed| consumed < pending))
    }

    /// Withholds fresh gameplay while committed death or respawn phases await older controls.
    pub(super) fn retain_committed_input_hold(&mut self, controls: &[CommittedControlEvent]) {
        for control in controls {
            let sequence = match control {
                CommittedControlEvent::LocalDeath { sequence } => sequence,
                CommittedControlEvent::Respawn {
                    sequence, respawn, ..
                } if respawn.searching_for_spawn() || respawn.ready_to_spawn() => sequence,
                _ => continue,
            };
            self.pending_control_sequence = Some(
                self.pending_control_sequence
                    .map_or(*sequence, |pending| pending.max(*sequence)),
            );
        }
    }

    /// Applies ordered death and respawn readiness before spatial reconciliation.
    pub(super) fn consume_nonspatial_phase(
        &mut self,
        session: u64,
        control: &CommittedControlEvent,
        local_actor: u64,
        movement: &mut MovementTicker,
        physics: &mut LocalPhysicsController,
        item_use: Option<&mut client_interaction::item_use::ItemUseRuntime>,
    ) -> bool {
        let (sequence, respawn) = match control {
            CommittedControlEvent::LocalDeath { sequence } => (sequence, None),
            CommittedControlEvent::Respawn {
                sequence, respawn, ..
            } => (sequence, Some(respawn)),
            _ => return false,
        };
        self.synchronize_session(Some(session));
        if self.last_sequence.is_some_and(|last| *sequence <= last) {
            return true;
        }
        self.last_sequence = Some(*sequence);
        if respawn.is_none_or(|respawn| respawn.searching_for_spawn()) {
            self.begin_search(movement, physics, item_use);
            if let Some(respawn) = respawn {
                super::control_apply::log_respawn(*respawn);
            }
            return true;
        }
        if !respawn.is_some_and(|respawn| respawn.ready_to_spawn()) {
            return true;
        }
        if let Some(item_use) = item_use {
            item_use.cancel_for_respawn();
        }
        self.searching = false;
        self.completion_actor = Some(local_actor);
        false
    }

    /// Starts the death input hold once, discarding actions from the previous life.
    fn begin_search(
        &mut self,
        movement: &mut MovementTicker,
        physics: &mut LocalPhysicsController,
        item_use: Option<&mut client_interaction::item_use::ItemUseRuntime>,
    ) {
        if !self.searching {
            movement.begin_respawn_search(physics);
            if let Some(item_use) = item_use {
                item_use.cancel_for_respawn();
            }
        }
        self.searching = true;
        self.completion_actor = None;
    }

    pub(crate) fn queue_completion(
        &mut self,
        mut send: impl FnMut(Packet) -> Result<(), PacketSendError>,
    ) -> Result<(), PacketSendError> {
        if let Some(actor) = self.completion_actor {
            send(protocol::respawn_ready_packet(actor))?;
            self.completion_actor = None;
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "respawn_tests.rs"]
mod tests;
