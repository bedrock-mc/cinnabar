//! Read-only retained server damage evidence; attacker identity is never inferred here.
use super::WorldAuthority;

/// Resource ceiling for replayable accepted actor status notices.
const MAX_COMBAT_EVIDENCE: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorCombatKind {
    Hurt,
    Death,
}

/// A server actor status accepted by this session's authoritative store.
#[derive(Clone, Copy, Debug)]
pub struct ActorCombatEvidence {
    pub revision: u64,
    pub ingress_sequence: u64,
    pub dimension: i32,
    pub runtime_id: u64,
    pub player: bool,
    pub position: [f32; 3],
    pub kind: ActorCombatKind,
}

impl WorldAuthority {
    /// Borrows bounded accepted status history without consuming particle/audio notice queues.
    pub fn combat_evidence(&self) -> impl Iterator<Item = &ActorCombatEvidence> {
        self.combat_evidence.iter()
    }

    /// Records only accepted status updates with finite retained actor pose.
    pub(super) fn record_combat_evidence(
        &mut self,
        ingress_sequence: u64,
        runtime_id: u64,
        kind: ActorCombatKind,
    ) {
        let Some(actor) = self.actors.get(runtime_id) else {
            return;
        };
        if !actor.position.iter().all(|v| v.is_finite()) {
            return;
        }
        let Some(revision) = self.combat_revision.checked_add(1) else {
            return;
        };
        self.combat_revision = revision;
        if self.combat_evidence.len() == MAX_COMBAT_EVIDENCE {
            self.combat_evidence.pop_front();
        }
        self.combat_evidence.push_back(ActorCombatEvidence {
            revision,
            ingress_sequence,
            dimension: self.current_dimension,
            runtime_id,
            player: matches!(actor.kind, protocol::ActorKind::Player { .. }),
            position: actor.position,
            kind,
        });
    }
}
