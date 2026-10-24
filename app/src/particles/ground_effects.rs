//! Catalog-driven, world-space ground contacts for equipped local and remote players.
use chunk_pipeline::WorldStream;
use client_interaction::movement::LocalPhysicsController;
use client_presentation::actor_publication::GroundEffectWearer;
use particles::{Fluid, ParticleSystem, ParticleWorld, SpawnRequest, TileRequest};
use render_model::hats::{
    self, CosmeticTrigger,
    ground::{Cadence, Cue},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

mod surface;
#[cfg(test)]
mod tests;

#[derive(Default)]
struct Track {
    cadence: Cadence,
    tick: Option<u64>,
    reset_revision: Option<u64>,
    local_authority: Option<u64>,
    previous: Option<[f32; 3]>,
    forward: [f32; 2],
}

/// Ground contacts are reset by session and actor lifetime, independently of F5 and body culling.
#[derive(Default)]
pub(super) struct GroundEffects {
    instance: Option<(u64, u64, i32)>,
    tracks: HashMap<GroundEffectWearer, Track>,
    draining: Vec<u64>,
    rejected: HashSet<Arc<str>>,
}

impl GroundEffects {
    /// Emits one burst per contact and lets already planted particles finish after unequipping.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn drive(
        &mut self,
        system: &mut ParticleSystem,
        local_source: Option<(&LocalPhysicsController, u64)>,
        world: &dyn ParticleWorld,
        stream: &WorldStream,
        wearers: &[GroundEffectWearer],
        dt: f32,
        alpha: f32,
    ) {
        let physics = local_source.map(|(physics, _)| physics);
        let identity = (
            system.instance(),
            stream.authority().actor_session_id(),
            stream.current_dimension(),
        );
        if self.instance != Some(identity) {
            *self = Self {
                instance: Some(identity),
                ..Default::default()
            };
        }
        self.draining.retain(|id| system.contains_emitter(*id));
        self.tracks.retain(|key, _| wearers.contains(key));
        for &wearer in wearers {
            let Some(actor) = stream
                .authority()
                .actor(wearer.runtime_id)
                .filter(|a| a.spawn_revision == wearer.spawn_revision)
            else {
                continue;
            };
            let local = wearer.runtime_id == stream.local_player_runtime_id();
            if actor.is_invisible() || actor.is_sleeping() || actor.is_gliding() {
                self.tracks.remove(&wearer);
                continue;
            }
            let track = self.tracks.entry(wearer).or_insert_with(|| Track {
                // Equipping starts from the current physics tick, never retained pre-equip history.
                tick: if local {
                    physics.and_then(|p| p.state()).map(|s| s.tick)
                } else {
                    None
                },
                local_authority: if local {
                    local_source.map(|(_, epoch)| epoch)
                } else {
                    None
                },
                ..Default::default()
            });
            if actor.teleported && track.reset_revision != Some(actor.movement_revision) {
                *track = Track {
                    reset_revision: Some(actor.movement_revision),
                    tick: if local {
                        physics.and_then(|p| p.state()).map(|s| s.tick)
                    } else {
                        None
                    },
                    ..Default::default()
                };
            }
            if local {
                let Some((physics, epoch)) = local_source else {
                    continue;
                };
                if track.local_authority != Some(epoch) {
                    *track = Track {
                        local_authority: Some(epoch),
                        tick: physics.state().map(|state| state.tick),
                        ..Default::default()
                    };
                }
                if physics
                    .state()
                    .is_some_and(|s| track.tick.is_some_and(|t| s.tick < t))
                {
                    *track = Track::default();
                }
                physics.visit_completed_ticks(track.tick, &mut |sample, environment, _| {
                    track.tick = Some(sample.tick);
                    if environment.in_water
                        || environment.in_lava
                        || !matches!(
                            sample.mode,
                            sim::MovementMode::Walking | sim::MovementMode::Crawling
                        )
                    {
                        track.cadence = Cadence::default();
                        track.previous = None;
                        return;
                    }
                    let feet = [
                        sample.position[0],
                        sample.position[1] - protocol::PLAYER_NETWORK_OFFSET,
                        sample.position[2],
                    ];
                    contact(
                        system,
                        world,
                        wearer,
                        track,
                        feet,
                        sample.grounded_after_tick,
                        sample.walks,
                        1.0 / simulation_clock::TICKS_PER_SECOND as f32,
                        sample.tick,
                        &mut self.draining,
                        &mut self.rejected,
                    );
                });
            } else if let Some(feet) = actor.interpolated_position(alpha) {
                contact(
                    system,
                    world,
                    wearer,
                    track,
                    feet,
                    remote_grounded(world, feet, actor.on_ground == Some(true)),
                    !actor.is_sneaking(),
                    dt,
                    actor.movement_revision,
                    &mut self.draining,
                    &mut self.rejected,
                );
            }
        }
    }
}

/// Samples contact heading once and places matching one-shot emitters on supported floor shapes.
#[allow(clippy::too_many_arguments)]
fn contact(
    system: &mut ParticleSystem,
    world: &dyn ParticleWorld,
    wearer: GroundEffectWearer,
    track: &mut Track,
    feet: [f32; 3],
    grounded: bool,
    walks: bool,
    dt: f32,
    seed: u64,
    draining: &mut Vec<u64>,
    rejected: &mut HashSet<Arc<str>>,
) {
    if dt > 0.25 || !dt.is_finite() {
        track.cadence = Cadence::default();
        track.previous = None;
        return;
    }
    if let Some(old) = track.previous.replace(feet) {
        let delta = [feet[0] - old[0], feet[2] - old[2]];
        let length = delta[0].hypot(delta[1]);
        if length > 0.001 && length <= 2.0 {
            track.forward = [delta[0] / length, delta[1] / length];
        }
    }
    let Some(cue) = track.cadence.advance(feet, grounded, walks, dt) else {
        return;
    };
    let forward = if track.forward == [0.0; 2] {
        [0.0, 1.0]
    } else {
        track.forward
    };
    let right = [forward[1], -forward[0]];
    let (trigger, lateral, radius) = match cue {
        Cue::Step(side) => (CosmeticTrigger::Step, side, 0.5),
        Cue::Land => (CosmeticTrigger::Landing, 0.0, 1.0),
    };
    let mut point = [
        feet[0] + right[0] * lateral,
        feet[1],
        feet[2] + right[1] * lateral,
    ];
    // A rotated square's AABB is conservative; suppress marks at ledges or uneven steps.
    let extent = radius * (forward[0].abs() + forward[1].abs());
    let Some(top) = surface::height(world, point, extent) else {
        return;
    };
    point[1] = top + 0.004;
    if world.fluid(point.map(|v| v.floor() as i32)) != Fluid::None {
        return;
    }
    let basis = [
        [right[0], 0.0, right[1]],
        [0.0, 1.0, 0.0],
        [forward[0], 0.0, forward[1]],
    ];
    for (index, effect) in hats::rig(wearer.item).emitters().iter().enumerate() {
        if effect.trigger != trigger || rejected.contains(&effect.identifier) {
            continue;
        }
        if draining.len() >= super::cosmetics::MAX_COSMETIC_EMITTERS
            || system.emitter_count() >= particles::MAX_EMITTERS
        {
            break;
        }
        if !system.has_effect(&effect.identifier) && !system.register_effect(&effect.effect) {
            rejected.insert(Arc::clone(&effect.identifier));
            continue;
        }
        let request = SpawnRequest {
            effect: effect.identifier.to_string(),
            position: point,
            basis: Some(basis),
            tile: effect.sprite.as_ref().map(|sprite| TileRequest {
                key: sprite.key,
                size: sprite.side,
                pixels: Arc::clone(&sprite.pixels),
            }),
            seed: seed ^ wearer.runtime_id.rotate_left(16) ^ index as u64,
            ..Default::default()
        };
        if let Some(id) = system.spawn(&request) {
            draining.push(id);
        }
    }
}

/// A packet's grounded target does not ground interpolated feet before they reach support.
fn remote_grounded(world: &dyn ParticleWorld, feet: [f32; 3], packet_grounded: bool) -> bool {
    packet_grounded && surface::height(world, feet, 0.01).is_some()
}
