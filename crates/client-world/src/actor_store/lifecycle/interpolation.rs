//! Actor tick state and frame-rate animation publication.

use super::*;
use simulation_clock::identity::{PositionAuthorityGeneration, SessionGeneration};

impl ActorStore {
    /// Java torso ticks follow this local simulation identity rather than the remote actor clock.
    pub(crate) fn set_local_motion_authority(
        &mut self,
        runtime_id: u64,
        authority: Option<(SessionGeneration, PositionAuthorityGeneration)>,
    ) {
        self.animation
            .set_local_motion_authority(runtime_id, authority);
    }

    /// Corrects only local torso motion after this frame's interaction admissions.
    pub(crate) fn sync_local_swing_motion(
        &mut self,
        runtime_id: u64,
        authority: (SessionGeneration, PositionAuthorityGeneration),
        samples: impl IntoIterator<Item = actor_animation::LocalSwingMotionSample>,
    ) {
        self.local_view_dirty |= self
            .animation
            .sync_local_swing_motion(runtime_id, authority, samples);
    }

    /// Changed committed local swing samples refresh the local rig even between actor ticks.
    pub(crate) fn sync_local_swing(
        &mut self,
        runtime_id: u64,
        progress: actor_animation::LocalSwingProgress,
    ) {
        self.local_view_dirty |= self.animation.sync_local_swing(runtime_id, progress);
    }

    /// Advances explicit simulation ticks, retaining the legacy per-tick evaluation contract.
    pub(crate) fn advance_interpolation_ticks(&mut self, ticks: u32) {
        self.advance_interpolation(ticks, false);
    }

    /// Advances all tick state and evaluates visuals for the last two ticks of the interval.
    pub(crate) fn advance_interpolation_frame(&mut self, ticks: u32) {
        self.advance_interpolation(ticks, true);
    }

    /// Predicts where this frame's `ticks` move each remote actor, for picks, without touching
    /// the live actors whose per-tick animation advances later in the frame.
    pub(crate) fn predict_remote_motion(&mut self, ticks: u32) {
        self.picks_ahead = ticks > 0;
        if !self.picks_ahead {
            return;
        }
        let local = self.remote_state_excluded_runtime_id;
        self.pick_states.clear();
        self.pick_states.extend(
            self.actors
                .iter()
                .filter(|(runtime_id, _)| local != Some(**runtime_id))
                .map(|(runtime_id, actor)| movement_interpolation::PickState {
                    runtime_id: *runtime_id,
                    previous_yaw: actor.yaw,
                    state: actor.motion_state(),
                }),
        );
        self.pick_states
            .sort_unstable_by_key(|pick| pick.runtime_id);
        // The live loop's per-tick order: every actor's motion, then seating.
        for _ in 0..ticks {
            for pick in &mut self.pick_states {
                pick.previous_yaw = pick.state.pose.yaw;
                pick.state.tick();
            }
            self.seat_pick_riders();
        }
    }

    /// The predicted pose picks read this frame, or `None` when the live pose is current.
    pub(crate) fn pick_pose(&self, runtime_id: u64) -> Option<ActorPose> {
        if !self.picks_ahead {
            return None;
        }
        self.pick_state(runtime_id).map(|state| state.pose)
    }

    pub(in crate::actor_store) fn pick_state(
        &self,
        runtime_id: u64,
    ) -> Option<&movement_interpolation::MotionState> {
        self.pick(runtime_id).map(|pick| &pick.state)
    }

    /// This frame's predicted motion of a remote actor, while picks are ahead of the live
    /// actors.
    pub(in crate::actor_store) fn pick_ahead(
        &self,
        runtime_id: u64,
    ) -> Option<&movement_interpolation::PickState> {
        self.picks_ahead.then(|| self.pick(runtime_id)).flatten()
    }

    fn pick(&self, runtime_id: u64) -> Option<&movement_interpolation::PickState> {
        self.pick_states
            .binary_search_by_key(&runtime_id, |pick| pick.runtime_id)
            .ok()
            .map(|index| &self.pick_states[index])
    }

    /// One tick of interpolated motion for every actor.
    fn step_motion(&mut self) {
        for actor in self.actors.values_mut() {
            actor.previous_pose = actor.current_pose();
            // Vanilla's interpolation tick clears velocity
            // before decrementing any positive interpolation count, including its last tick.
            if actor.interpolation_ticks_remaining > 0 {
                actor.status.native_velocity = [0.0; 3];
            }
            let mut state = actor.motion_state();
            state.tick();
            actor.apply_motion_state(state);
            if actor.position.map(f32::to_bits) != actor.previous_pose.position.map(f32::to_bits) {
                self.effect_members.collidable.update(actor);
            }
            actor.advance_creeper_swell();
            actor.status.tick();
            actor.tick_boat_hurt();
            actor.finish_player_death(self.world_default_game_mode);
            let using_item = actor.is_using_item();
            actor.status.advance_kinetic_hit(using_item);
        }
    }

    /// Keeps motion and status exact while separating frame and simulation evaluation cadence.
    fn advance_interpolation(&mut self, ticks: u32, frame: bool) {
        let refresh_view = frame && ticks == 0 && self.local_view_dirty;
        if ticks > 0 || refresh_view {
            self.local_view_dirty = false;
        }
        self.prepare_appearances(ticks > 0);
        // The live actors now reach the positions picks were reading.
        self.picks_ahead = false;
        let prepare_frame = frame
            && ticks == 0
            && (refresh_view
                || self.animation.needs_initial_presentation(
                    &self.actors,
                    self.animation_view.as_ref(),
                    self.remote_state_excluded_runtime_id,
                    |actor| self.appearance_ready(actor),
                ));
        for tick in 0..ticks.max(u32::from(prepare_frame)) {
            if !prepare_frame {
                if let Some(pending) = &mut self.pending_local_damage {
                    pending.tick();
                }
                self.step_motion();
                self.show_local_vehicle_render();
                self.seat_riders();
            }
            if !prepare_frame {
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
            let world_game_mode = self.world_default_game_mode;
            let local_hands = self.local_hands.clone();
            let local_equipment_count = self.local_equipment_count;
            let local_held_network_ids = self.local_held_network_ids;
            let local_item_use = self.local_item_use;
            let local_main_metadata = self.local_main_metadata;
            let local_held_variants = self.local_held_variants;
            let local_main_slot = self.local_main_slot;
            let local_main_stack_id = self.local_main_stack_id;
            let local_bedrock_swing_ticks = self.local_bedrock_swing_ticks;
            let local_java_swing_ticks = self.local_java_swing_ticks;
            let view = self.animation_view.as_ref();
            // A locked seat turns its rider's body along the seat's facing.
            let seat_facings: FastHashMap<i64, f32> = self
                .rider_to_ridden
                .keys()
                .filter_map(|rider| Some((*rider, self.rider_view_lock(*rider, 1.0)?.facing())))
                .collect();
            let context = |actor: &ActorSnapshot| {
                let lifetime = ActorLifetimeId {
                    session_id,
                    dimension,
                    runtime_id: actor.runtime_id,
                    spawn_revision: actor.spawn_revision,
                };
                let is_local = local_runtime == Some(actor.runtime_id);
                // Each hand's equipment is looked up once; every held-item field reads it.
                let right = items.get_in_hand(lifetime, protocol::ActorHandedness::Right);
                let left = items.get_in_hand(lifetime, protocol::ActorHandedness::Left);
                let held_identifier = |equipment: Option<&crate::ActorEquipmentSnapshot>| {
                    equipment
                        .filter(|equipment| equipment.item.identity.network_id != 0)
                        .and_then(|equipment| equipment.item.identifier.clone())
                };
                let mut hand_charged = [right, left].into_iter().any(|equipment| {
                    equipment.is_some_and(|equipment| equipment.item.charged_projectile.is_some())
                });
                let main_hand = if is_local {
                    local_hands[0].clone()
                } else {
                    held_identifier(right)
                };
                let main_hand_metadata = if is_local {
                    local_main_metadata
                } else {
                    right.map_or(0, |equipment| {
                        equipment
                            .item
                            .damage
                            .unwrap_or(equipment.item.identity.metadata)
                    })
                };
                let main_hand_use = items.held_use_profile(
                    lifetime,
                    protocol::ActorHandedness::Right,
                    is_local.then_some(local_held_network_ids[0]),
                );
                let mut main_hand_max_use_ticks = main_hand_use.ticks.unwrap_or(0);
                let local_item_use_ticks = if is_local
                    && let LocalItemUse::Predicted {
                        elapsed_ticks,
                        max_ticks,
                        charged,
                    } = local_item_use
                {
                    main_hand_max_use_ticks = max_ticks;
                    hand_charged = charged;
                    Some(elapsed_ticks.unwrap_or(0).min(max_ticks))
                } else {
                    None
                };
                let off_hand = if is_local {
                    local_hands[1].clone()
                } else {
                    held_identifier(left)
                };
                let off_hand_max_use_ticks = items
                    .held_use_profile(
                        lifetime,
                        protocol::ActorHandedness::Left,
                        is_local.then_some(local_held_network_ids[1]),
                    )
                    .ticks
                    .unwrap_or(0);
                let attack = main_hand.as_deref().and_then(|id| items.attack_timing(id));
                let main_hand_kinetic = attack.and_then(|attack| attack.kinetic_weapon);
                let main_hand_is_spear = attack.is_some_and(|attack| attack.is_spear);
                let main_hand_swing_seconds = attack
                    .and_then(|attack| attack.swing_duration_ticks)
                    .map(|ticks| ticks as f32 * simulation_clock::TICK_DURATION.as_secs_f32());
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
                actor_animation::ActorTickContext {
                    combat_visuals: render_api::DEFAULT_COMBAT_VISUALS,
                    appearance_pending: !self.ready_appearances.admits(
                        actor, prepared, &self.players, &self.unlisted_players,
                    ),
                    frame_alpha: 0.0,
                    // The last two evaluations split the interval, so script time advances once.
                    animation_elapsed_ticks: frame.then_some(if ticks == 0 || tick + 1 == ticks {
                        ticks.min(1)
                    } else {
                        ticks - 1
                    }),
                    is_riding: rider_to_ridden.contains_key(&actor.unique_id),
                    seat_facing: seat_facings.get(&actor.unique_id).copied(),
                    hand_charged,
                    main_hand,
                    held_item_facts: [protocol::ActorHandedness::Right, protocol::ActorHandedness::Left]
                        .map(|hand| {
                            let network_id = if is_local {
                                local_held_network_ids[usize::from(hand == protocol::ActorHandedness::Left)]
                            } else {
                                items.get_in_hand(lifetime, hand).map_or(0, |equipment| equipment.item.identity.network_id)
                            };
                            items.animation_fact(network_id)
                        }),
                    main_hand_metadata,
                    held_variants: if is_local {
                        local_held_variants
                    } else {
                        [right, left].map(|equipment| {
                            equipment
                                .map(|equipment| (&equipment.item.identity).into())
                                .unwrap_or_default()
                        })
                    },
                    main_hand_stack_id: if is_local {
                        local_main_stack_id
                    } else {
                        right
                            .map(|equipment| equipment.item.identity.stack_network_id)
                            .filter(|id| *id > 0)
                    },
                    bedrock_swing_ticks: if is_local {
                        local_bedrock_swing_ticks
                    } else {
                        actor
                            .status
                            .swing_effects
                            .bedrock_ticks(render_api::DEFAULT_COMBAT_VISUALS)
                    },
                    java_swing_ticks: if is_local {
                        local_java_swing_ticks
                    } else {
                        actor.status.swing_effects.ticks()
                    },
                    main_hand_slot: if is_local {
                        local_main_slot
                    } else {
                        right.map_or(0, |equipment| equipment.selected_slot)
                    },
                    main_hand_max_use_ticks,
                    main_hand_consumption: main_hand_use.consumption,
                    local_item_use_ticks,
                    off_hand_max_use_ticks,
                    main_hand_kinetic,
                    main_hand_is_spear,
                    main_hand_swing_seconds,
                    off_hand,
                    ridden: rider_to_ridden
                        .get(&actor.unique_id)
                        .and_then(kind_of)
                        .map(|kind| match kind {
                            ActorKind::Player { .. } => std::sync::Arc::from(assets::facts::entity::NativeEntityClass::Player.identifier().expect("pinned player identity")),
                            ActorKind::Entity { identifier } => std::sync::Arc::clone(identifier),
                        }),
                    has_rider,
                    has_player_rider,
                    attachable: None,
                    is_local_first_person: local_first_person == Some(actor.runtime_id),
                    is_local_player: is_local,
                    view_bobbing: is_local.then_some(local_view_bobbing),
                    is_local,
                    is_flying: is_local && local_flying,
                    world_game_mode,
                    is_in_ui: false,
                    camera_rotation,
                    camera_position,
                    armor: worn_armor(items, items.armor(actor.runtime_id)),
                    equipment_count: if is_local { local_equipment_count } else { items.armor(actor.runtime_id).map_or(0, |armor| armor.equipment_count()) },
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
            if prepare_frame {
                self.animation
                    .prepare_frame(actors, view, local_runtime, refresh_view, context);
            } else {
                // The last two ticks evaluate, so the drawn blend spans exactly one tick.
                self.animation.advance_tick(
                    actors,
                    view,
                    local_runtime,
                    !frame || tick + 2 >= ticks,
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
    items: &crate::item::ItemStateStore,
    snapshot: Option<&crate::item::ActorArmorSnapshot>,
) -> [Option<actor_animation::WornArmor>; 5] {
    let piece = |piece: &crate::item::ActorArmorPiece| {
        Some(actor_animation::WornArmor {
            item: piece.item.identifier.clone()?,
            fact: items.animation_fact(piece.item.identity.network_id),
            dye_rgb: piece.dye_rgb,
        })
    };
    snapshot.map_or_else(Default::default, |armor| armor.pieces().map(piece))
}

/// Aggregates the rider flags used by each actor's animation context.
fn rider_contexts(
    links: impl Iterator<Item = (i64, i64)>,
    mut is_player: impl FnMut(i64) -> bool,
) -> FastHashMap<i64, (bool, bool)> {
    let mut contexts = FastHashMap::new();
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
