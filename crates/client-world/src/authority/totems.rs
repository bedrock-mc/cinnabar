//! Optional native activation counts follow accepted player lifetimes, without replaying a notice queue.

use super::WorldAuthority;
use std::collections::HashMap;

#[derive(Default)]
pub(super) struct Tracking {
    dimension: i32,
    counts: HashMap<u64, Count>,
}

struct Count {
    spawn: u64,
    value: u32,
}

impl WorldAuthority {
    /// Borrows only current, living player lifetimes with positive counts, keyed by their roster UUID.
    pub fn player_totem_activations(&self) -> impl Iterator<Item = ([u8; 16], u32, bool)> + '_ {
        self.totems.as_ref().into_iter().flat_map(move |state| {
            state.counts.iter().filter_map(move |(runtime_id, count)| {
                let actor = self.actors.get(*runtime_id)?;
                let protocol::ActorKind::Player { uuid, .. } = &actor.kind else {
                    return None;
                };
                (state.dimension == self.current_dimension
                    && count.spawn == actor.spawn_revision
                    && !actor.status.dead
                    && count.value != 0)
                    .then_some((
                        *uuid,
                        count.value,
                        *runtime_id == self.local_player_runtime_id,
                    ))
            })
        })
    }

    /// Starts a fresh counting interval on enable and immediately forgets it on disable.
    pub fn set_totem_tracking(&mut self, enabled: bool) {
        if !enabled {
            self.totems = None;
        } else if self
            .totems
            .as_ref()
            .is_none_or(|state| state.dimension != self.current_dimension)
        {
            self.totems = Some(Tracking {
                dimension: self.current_dimension,
                ..Default::default()
            });
        }
    }

    /// Returns a current player's count in this enabled interval; retired lifetimes never match.
    pub fn totem_activations(&self, runtime_id: u64) -> Option<u32> {
        let state = self.totems.as_ref()?;
        let actor = self.actors.get(runtime_id)?;
        if state.dimension != self.current_dimension
            || !matches!(actor.kind, protocol::ActorKind::Player { .. })
        {
            return None;
        }
        Some(
            state
                .counts
                .get(&runtime_id)
                .filter(|count| count.spawn == actor.spawn_revision)
                .map_or(0, |count| count.value),
        )
    }

    /// Applies only committed activation/death notices, independently of bounded audio and particle queues.
    pub(super) fn observe_totem_status(&mut self, runtime_id: u64, activated: bool) {
        let Some(state) = &mut self.totems else {
            return;
        };
        let Some(actor) = self.actors.get(runtime_id) else {
            return;
        };
        if !matches!(actor.kind, protocol::ActorKind::Player { .. }) {
            return;
        }
        if state.dimension != self.current_dimension {
            state.dimension = self.current_dimension;
            state.counts.clear();
        }
        if !activated {
            state.counts.remove(&runtime_id);
            return;
        }
        if state.counts.len() == crate::actor_store::MAX_TRACKED_ACTORS {
            state.counts.retain(|runtime_id, count| {
                self.actors
                    .get(*runtime_id)
                    .is_some_and(|actor| actor.spawn_revision == count.spawn)
            });
        }
        let count = state.counts.entry(runtime_id).or_insert(Count {
            spawn: actor.spawn_revision,
            value: 0,
        });
        if count.spawn != actor.spawn_revision {
            *count = Count {
                spawn: actor.spawn_revision,
                value: 0,
            };
        }
        count.value = count.value.saturating_add(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{ActorEvent, ActorKind, ActorStatusKind, WorldBootstrap, WorldEvent};
    use std::sync::Arc;

    /// Creates an isolated world with a normal remote player; no profile or game asset fixture is needed.
    fn world() -> WorldAuthority {
        let mut world = WorldAuthority::new(
            WorldBootstrap {
                local_player_unique_id: 1,
                local_player_runtime_id: 1,
                dimension: 0,
                player_position: [0.; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            },
            Arc::new(assets::RuntimeAssets::diagnostic()),
            None,
            [0.; 3],
            None,
        );
        spawn(&mut world, 1);
        world
    }

    /// Reuses an actor ID with a fresh native spawn revision.
    fn spawn(world: &mut WorldAuthority, sequence: u64) {
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Spawn(protocol::ActorSpawnEvent {
                    dimension: 0,
                    unique_id: 2,
                    runtime_id: 2,
                    kind: ActorKind::Player {
                        uuid: [2; 16],
                        username: "Fixture".into(),
                    },
                    position: [1., 64., 1.],
                    velocity: [0.; 3],
                    pitch: 0.,
                    yaw: 0.,
                    head_yaw: 0.,
                    body_yaw: 0.,
                    held_item: Default::default(),
                    metadata: Arc::from([]),
                    attributes: Arc::from([]),
                    properties: Arc::from([]),
                    links: Arc::from([]),
                })),
                Some(sequence),
            )
            .unwrap();
    }

    /// Sends one status through the same ordered commit boundary as a decoded server notice.
    fn status(world: &mut WorldAuthority, sequence: u64, kind: ActorStatusKind) {
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Status(protocol::ActorStatusEvent {
                    runtime_id: 2,
                    kind,
                    data: 0,
                })),
                Some(sequence),
            )
            .unwrap();
    }

    /// Commits server health through the native attribute owner, without a separate death notice.
    fn health(world: &mut WorldAuthority, sequence: u64, current: f32) {
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Attributes(
                    protocol::ActorAttributesUpdateEvent {
                        dimension: 0,
                        runtime_id: 2,
                        tick: sequence,
                        attributes: Arc::from([protocol::ActorAttribute {
                            name: protocol_ids::Attribute::Health.wire_name().into(),
                            min: 0.,
                            max: 20.,
                            current,
                            default: None,
                            modifiers: Arc::from([]),
                        }]),
                    },
                )),
                Some(sequence),
            )
            .unwrap();
    }

    /// Health-only deaths clear counts before a later revival can expose them again.
    #[test]
    fn health_attribute_death_resets_totems_without_a_status_notice() {
        let mut world = world();
        world.set_totem_tracking(true);
        status(&mut world, 2, ActorStatusKind::TotemActivate);
        health(&mut world, 3, 0.);
        assert_eq!(world.totem_activations(2), Some(0));
        assert!(world.player_totem_activations().next().is_none());
        health(&mut world, 4, 20.);
        assert_eq!(world.totem_activations(2), Some(0));
        status(&mut world, 5, ActorStatusKind::TotemActivate);
        assert_eq!(world.totem_activations(2), Some(1));
    }

    /// Only accepted activations inside the enabled interval count; other status events and disable gaps do not.
    #[test]
    fn confirmed_totem_uses_follow_enable_death_and_actor_lifetimes() {
        let mut world = world();
        status(&mut world, 2, ActorStatusKind::TotemActivate);
        assert_eq!(world.totem_activations(2), None);
        world.set_totem_tracking(true);
        assert_eq!(world.totem_activations(2), Some(0));
        status(&mut world, 3, ActorStatusKind::TotemActivate);
        status(&mut world, 4, ActorStatusKind::Hurt);
        world.set_totem_tracking(true);
        assert_eq!(world.totem_activations(2), Some(1));
        for sequence in 5..105 {
            status(&mut world, sequence, ActorStatusKind::TotemActivate);
        }
        assert_eq!(
            world.totem_activations(2),
            Some(101),
            "bounded consumer queues cannot lose accepted activations"
        );
        status(&mut world, 105, ActorStatusKind::Death);
        assert_eq!(world.totem_activations(2), Some(0));
        status(&mut world, 106, ActorStatusKind::SpawnAlive);
        status(&mut world, 107, ActorStatusKind::TotemActivate);
        assert_eq!(world.totem_activations(2), Some(1));
        world
            .apply_ordered_event(
                WorldEvent::Actor(ActorEvent::Remove(protocol::ActorRemoveEvent {
                    dimension: 0,
                    unique_id: 2,
                })),
                Some(108),
            )
            .unwrap();
        assert_eq!(world.totem_activations(2), None);
        spawn(&mut world, 109);
        assert_eq!(world.totem_activations(2), Some(0));
        status(&mut world, 110, ActorStatusKind::TotemActivate);
        assert_eq!(world.totem_activations(2), Some(1));
        world.set_totem_tracking(false);
        status(&mut world, 111, ActorStatusKind::TotemActivate);
        world.set_totem_tracking(true);
        assert_eq!(world.totem_activations(2), Some(0));
    }
}
