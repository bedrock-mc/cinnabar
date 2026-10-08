//! Particle presentation of server-owned lingering dragon breath clouds.

use protocol::{ActorMetadataValue, SpawnParticleEffectEvent};

use super::{ActorKind, ActorSnapshot, ActorStore};

const RADIUS_KEY: u32 = 61;
const PARTICLE_KEY: u32 = 63;
const DURATION_KEY: u32 = 95;
const CHANGE_RATE_KEY: u32 = 97;
const CHANGE_ON_PICKUP_KEY: u32 = 98;
const PICKUP_COUNT_KEY: u32 = 99;
const DRAGON_BREATH_PARTICLE: i32 = 49;
const EMISSION_TICKS: u32 = 5;
const MIN_RADIUS: f32 = 0.5;
const LINGERING_BREATH: &str = "minecraft:dragon_breath_lingering";

enum CloudPresentation {
    Unavailable,
    Expired,
    Active(f32),
}

impl ActorStore {
    pub(super) fn advance_cloud_particles(&mut self) {
        let Ok(dimension) = u8::try_from(self.dimension) else {
            return;
        };
        for actor in self.actors.values_mut() {
            if !matches!(&actor.kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:area_effect_cloud")
            {
                continue;
            }
            if actor.status.cloud_particles_expired {
                continue;
            }
            let Some(start) = actor.status.cloud_start_tick else {
                actor.status.cloud_start_tick = Some(actor.status.age_ticks);
                continue;
            };
            let elapsed = actor.status.age_ticks.saturating_sub(start);
            let radius = match cloud_presentation(actor, elapsed) {
                CloudPresentation::Active(radius) => radius,
                CloudPresentation::Expired => {
                    actor.status.cloud_particles_expired = true;
                    continue;
                }
                CloudPresentation::Unavailable => continue,
            };
            if !elapsed.is_multiple_of(EMISSION_TICKS) {
                continue;
            }
            let lifetime = EMISSION_TICKS as f32 * crate::ACTOR_TICK_DURATION.as_secs_f32();
            let variables = format!(
                "{{\"variable.cloud_lifetime\":{lifetime},\"variable.cloud_radius\":{radius},\"variable.particle_multiplier\":{EMISSION_TICKS}}}"
            );
            super::dragon_particles::push_effect(
                &mut self.particle_effects,
                self.latest_sequence,
                self.dimension,
                SpawnParticleEffectEvent {
                    dimension,
                    actor_unique_id: None,
                    position: actor.position,
                    effect: LINGERING_BREATH.into(),
                    molang_variables: Some(variables.into()),
                },
            );
        }
    }
}

fn cloud_presentation(actor: &ActorSnapshot, elapsed: u32) -> CloudPresentation {
    if !matches!(
        actor.metadata.get(&PARTICLE_KEY),
        Some(ActorMetadataValue::Int(DRAGON_BREATH_PARTICLE))
    ) {
        return CloudPresentation::Unavailable;
    }
    let Some(ActorMetadataValue::Int(duration)) = actor.metadata.get(&DURATION_KEY) else {
        return CloudPresentation::Unavailable;
    };
    let duration = if *duration < -1 { 0 } else { *duration };
    if duration != -1 && elapsed > duration as u32 {
        return CloudPresentation::Expired;
    }
    let Some(ActorMetadataValue::Float(initial)) = actor.metadata.get(&RADIUS_KEY) else {
        return CloudPresentation::Unavailable;
    };
    let Some(rate) = float_metadata(actor, CHANGE_RATE_KEY, -1.0) else {
        return CloudPresentation::Unavailable;
    };
    let Some(change_on_pickup) = float_metadata(actor, CHANGE_ON_PICKUP_KEY, -0.5) else {
        return CloudPresentation::Unavailable;
    };
    let pickups = match actor.metadata.get(&PICKUP_COUNT_KEY) {
        Some(ActorMetadataValue::Int(count)) => (*count).max(0),
        _ => 0,
    };
    let radius = initial + elapsed as f32 * rate + pickups as f32 * change_on_pickup;
    if !initial.is_finite() || !radius.is_finite() {
        CloudPresentation::Unavailable
    } else if radius < MIN_RADIUS {
        CloudPresentation::Expired
    } else {
        CloudPresentation::Active(radius)
    }
}

fn float_metadata(actor: &ActorSnapshot, key: u32, default: f32) -> Option<f32> {
    match actor.metadata.get(&key) {
        Some(ActorMetadataValue::Float(value)) => value.is_finite().then_some(*value),
        Some(_) => Some(0.0),
        None => Some(default),
    }
}

#[cfg(test)]
#[path = "cloud_particles_tests.rs"]
mod tests;
