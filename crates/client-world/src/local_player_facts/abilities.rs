//! Passive session-bound evidence, never an effective permission resolver.
use protocol::AbilitiesUpdate;

use super::LocalPlayerFacts;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Binding {
    session: u64,
    stream: u64,
    actor_unique_id: i64,
}

#[derive(Clone, Debug, Default)]
pub(super) struct LocalAbilities {
    binding: Option<Binding>,
    sequence: Option<u64>,
    update: Option<AbilitiesUpdate>,
}

impl LocalPlayerFacts {
    /// Retires evidence and its admission binding. A drain cannot re-arm it.
    pub fn clear_local_abilities(&mut self) {
        self.local_abilities = LocalAbilities::default();
    }

    /// Accepted fatal-free bootstrap tail only; no received evidence is invented.
    pub fn bind_local_abilities(
        &mut self,
        session: u64,
        stream: u64,
        actor_unique_id: i64,
        setup_succeeded: bool,
    ) {
        if self.session_id() == session && setup_succeeded {
            self.local_abilities = LocalAbilities {
                binding: Some(Binding {
                    session,
                    stream,
                    actor_unique_id,
                }),
                ..Default::default()
            };
        }
    }

    /// Missing, failed or replaced streams retire evidence, without minting a new binding.
    pub fn synchronize_local_abilities(&mut self, session: u64, stream: Option<u64>) {
        if self
            .local_abilities
            .binding
            .is_some_and(|binding| binding.session != session || Some(binding.stream) != stream)
        {
            self.clear_local_abilities();
        }
    }

    /// Retains only newer evidence matching the accepted session, stream and actor.
    pub fn apply_local_abilities(
        &mut self,
        session: u64,
        stream: u64,
        sequence: u64,
        update: AbilitiesUpdate,
    ) {
        let Some(binding) = self.local_abilities.binding else {
            return;
        };
        if self.session_id() != session
            || binding.session != session
            || binding.stream != stream
            || binding.actor_unique_id != update.actor_unique_id
            || self
                .local_abilities
                .sequence
                .is_some_and(|old| sequence <= old)
        {
            return;
        }
        self.local_abilities.sequence = Some(sequence);
        self.local_abilities.update = Some(update);
    }

    /// None means unknown. Received-empty and unavailable remain distinct evidence.
    pub fn local_abilities(&self) -> Option<&AbilitiesUpdate> {
        self.local_abilities.update.as_ref()
    }
}
