//! Callback-scoped native motion, food, actor and combat evidence, and its validation.
use crate::{CombatObservation, MotionObservation, ObservedActor, VitalsObservation};
use anyhow::{Result, ensure};

/// One day of the server clock, fenced to the currently presented session and dimension.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WorldDay {
    pub session: u64,
    pub dimension: i32,
    pub day: u64,
}

/// Callback-scoped read-only facts captured by native owning adapters.
#[derive(Clone, Default)]
pub struct ModuleObservations {
    pub world_day: Option<WorldDay>,
    /// Confirmed activations in the current native counting interval; absent while tracking is disabled.
    pub local_totem_uses: Option<u32>,
    pub motion: Option<MotionObservation>,
    pub vitals: Option<VitalsObservation>,
    pub ping_ms: Option<u32>,
    pub actors: Vec<ObservedActor>,
    pub combat: Vec<CombatObservation>,
}

/// Checks one finite world vector without inventing missing native evidence.
fn finite(point: (f32, f32, f32)) -> bool {
    [point.0, point.1, point.2].into_iter().all(f32::is_finite)
}

/// Rejects malformed or oversized native facts before any sandbox import can read them.
pub(super) fn validate(facts: &ModuleObservations) -> Result<()> {
    ensure!(
        facts.world_day.is_none_or(|day| day.session != 0),
        "invalid world day observation"
    );
    ensure!(
        facts.motion.as_ref().is_none_or(|m| m.session != 0
            && m.yaw.is_finite()
            && m.pitch.is_finite()
            && finite(m.velocity)),
        "invalid motion observation"
    );
    ensure!(
        facts.vitals.as_ref().is_none_or(|v| v.session != 0
            && [v.hunger, v.max_hunger, v.saturation]
                .into_iter()
                .flatten()
                .all(|n| n.is_finite() && n >= 0.0)),
        "invalid vitals observation"
    );
    ensure!(
        facts
            .vitals
            .as_ref()
            .and_then(|v| v.held_food.as_ref())
            .is_none_or(
                |food| food.saturation_modifier.is_finite() && food.saturation_modifier >= 0.0
            ),
        "invalid held food observation"
    );
    ensure!(
        facts.actors.len() <= mod_api::MAX_OBSERVED_ACTORS
            && facts.actors.iter().all(|observed| {
                let a = &observed.actor;
                a.session != 0
                    && a.runtime_id != 0
                    && finite(a.position)
                    && a.look_origin.is_none_or(finite)
                    && a.look_direction.is_none_or(finite)
                    && a.yaw.is_finite()
                    && a.pitch.is_finite()
                    && a.identifier.len() <= 128
                    && !a.identifier.chars().any(char::is_control)
                    && a.name.len() <= mod_api::MAX_OBSERVED_ACTOR_NAME_BYTES
                    && !a.name.chars().any(char::is_control)
                    && a.health.is_none_or(|n| n.is_finite() && n >= 0.)
                    && observed.max_health.is_none_or(|n| n.is_finite() && n > 0.)
                    && a.boxes.len() <= mod_api::MAX_OBSERVED_ACTOR_BOXES
                    && a.boxes.iter().all(|b| {
                        finite(b.min)
                            && finite(b.max)
                            && b.min.0 <= b.max.0
                            && b.min.1 <= b.max.1
                            && b.min.2 <= b.max.2
                    })
            }),
        "invalid actor observation"
    );
    ensure!(
        facts.combat.len() <= mod_api::MAX_OBSERVED_COMBAT_EVENTS
            && facts
                .combat
                .windows(2)
                .all(|events| events[0].sequence < events[1].sequence)
            && facts.combat.iter().all(|e| e.sequence != 0
                && e.session != 0
                && e.runtime_id != 0
                && finite(e.position)
                && e.distance
                    .is_none_or(|n| n.is_finite() && (0.0..=128.0).contains(&n))),
        "invalid combat observation"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ActorObservation, ActorObservationBounds, CombatKind};
    /// Creates native facts whose actor and event share an explicit session fence.
    fn facts() -> ModuleObservations {
        ModuleObservations {
            actors: vec![ObservedActor {
                actor: ActorObservation {
                    session: 1,
                    dimension: 0,
                    runtime_id: 5,
                    identifier: "minecraft:player".into(),
                    yaw: 0.,
                    pitch: 0.,
                    pickable: true,
                    hittable: None,
                    look_origin: None,
                    look_direction: None,
                    player: true,
                    position: (0., 0., 0.),
                    boxes: vec![ActorObservationBounds {
                        min: (-0.3, 0., -0.3),
                        max: (0.3, 1.8, 0.3),
                    }],
                    name: "Player".into(),
                    invisible: false,
                    sneaking: false,
                    hurt: false,
                    health: Some(20.),
                },
                max_health: Some(20.),
            }],
            combat: vec![CombatObservation {
                sequence: 1,
                session: 1,
                dimension: 0,
                kind: CombatKind::Hurt,
                local: false,
                player: true,
                runtime_id: 5,
                distance: None,
                position: (0., 0., 0.),
            }],
            ..Default::default()
        }
    }
    #[test]
    fn malformed_boxes_and_repeated_evidence_are_rejected() {
        let mut facts = facts();
        assert!(validate(&facts).is_ok());
        facts.actors[0].max_health = Some(f32::NAN);
        assert!(validate(&facts).is_err());
        facts.actors[0].max_health = Some(0.);
        assert!(validate(&facts).is_err());
        facts.actors[0].max_health = None;
        assert!(validate(&facts).is_ok());
        facts.actors[0].actor.boxes[0].min.0 = f32::NAN;
        assert!(validate(&facts).is_err());
        facts.actors.clear();
        facts.combat.push(facts.combat[0]);
        assert!(validate(&facts).is_err());
    }
}
