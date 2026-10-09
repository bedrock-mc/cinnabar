//! Applies held and worn equipment to actors and records the outcomes.

use super::*;
use crate::item::EquipmentOutcome;

impl ActorStore {
    pub(crate) fn apply_equipment(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: EquipmentEvent,
    ) -> ActorApplyResult {
        let (runtime_id, stack) = (event.actor_runtime_id, event.stack.clone());
        let (result, outcome) = self.apply_equipment_inner(session_id, sequence, event);
        self.items.note(runtime_id, false, outcome, &[&stack]);
        result
    }

    fn apply_equipment_inner(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: EquipmentEvent,
    ) -> (ActorApplyResult, EquipmentOutcome) {
        let guard = self.guard(session_id, sequence);
        if guard != ActorApplyResult::Updated {
            return (guard, EquipmentOutcome::Stale);
        }
        if self.remote_state_excluded_runtime_id == Some(event.actor_runtime_id) {
            return (
                ActorApplyResult::MissingActor,
                EquipmentOutcome::LocalPlayer,
            );
        }
        let Some(lifetime) = self.lifetime(event.actor_runtime_id) else {
            return (
                ActorApplyResult::MissingActor,
                EquipmentOutcome::UnknownActor,
            );
        };
        if self.items.apply_equipment(lifetime, sequence, event) {
            (ActorApplyResult::Updated, EquipmentOutcome::Applied)
        } else {
            (
                ActorApplyResult::CapacityRejected,
                EquipmentOutcome::RejectedStack,
            )
        }
    }

    /// Applies worn armor to a live remote actor, or to the client-owned local runtime even
    /// before its synthetic actor exists.
    pub(crate) fn apply_armor(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: &protocol::ArmorEquipmentEvent,
    ) -> ActorApplyResult {
        let (result, outcome) = self.apply_armor_inner(session_id, sequence, event);
        let stacks = [
            &event.helmet,
            &event.chestplate,
            &event.leggings,
            &event.boots,
            &event.body,
        ];
        self.items
            .note(event.actor_runtime_id, true, outcome, &stacks);
        result
    }

    fn apply_armor_inner(
        &mut self,
        session_id: u64,
        sequence: u64,
        event: &protocol::ArmorEquipmentEvent,
    ) -> (ActorApplyResult, EquipmentOutcome) {
        let guard = self.guard(session_id, sequence);
        if guard != ActorApplyResult::Updated {
            return (guard, EquipmentOutcome::Stale);
        }
        let lifetime = self.lifetime(event.actor_runtime_id).or_else(|| {
            (self.remote_state_excluded_runtime_id == Some(event.actor_runtime_id)).then_some(
                ActorLifetimeId {
                    session_id: self.session_id,
                    dimension: self.dimension,
                    runtime_id: event.actor_runtime_id,
                    spawn_revision: 0,
                },
            )
        });
        let Some(lifetime) = lifetime else {
            return (
                ActorApplyResult::MissingActor,
                EquipmentOutcome::UnknownActor,
            );
        };
        if self.items.apply_armor(lifetime, sequence, event) {
            (ActorApplyResult::Updated, EquipmentOutcome::Applied)
        } else {
            (
                ActorApplyResult::CapacityRejected,
                EquipmentOutcome::RejectedStack,
            )
        }
    }

    /// Drains where equipment events landed since the last call.
    pub(crate) fn take_equipment_notices(&mut self) -> Vec<crate::EquipmentNotice> {
        self.items.take_notices()
    }
}
