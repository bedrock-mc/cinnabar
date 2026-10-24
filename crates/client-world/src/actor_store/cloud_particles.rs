//! Particle presentation of server-owned lingering effect clouds.

use protocol::{ActorMetadataValue, SpawnParticleEffectEvent};
use protocol_ids::ActorDataKey;

use super::{ActorSnapshot, ActorStore};

const DRAGON_BREATH_PARTICLE: i32 = 49;
const EMISSION_TICKS: u32 = 5;
const POTION_EMISSION_TICKS: u32 = 2;
const MIN_RADIUS: f32 = 0.5;
const LINGERING_BREATH: &str = assets::facts::ParticleEffect::DragonBreathLingering.identifier();
const LINGERING_POTION: &str = assets::facts::ParticleEffect::MobspellLingering.identifier();

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
            if actor.entity_fact.native_class
                != assets::facts::entity::NativeEntityClass::AreaEffectCloud
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
            let dragon_breath = matches!(
                actor.metadata.get(&ActorDataKey::DataParticle.wire_value()),
                Some(ActorMetadataValue::Int(DRAGON_BREATH_PARTICLE))
            );
            let emission_ticks = if dragon_breath {
                EMISSION_TICKS
            } else {
                POTION_EMISSION_TICKS
            };
            if !elapsed.is_multiple_of(emission_ticks) {
                continue;
            }
            let lifetime = emission_ticks as f32 * simulation_clock::TICK_DURATION.as_secs_f32();
            let color = if dragon_breath {
                String::new()
            } else {
                let color = match actor.metadata.get(&ActorDataKey::EffectColor.wire_value()) {
                    Some(ActorMetadataValue::Int(color)) => *color as u32,
                    _ => 0,
                };
                let [alpha, red, green, blue] = color
                    .to_be_bytes()
                    .map(|byte| f32::from(byte) / f32::from(u8::MAX));
                format!(
                    ",\"variable.color.r\":{red},\"variable.color.g\":{green},\"variable.color.b\":{blue},\"variable.color.a\":{alpha}"
                )
            };
            let variables = format!(
                "{{\"variable.cloud_lifetime\":{lifetime},\"variable.cloud_radius\":{radius},\"variable.particle_multiplier\":{emission_ticks}{color}}}"
            );
            super::dragon_particles::push_effect(
                &mut self.particle_effects,
                self.latest_sequence,
                self.dimension,
                SpawnParticleEffectEvent {
                    dimension,
                    actor_unique_id: None,
                    position: actor.position,
                    effect: if dragon_breath {
                        LINGERING_BREATH
                    } else {
                        LINGERING_POTION
                    }
                    .into(),
                    molang_variables: Some(variables.into()),
                },
            );
        }
    }
}

/// Computes the shared lifetime and radius admission for every cloud particle profile.
fn cloud_presentation(actor: &ActorSnapshot, elapsed: u32) -> CloudPresentation {
    let Some(ActorMetadataValue::Int(duration)) =
        actor.metadata.get(&ActorDataKey::DataDuration.wire_value())
    else {
        return CloudPresentation::Unavailable;
    };
    let duration = if *duration < -1 { 0 } else { *duration };
    if duration != -1 && elapsed > duration as u32 {
        return CloudPresentation::Expired;
    }
    let Some(ActorMetadataValue::Float(initial)) =
        actor.metadata.get(&ActorDataKey::DataRadius.wire_value())
    else {
        return CloudPresentation::Unavailable;
    };
    let Some(rate) = float_metadata(actor, ActorDataKey::DataChangeRate.wire_value(), -1.0) else {
        return CloudPresentation::Unavailable;
    };
    let Some(change_on_pickup) =
        float_metadata(actor, ActorDataKey::DataChangeOnPickup.wire_value(), -0.5)
    else {
        return CloudPresentation::Unavailable;
    };
    let pickups = match actor
        .metadata
        .get(&ActorDataKey::DataPickupCount.wire_value())
    {
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

/// Reads a finite optional float with the actor metadata getter's default behavior.
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
