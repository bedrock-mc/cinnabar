//! Actor tick state and frame-rate animation publication.

use super::*;

impl ActorStore {
    /// Java torso ticks follow this local simulation identity rather than the remote actor clock.
    pub(crate) fn set_local_motion_authority(
        &mut self,
        runtime_id: u64,
        authority: Option<(u64, u64)>,
    ) {
        self.animation
            .set_local_motion_authority(runtime_id, authority);
    }

    /// Corrects only local torso motion after this frame's interaction admissions.
    pub(crate) fn sync_local_swing_motion(
        &mut self,
        runtime_id: u64,
        authority: (u64, u64),
        samples: impl IntoIterator<Item = crate::LocalSwingMotionSample>,
    ) {
        self.local_view_dirty |= self
            .animation
            .sync_local_swing_motion(runtime_id, authority, samples);
    }

    /// Changed committed local swing samples refresh the local rig even between actor ticks.
    pub(crate) fn sync_local_swing(
        &mut self,
        runtime_id: u64,
        progress: crate::LocalSwingProgress,
    ) {
        self.local_view_dirty |= self.animation.sync_local_swing(runtime_id, progress);
    }

    /// Advances explicit simulation ticks, retaining the legacy per-tick evaluation contract.
    pub(crate) fn advance_interpolation_ticks(&mut self, ticks: u32) {
        self.advance_interpolation(ticks, false);
    }

    /// Advances all tick state and evaluates visuals once with the full elapsed interval.
    pub(crate) fn advance_interpolation_frame(&mut self, ticks: u32) {
        self.advance_interpolation(ticks, true);
    }

    /// Keeps motion and status exact while separating frame and simulation evaluation cadence.
    fn advance_interpolation(&mut self, ticks: u32, frame: bool) {
        let refresh_view = frame && ticks == 0 && self.local_view_dirty;
        if ticks > 0 || refresh_view {
            self.local_view_dirty = false;
        }
        self.prepare_appearances(ticks > 0);
        for tick in 0..ticks.max(u32::from(refresh_view)) {
            if !refresh_view {
                for actor in self.actors.values_mut() {
                    let current = actor.current_pose();
                    actor.previous_pose = current;
                    let mut next =
                        if actor.interpolation_ticks_remaining == 0 && actor.is_dying_dragon() {
                            current
                        } else {
                            actor.received_pose
                        };
                    // Vanilla's interpolation tick clears velocity
                    // before decrementing any positive interpolation count, including its last tick.
                    if actor.interpolation_ticks_remaining > 0 {
                        actor.status.native_velocity = [0.0; 3];
                    }
                    // The final step lands exactly on the target.
                    if actor.interpolation_ticks_remaining > 1 {
                        // Each step closes 1/n of the remaining gap; angles take the short way.
                        let divisor = actor.interpolation_ticks_remaining as f32;
                        let target = actor.received_pose;
                        next.position = std::array::from_fn(|axis| {
                            current.position[axis]
                                + (target.position[axis] - current.position[axis]) / divisor
                        });
                    }
                    actor.interpolate_movement_rotation(current, &mut next);
                    actor.interpolation_ticks_remaining =
                        actor.interpolation_ticks_remaining.saturating_sub(1);
                    actor.set_current_pose(next);
                    actor.advance_movement_interpolation();
                    actor.status.tick();
                    if let Some(wither) = &mut actor.status.wither_animation {
                        wither.tick(actor.status.dead);
                    }
                }
                self.seat_riders();
            }
            if !refresh_view {
                self.advance_pickup_visuals();
                self.advance_dragon_animation();
                self.advance_dragon_beams();
                self.advance_dragon_particles();
                self.advance_synchronized_audio();
            }
            let (session_id, dimension) = (self.session_id, self.dimension);
            let (actors, unique_to_runtime) = (&self.actors, &self.unique_to_runtime);
            let (rider_to_ridden, items) = (&self.rider_to_ridden, &self.items);
            let rider_contexts = rider_contexts(
                rider_to_ridden
                    .iter()
                    .map(|(&rider, &ridden)| (rider, ridden)),
                |rider| {
                    unique_to_runtime
                        .get(&rider)
                        .and_then(|runtime| actors.get(runtime))
                        .is_some_and(|actor| matches!(actor.kind, ActorKind::Player { .. }))
                },
            );
            let camera_rotation = self.camera_rotation;
            let camera_position = self.camera_position;
            let property_registry = &self.property_registry;
            let prepared = self.animation.has_skin_preparation();
            let players = if prepared {
                &self.ready_appearances.profiles
            } else {
                &self.players
            };
            let unlisted = if prepared {
                &self.ready_appearances.profiles
            } else {
                &self.unlisted_players
            };
            let local_first_person = self
                .remote_state_excluded_runtime_id
                .filter(|_| self.local_first_person);
            let local_runtime = self.remote_state_excluded_runtime_id;
            let local_view_bobbing = self.local_view_bobbing;
            let local_flying = self.local_flying;
            let local_hands = self.local_hands.clone();
            let local_main_metadata = self.local_main_metadata;
            let local_main_slot = self.local_main_slot;
            let local_main_stack_id = self.local_main_stack_id;
            let local_bedrock_swing_ticks = self.local_bedrock_swing_ticks;
            let local_java_swing_ticks = self.local_java_swing_ticks;
            let view = self.animation_view.as_ref();
            let context = |actor: &ActorSnapshot| {
                let lifetime = ActorLifetimeId {
                    session_id,
                    dimension,
                    runtime_id: actor.runtime_id,
                    spawn_revision: actor.spawn_revision,
                };
                let is_local = local_runtime == Some(actor.runtime_id);
                let held = |hand| {
                    if is_local {
                        return local_hands[usize::from(hand != protocol::ActorHandedness::Right)]
                            .clone();
                    }
                    items
                        .get_in_hand(lifetime, hand)
                        .filter(|equipment| equipment.item.identity.network_id != 0)
                        .and_then(|equipment| equipment.item.identifier.clone())
                };
                let hand_charged = [
                    protocol::ActorHandedness::Right,
                    protocol::ActorHandedness::Left,
                ]
                .into_iter()
                .any(|hand| {
                    items
                        .get_in_hand(lifetime, hand)
                        .is_some_and(|equipment| equipment.item.charged_projectile.is_some())
                });
                let main_hand = held(protocol::ActorHandedness::Right);
                let main_hand_metadata = if is_local {
                    local_main_metadata
                } else {
                    items
                        .get_in_hand(lifetime, protocol::ActorHandedness::Right)
                        .map_or(0, |equipment| {
                            equipment
                                .item
                                .damage
                                .unwrap_or(equipment.item.identity.metadata)
                        })
                };
                let main_hand_max_use_ticks = main_hand
                    .as_deref()
                    .and_then(|identifier| items.max_use_ticks(identifier))
                    .unwrap_or(0);
                let kind_of = |unique_id: &i64| {
                    unique_to_runtime
                        .get(unique_id)
                        .and_then(|runtime_id| actors.get(runtime_id))
                        .map(|actor| &actor.kind)
                };
                let (has_rider, has_player_rider) = rider_contexts
                    .get(&actor.unique_id)
                    .copied()
                    .unwrap_or_default();
                crate::actor_animation::ActorTickContext {
                    frame_alpha: 0.0,
                    animation_elapsed_ticks: frame.then_some(ticks),
                    is_riding: rider_to_ridden.contains_key(&actor.unique_id),
                    hand_charged,
                    main_hand,
                    main_hand_metadata,
                    main_hand_stack_id: if is_local {
                        local_main_stack_id
                    } else {
                        items
                            .get_in_hand(lifetime, protocol::ActorHandedness::Right)
                            .map(|equipment| equipment.item.identity.stack_network_id)
                            .filter(|id| *id > 0)
                    },
                    bedrock_swing_ticks: if is_local {
                        local_bedrock_swing_ticks
                    } else {
                        0
                    },
                    java_swing_ticks: if is_local {
                        local_java_swing_ticks
                    } else {
                        crate::ACTOR_SWING_TICKS
                    },
                    main_hand_slot: if is_local {
                        local_main_slot
                    } else {
                        items
                            .get_in_hand(lifetime, protocol::ActorHandedness::Right)
                            .map_or(0, |equipment| equipment.selected_slot)
                    },
                    main_hand_max_use_ticks,
                    off_hand: held(protocol::ActorHandedness::Left),
                    ridden: rider_to_ridden
                        .get(&actor.unique_id)
                        .and_then(kind_of)
                        .map(|kind| match kind {
                            ActorKind::Player { .. } => std::sync::Arc::from("minecraft:player"),
                            ActorKind::Entity { identifier } => std::sync::Arc::clone(identifier),
                        }),
                    has_rider,
                    has_player_rider,
                    attachable: None,
                    is_local_first_person: local_first_person == Some(actor.runtime_id),
                    view_bobbing: is_local.then_some(local_view_bobbing),
                    is_local,
                    is_flying: is_local && local_flying,
                    is_in_ui: false,
                    camera_rotation,
                    camera_position,
                    armor: worn_armor(items.armor(actor.runtime_id)),
                    properties: property_registry.for_kind(&actor.kind),
                    skin_geometry: match &actor.kind {
                        ActorKind::Player { uuid, .. } => players.get(uuid).or_else(|| unlisted.get(uuid)).and_then(|profile| {
                            match &profile.skin {
                                protocol::PlayerSkin::Standard(skin) => skin.geometry.clone(),
                                protocol::PlayerSkin::Unavailable(_) => None,
                            }
                        }),
                        ActorKind::Entity { .. } => None,
                    },
                    has_cape: match &actor.kind {
                        ActorKind::Player { uuid, .. } => players.get(uuid).or_else(|| unlisted.get(uuid)).is_some_and(|profile| {
                            matches!(&profile.skin, protocol::PlayerSkin::Standard(skin) if skin.cape.is_some())
                        }),
                        ActorKind::Entity { .. } => false,
                    },
                }
            };
            if refresh_view {
                if let Some(runtime_id) = local_runtime {
                    self.animation
                        .refresh_local_view(actors, runtime_id, context);
                }
            } else {
                self.animation.advance_tick(
                    actors,
                    view,
                    local_runtime,
                    !frame || tick + 1 == ticks,
                    !frame || tick == 0,
                    context,
                );
                self.actions.advance_tick();
            }
        }
    }
}

/// Worn stacks in helmet, chestplate, leggings, boots, body order.
fn worn_armor(
    snapshot: Option<&crate::item::ActorArmorSnapshot>,
) -> [Option<crate::actor_animation::WornArmor>; 5] {
    let piece = |piece: &crate::item::ActorArmorPiece| {
        Some(crate::actor_animation::WornArmor {
            item: piece.item.identifier.clone()?,
            dye_rgb: piece.dye_rgb,
        })
    };
    snapshot.map_or_else(Default::default, |armor| {
        [
            piece(&armor.helmet),
            piece(&armor.chestplate),
            piece(&armor.leggings),
            piece(&armor.boots),
            piece(&armor.body),
        ]
    })
}

/// Aggregates the rider flags used by each actor's animation context.
fn rider_contexts(
    links: impl Iterator<Item = (i64, i64)>,
    mut is_player: impl FnMut(i64) -> bool,
) -> HashMap<i64, (bool, bool)> {
    let mut contexts = HashMap::new();
    for (rider, ridden) in links {
        let flags = contexts.entry(ridden).or_insert((false, false));
        flags.0 = true;
        flags.1 |= is_player(rider);
    }
    contexts
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn review_riding_contexts_visit_each_link_once_and_retain_missing_riders() {
        let visits = std::cell::Cell::new(0);
        let links = [(1, 20), (99, 30)]
            .into_iter()
            .inspect(|_| visits.set(visits.get() + 1));
        let flags = rider_contexts(links, |rider| rider == 1);
        assert_eq!(flags.get(&20), Some(&(true, true)));
        assert_eq!(flags.get(&30), Some(&(true, false)));
        assert_eq!(visits.get(), 2);
    }
}
