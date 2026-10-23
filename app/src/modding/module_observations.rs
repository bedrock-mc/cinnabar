//! Session-fenced read-only native facts. Combat records retain evidence, never inferred damage.
use super::gameplay::GameplayContext;
use client_ui::ui_runtime::UiRuntime;
use mod_host::{
    ActorObservation, ActorObservationBounds, CombatKind, CombatObservation, MotionObservation,
    ObservedActor, VitalsObservation, WorldObservation, module_observations::ModuleObservations,
};
use simulation_clock::identity::SessionGeneration;
use std::{
    collections::{BinaryHeap, VecDeque},
    path::PathBuf,
    sync::{Mutex, mpsc},
    time::{Duration, Instant},
};

/// Captures one bounded history and one outstanding asynchronous core status request.
#[derive(Default)]
pub(super) struct ModuleObserver {
    scope: Option<(u64, i32)>,
    server_revision: u64,
    attack_revision: u64,
    sequence: u64,
    history: VecDeque<CombatObservation>,
    ping: PingPoll,
}

/// Keeps observed identifiers bounded at a valid UTF-8 prefix.
fn bounded_text(text: &str) -> String {
    let mut end = text.len().min(128);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end]
        .chars()
        .filter(|character| !character.is_control())
        .collect()
}

/// Current native ray target, including admitted range and loaded-world line of sight.
/// Unknown input/ray/block authority stays unknown instead of becoming a target guess.
pub(super) fn current_target(
    context: &GameplayContext<'_>,
    ui: &UiRuntime,
    interaction: Option<&super::interaction::ModInteraction>,
) -> Option<Option<u64>> {
    let origin = context.origin.as_deref()?;
    let ray = origin.outbound_ray()?;
    let world = context.world.as_deref()?;
    let stream = world.stream.as_ref()?;
    let movement = context.movement.as_deref()?;
    let input = context.input.as_deref()?.snapshot()?;
    let player = context.player.as_deref()?;
    if !movement.accepts_block_interactions()
        || !crate::interaction_authority::ray_is_current(
            ray,
            SessionGeneration::new(ui.session_id()),
            stream,
        )
    {
        return None;
    }
    let mode = gameplay::mining::protocol_input_mode(input.input_mode);
    let caps = player.facts.game_mode_capabilities();
    let reach = if caps.is_some_and(|caps| caps.creative_reach) {
        gameplay::mining::creative_reach(mode)
    } else {
        gameplay::mining::survival_reach(mode)
    };
    let actor_reach = super::interaction::effective_reach(reach, interaction);
    let attack_reach = super::interaction::effective_reach(
        caps.map_or(
            client_world::game_mode_capabilities::SURVIVAL_ATTACK_REACH,
            |caps| caps.attack_reach,
        ),
        interaction,
    );
    let selection = client_interaction::mining::hand_interaction_selection(player)?;
    let observed = crate::interaction_authority::observe_block_ray(
        origin,
        ui,
        world,
        context.collisions.as_deref()?,
        selection,
        player.facts.player_game_mode(),
        (
            mode,
            super::interaction::block_pick_reach(reach, actor_reach),
            (input.authority_generation, input.frame_sequence),
            movement.position_authority_identity().1,
        ),
    )
    .ok()?;
    let ray_origin = ray.origin().to_array();
    let block_distance = observed.map(|block| block.hit_distance());
    let aim = context.aim.as_deref()?;
    let authority = stream.authority();
    let hit = gameplay::melee::pick_actor_by(
        authority
            .remote_actors()
            .filter(|actor| match aim.target.map(|target| target.kind) {
                Some(client_presentation::aim_assist::TargetKind::Actor(lifetime)) => {
                    actor.runtime_id == lifetime.runtime_id
                }
                Some(client_presentation::aim_assist::TargetKind::Block { .. }) => false,
                None => true,
            }),
        |actor| authority.pick_hit_boxes(actor),
        ui.gameplay_hud().mount_unique_id(),
        ray_origin,
        ray.direction().to_array(),
        actor_reach,
    );
    Some(
        match gameplay::melee::classify(hit, block_distance, attack_reach) {
            gameplay::melee::Crosshair::Actor(hit) => Some(hit.runtime_id),
            _ => None,
        },
    )
}

/// Player origin comes from retained wire pose; other actors use their authored head pivot.
fn look_origin(
    authority: &client_world::WorldAuthority,
    actor: &client_world::ActorSnapshot,
    position: [f32; 3],
) -> Option<[f32; 3]> {
    if matches!(actor.kind, protocol::ActorKind::Player { .. }) {
        return actor.native_position_at(position);
    }
    let rig = authority.actor_rig(actor.runtime_id)?;
    let index = rig
        .bone_names
        .iter()
        .position(|name| name.as_ref() == "head")?;
    let head = rig.current.get(index)?;
    let local = [
        head.translation_scale[0] / 16.0 * rig.axis_scale[0],
        head.translation_scale[1] / 16.0 * rig.axis_scale[1],
        head.translation_scale[2] / 16.0 * rig.axis_scale[2],
    ];
    let transform = client_presentation::presentation::actors::rig_world_from_actor(
        position,
        rig.body_yaw,
        actor.render_scale() * rig.scale,
    );
    Some(std::array::from_fn(|row| {
        transform[row][3]
            + (0..3)
                .map(|axis| transform[row][axis] * local[axis])
                .sum::<f32>()
    }))
}

/// A status response retains the session that requested its optional connection.
type PingResponse = (u64, Option<bridge::NetworkConnection>);

/// One request in flight prevents an unavailable core from spawning unbounded workers.
#[derive(Default)]
struct PingPoll {
    pending: Option<Mutex<mpsc::Receiver<PingResponse>>>,
    next_poll: Option<Instant>,
    session: u64,
    attempt: u64,
    value: Option<u32>,
}
impl PingPoll {
    /// Clears stale samples and accepts only this session's retained local runtime identity.
    fn sample(&mut self, session: u64, runtime_id: u64, directory: Option<PathBuf>) -> Option<u32> {
        if session != self.session {
            self.session = session;
            self.attempt = 0;
            self.value = None;
            self.next_poll = None;
        }
        let response = self.pending.as_ref().and_then(|pending| {
            pending
                .lock()
                .ok()
                .and_then(|receiver| match receiver.try_recv() {
                    Ok(result) => Some(Some(result)),
                    Err(mpsc::TryRecvError::Disconnected) => Some(None),
                    Err(mpsc::TryRecvError::Empty) => None,
                })
        });
        if let Some(response) = response {
            self.pending = None;
            if let Some((response_session, connection)) = response {
                if response_session == session {
                    self.value = self.accept(runtime_id, connection);
                }
            } else {
                self.value = None;
            }
        }
        let now = Instant::now();
        if self.pending.is_none() && self.next_poll.is_none_or(|deadline| now >= deadline) {
            self.next_poll = Some(now + Duration::from_secs(1));
            if let Some(directory) = directory {
                let (sender, receiver) = mpsc::sync_channel(1);
                self.pending = Some(Mutex::new(receiver));
                std::thread::spawn(move || {
                    let result = tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                        .ok()
                        .and_then(|runtime| {
                            runtime.block_on(async {
                                tokio::time::timeout(
                                    Duration::from_secs(2),
                                    bridge::read_status(&directory),
                                )
                                .await
                                .ok()
                                .and_then(Result::ok)
                                .and_then(|status| status.network)
                            })
                        });
                    let _ = sender.send((session, result));
                });
            } else {
                self.value = None;
            }
        }
        self.value
    }
    /// Rejects stale attempts and mismatched local actors; absence means unknown, never zero.
    fn accept(
        &mut self,
        runtime_id: u64,
        connection: Option<bridge::NetworkConnection>,
    ) -> Option<u32> {
        let connection = connection?;
        if connection.local_runtime_id != runtime_id || connection.attempt_id < self.attempt {
            return None;
        }
        self.attempt = connection.attempt_id;
        connection.round_trip_ms
    }
}

/// A borrowed actor ordered by distance, then runtime identity, with the furthest first in a heap.
struct NearbyActor<'a> {
    distance: f32,
    actor: &'a client_world::ActorSnapshot,
}

impl PartialEq for NearbyActor<'_> {
    /// Uses the same distance and runtime identity order as selection.
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for NearbyActor<'_> {}
impl PartialOrd for NearbyActor<'_> {
    /// Provides a total order for heap comparisons.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for NearbyActor<'_> {
    /// Breaks equal-distance ties by runtime identity, independent of arrival order.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then(self.actor.runtime_id.cmp(&other.actor.runtime_id))
    }
}

/// Returns the nearest in-range actors in order, retaining only the observation limit.
fn nearest_observed_actors<'a>(
    actors: impl Iterator<Item = &'a client_world::ActorSnapshot>,
    origin: [f32; 3],
) -> impl ExactSizeIterator<Item = &'a client_world::ActorSnapshot> {
    let mut nearby = BinaryHeap::new();
    for actor in actors {
        let distance = actor
            .position
            .iter()
            .zip(origin)
            .map(|(a, b)| (a - b).powi(2))
            .sum::<f32>();
        if !distance.is_finite() || distance > 128. * 128. {
            continue;
        }
        let candidate = NearbyActor { distance, actor };
        if nearby.len() < mod_api::MAX_OBSERVED_ACTORS {
            if nearby.is_empty() {
                nearby.reserve_exact(mod_api::MAX_OBSERVED_ACTORS);
            }
            nearby.push(candidate);
        } else if nearby.peek().is_some_and(|furthest| candidate < *furthest) {
            *nearby.peek_mut().expect("nonempty bounded heap") = candidate;
        }
    }
    nearby
        .into_sorted_vec()
        .into_iter()
        .map(|candidate| candidate.actor)
}

impl ModuleObserver {
    /// Appends retained facts with a monotonic cursor, allowing independent incremental consumers.
    fn push(&mut self, mut event: CombatObservation) {
        let Some(sequence) = self.sequence.checked_add(1) else {
            return;
        };
        self.sequence = sequence;
        event.sequence = sequence;
        if self.history.len() == mod_api::MAX_OBSERVED_COMBAT_EVENTS {
            self.history.pop_front();
        }
        self.history.push_back(event);
    }
    /// Observes native owners without depending on gameplay input capture.
    pub(super) fn capture(
        &mut self,
        gameplay: &GameplayContext<'_>,
        ui: &UiRuntime,
        world: Option<&WorldObservation>,
        player: &player_state::PlayerState,
        interaction: Option<&super::interaction::ModInteraction>,
    ) -> ModuleObservations {
        let Some(world) = world else {
            self.scope = None;
            self.history.clear();
            self.ping.sample(0, 0, None);
            return ModuleObservations::default();
        };
        let Some(authority) = gameplay
            .world
            .as_ref()
            .and_then(|world| world.stream.as_ref())
            .map(|stream| stream.authority())
        else {
            return ModuleObservations::default();
        };
        let scope = (world.session, world.dimension);
        if self.scope != Some(scope) {
            self.scope = Some(scope);
            self.history.clear();
            // Old dimension/session evidence must not replay into the newly opened scope.
            self.server_revision = authority
                .combat_evidence()
                .last()
                .map_or(0, |event| event.revision);
            self.attack_revision = gameplay
                .melee
                .as_ref()
                .and_then(|melee| melee.admitted_attack())
                .map_or(0, |attack| attack.sequence);
        }
        let motion = gameplay
            .physics
            .as_ref()
            .and_then(|physics| physics.state())
            .zip(gameplay.view.as_ref())
            .map(|(state, view)| {
                let (yaw, pitch, _) = view.rotation().to_euler(bevy::math::EulerRot::YXZ);
                MotionObservation {
                    session: world.session,
                    dimension: world.dimension,
                    yaw: yaw.to_degrees(),
                    pitch: pitch.to_degrees(),
                    velocity: (
                        state.velocity.x as f32,
                        state.velocity.y as f32,
                        state.velocity.z as f32,
                    ),
                    on_ground: state.on_ground,
                }
            });
        let hunger = ui.hud().hunger();
        let vitals = Some(VitalsObservation {
            session: world.session,
            dimension: world.dimension,
            hunger: hunger.map(|stat| stat.current() as f32),
            max_hunger: hunger.map(|stat| stat.maximum() as f32),
            saturation: ui.gameplay_hud().saturation(),
            held_food: player
                .selected_stack()
                .and_then(|stack| authority.canonical_item_stack(stack))
                .and_then(|stack| stack.identifier)
                .and_then(|identifier| ui.food_properties(&identifier))
                .map(|food| mod_host::FoodObservation {
                    nutrition: food.nutrition(),
                    saturation_modifier: food.saturation_modifier(),
                }),
        });
        let origin = [world.position.0, world.position.1, world.position.2];
        let target = current_target(gameplay, ui, interaction);
        let actors = nearest_observed_actors(authority.remote_actors(), origin)
            .map(|actor| {
                let position = actor
                    .interpolated_position(
                        gameplay.actor_partial.as_ref().map_or(0., |tick| tick.0),
                    )
                    .unwrap_or(actor.position);
                let name = authority
                    .actor_name_tag(actor.unique_id)
                    .or_else(|| authority.actor_display_name(actor.unique_id));
                let name = name.as_deref().unwrap_or("");
                let mut end = name.len().min(mod_api::MAX_OBSERVED_ACTOR_NAME_BYTES);
                while !name.is_char_boundary(end) {
                    end -= 1;
                }
                ObservedActor {
                    actor: ActorObservation {
                        session: world.session,
                        dimension: world.dimension,
                        identifier: match &actor.kind {
                            protocol::ActorKind::Player { .. } => {
                                assets::facts::entity::NativeEntityClass::Player
                                    .identifier()
                                    .expect("pinned player identity")
                                    .into()
                            }
                            protocol::ActorKind::Entity { identifier } => bounded_text(identifier),
                        },
                        yaw: actor.yaw,
                        pitch: actor.pitch,
                        pickable: gameplay::melee::actor_is_pickable(actor),
                        hittable: target.map(|target| target == Some(actor.runtime_id)),
                        look_origin: look_origin(authority, actor, position)
                            .map(|point| (point[0], point[1], point[2])),
                        look_direction: {
                            let yaw = actor.head_yaw.to_radians();
                            let pitch = actor.pitch.to_radians();
                            Some((
                                -yaw.sin() * pitch.cos(),
                                -pitch.sin(),
                                yaw.cos() * pitch.cos(),
                            ))
                        },
                        runtime_id: actor.runtime_id,
                        player: matches!(actor.kind, protocol::ActorKind::Player { .. }),
                        position: (position[0], position[1], position[2]),
                        boxes: actor
                            .hit_boxes_at(position)
                            .take(mod_api::MAX_OBSERVED_ACTOR_BOXES)
                            .map(|(min, max)| ActorObservationBounds {
                                min: (min[0], min[1], min[2]),
                                max: (max[0], max[1], max[2]),
                            })
                            .collect(),
                        name: name[..end]
                            .chars()
                            .filter(|character| !character.is_control())
                            .collect(),
                        invisible: actor.is_invisible(),
                        sneaking: actor.is_sneaking(),
                        hurt: actor.hurt_overlay_active(),
                        health: authority
                            .actor_health_by_unique(actor.unique_id)
                            .map(|(current, _)| current),
                    },
                    max_health: authority
                        .actor_health_by_unique(actor.unique_id)
                        .map(|(_, maximum)| maximum)
                        .filter(|maximum| maximum.is_finite() && *maximum > 0.),
                }
            })
            .collect();
        if let Some(attack) = gameplay
            .melee
            .as_ref()
            .and_then(|melee| melee.admitted_attack())
            .filter(|attack| attack.sequence > self.attack_revision)
        {
            self.attack_revision = attack.sequence;
            self.push(CombatObservation {
                sequence: 0,
                session: world.session,
                dimension: world.dimension,
                kind: CombatKind::AttackAdmitted,
                local: false,
                player: authority
                    .actor(attack.hit.runtime_id)
                    .is_some_and(|actor| matches!(actor.kind, protocol::ActorKind::Player { .. })),
                runtime_id: attack.hit.runtime_id,
                distance: Some(attack.hit.distance as f32),
                position: (
                    attack.hit.point[0],
                    attack.hit.point[1],
                    attack.hit.point[2],
                ),
            });
        }
        for event in authority
            .combat_evidence()
            .filter(|event| event.revision > self.server_revision)
            .copied()
            .collect::<Vec<_>>()
        {
            self.server_revision = event.revision;
            if event.dimension == world.dimension {
                self.push(CombatObservation {
                    sequence: 0,
                    session: world.session,
                    dimension: world.dimension,
                    kind: match event.kind {
                        client_world::ActorCombatKind::Hurt => CombatKind::Hurt,
                        client_world::ActorCombatKind::Death => CombatKind::Death,
                    },
                    player: event.player,
                    local: event.runtime_id == authority.local_player_runtime_id(),
                    runtime_id: event.runtime_id,
                    distance: None,
                    position: (event.position[0], event.position[1], event.position[2]),
                });
            }
        }
        let ping_ms = self.ping.sample(
            world.session,
            authority.local_player_runtime_id(),
            gameplay
                .session
                .as_ref()
                .and_then(|session| session.socket_directory())
                .map(PathBuf::from),
        );
        ModuleObservations {
            world_day: gameplay.world_day(world),
            local_totem_uses: authority.totem_activations(authority.local_player_runtime_id()),
            motion,
            vitals,
            ping_ms,
            actors,
            combat: self.history.iter().cloned().collect(),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    /// Creates a packet-admitted player snapshot without local carriers.
    fn actor_template() -> client_world::ActorSnapshot {
        let mut stream = chunk_pipeline::WorldStream::new(protocol::WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: false,
        });
        stream
            .submit(
                1,
                protocol::WorldEvent::Actor(protocol::ActorEvent::Spawn(
                    protocol::ActorSpawnEvent {
                        dimension: 0,
                        unique_id: 2,
                        runtime_id: 2,
                        kind: protocol::ActorKind::Player {
                            uuid: [0; 16],
                            username: "selection-fixture".into(),
                        },
                        position: [0.; 3],
                        velocity: [0.; 3],
                        pitch: 0.,
                        yaw: 0.,
                        head_yaw: 0.,
                        body_yaw: 0.,
                        held_item: Default::default(),
                        metadata: [].into(),
                        attributes: [].into(),
                        properties: [].into(),
                        links: [].into(),
                    },
                )),
            )
            .unwrap();
        stream.authority().remote_actors().next().unwrap().clone()
    }

    /// Retains the full-sort selection as a behavior and allocation reference.
    fn full_sort_nearest<'a>(
        actors: impl Iterator<Item = &'a client_world::ActorSnapshot>,
        origin: [f32; 3],
    ) -> impl ExactSizeIterator<Item = &'a client_world::ActorSnapshot> {
        let mut nearby: Vec<_> = actors
            .filter_map(|actor| {
                let distance = actor
                    .position
                    .iter()
                    .zip(origin)
                    .map(|(a, b)| (a - b).powi(2))
                    .sum::<f32>();
                (distance.is_finite() && distance <= 128. * 128.).then_some((distance, actor))
            })
            .collect();
        nearby.sort_by(|a, b| {
            a.0.total_cmp(&b.0)
                .then(a.1.runtime_id.cmp(&b.1.runtime_id))
        });
        nearby
            .into_iter()
            .take(mod_api::MAX_OBSERVED_ACTORS)
            .map(|(_, actor)| actor)
    }

    /// Makes a repeatable shuffled crowd with distance ties and unique runtime identities.
    fn crowd(
        template: &client_world::ActorSnapshot,
        count: usize,
    ) -> Vec<client_world::ActorSnapshot> {
        let mut state = 7_u32;
        let mut actors: Vec<_> = (0..count)
            .map(|index| {
                let mut actor = template.clone();
                actor.runtime_id = index as u64;
                actor.position = [(index % 127) as f32, 0., 0.];
                actor
            })
            .collect();
        for index in (1..count).rev() {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            actors.swap(index, state as usize % (index + 1));
        }
        actors
    }

    /// Checks identities and borrowed payloads against the previous full-sort behavior.
    fn assert_selection_matches(actors: &[client_world::ActorSnapshot], origin: [f32; 3]) {
        let expected: Vec<_> = full_sort_nearest(actors.iter(), origin).collect();
        let actual: Vec<_> = nearest_observed_actors(actors.iter(), origin).collect();
        assert_eq!(actual.len(), expected.len());
        for (actual, expected) in actual.into_iter().zip(expected) {
            assert!(std::ptr::eq(actual, expected));
        }
    }

    #[test]
    fn nearest_observations_match_full_sort_for_crowds_and_input_orders() {
        let template = actor_template();
        for count in [
            0,
            1,
            mod_api::MAX_OBSERVED_ACTORS - 1,
            mod_api::MAX_OBSERVED_ACTORS,
            100,
            1000,
        ] {
            let mut actors = crowd(&template, count);
            for origin in [[0.; 3], [10., -2., 4.], [128., 0., 0.]] {
                assert_selection_matches(&actors, origin);
                actors.reverse();
                assert_selection_matches(&actors, origin);
                actors.sort_by_key(|actor| actor.runtime_id);
                assert_selection_matches(&actors, origin);
                actors.reverse();
                assert_selection_matches(&actors, origin);
            }
        }
    }

    #[test]
    fn nearest_observations_replace_cutoff_ties_by_runtime_identity() {
        let template = actor_template();
        let mut actors = crowd(&template, mod_api::MAX_OBSERVED_ACTORS + 8);
        for actor in &mut actors {
            actor.position = [1., 0., 0.];
        }
        actors.sort_by_key(|actor| std::cmp::Reverse(actor.runtime_id));
        let ids: Vec<_> = nearest_observed_actors(actors.iter(), [0.; 3])
            .map(|actor| actor.runtime_id)
            .collect();
        assert_eq!(
            ids,
            (0..mod_api::MAX_OBSERVED_ACTORS as u64).collect::<Vec<_>>()
        );
        actors[0].position = [0.; 3];
        assert_eq!(
            nearest_observed_actors(actors.iter(), [0.; 3])
                .next()
                .unwrap()
                .runtime_id,
            actors[0].runtime_id
        );
        assert_selection_matches(&actors, [0.; 3]);
    }

    #[test]
    fn nearest_observations_preserve_range_and_invalid_distance_handling() {
        let template = actor_template();
        let positions = [
            [0.; 3],
            [128., 0., 0.],
            [-128., 0., 0.],
            [f32::from_bits(128_f32.to_bits() + 1), 0., 0.],
            [128., 1., 0.],
            [f32::NAN, 0., 0.],
            [0., f32::INFINITY, 0.],
            [0., 0., f32::NEG_INFINITY],
            [f32::MAX, 0., 0.],
        ];
        let actors: Vec<_> = positions
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                let mut actor = template.clone();
                actor.runtime_id = index as u64;
                actor.position = position;
                actor
            })
            .collect();
        let ids: Vec<_> = nearest_observed_actors(actors.iter(), [0.; 3])
            .map(|actor| actor.runtime_id)
            .collect();
        assert_eq!(ids, [0, 1, 2]);
        for origin in [
            [0.; 3],
            [f32::NAN, 0., 0.],
            [f32::INFINITY, 0., 0.],
            [f32::MAX; 3],
        ] {
            assert_selection_matches(&actors, origin);
        }
    }

    #[test]
    fn nearest_observations_do_not_allocate_without_eligible_actors() {
        let (empty, allocations) =
            test_alloc::measure(|| nearest_observed_actors(std::iter::empty(), [0.; 3]));
        assert_eq!(empty.len(), 0);
        assert_eq!(allocations.calls, 0);
        let mut actor = actor_template();
        for position in [[129., 0., 0.], [f32::NAN, 0., 0.], [f32::MAX; 3]] {
            actor.position = position;
            let (empty, allocations) =
                test_alloc::measure(|| nearest_observed_actors(std::iter::once(&actor), [0.; 3]));
            assert_eq!(empty.len(), 0);
            assert_eq!(allocations.calls, 0);
        }
    }

    #[test]
    fn nearest_observations_allocation_benchmark() {
        let template = actor_template();
        let mut bounded_bytes = None;
        for count in [16, 100, 1000] {
            let actors = crowd(&template, count);
            let (old, before) = test_alloc::measure(|| full_sort_nearest(actors.iter(), [0.; 3]));
            let (new, after) =
                test_alloc::measure(|| nearest_observed_actors(actors.iter(), [0.; 3]));
            assert!(old.zip(new).all(|(old, new)| std::ptr::eq(old, new)));
            assert!(after.calls > 0, "allocation counter must be active");
            assert!(after.bytes <= before.bytes);
            if let Some(bytes) = bounded_bytes {
                assert_eq!(
                    after.bytes, bytes,
                    "selection storage must not grow with the crowd"
                );
            }
            bounded_bytes = Some(after.bytes);
            if count > mod_api::MAX_OBSERVED_ACTORS {
                assert!(after.bytes < before.bytes);
            }
            eprintln!("nearest actors={count}: before={before:?} after={after:?}");
        }
    }

    #[test]
    fn ping_keeps_unknown_distinct_and_rejects_old_connections() {
        let mut poll = PingPoll::default();
        let connection = |attempt_id, local_runtime_id, round_trip_ms| bridge::NetworkConnection {
            attempt_id,
            local_runtime_id,
            round_trip_ms,
        };
        assert_eq!(poll.accept(7, Some(connection(2, 7, Some(31)))), Some(31));
        assert_eq!(poll.accept(7, Some(connection(1, 7, Some(99)))), None);
        assert_eq!(poll.accept(7, Some(connection(3, 8, Some(99)))), None);
        assert_eq!(poll.accept(7, Some(connection(3, 7, None))), None);
        assert_eq!(poll.sample(5, 7, None), None);
    }
}
