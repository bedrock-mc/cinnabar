//! Server-authoritative player combat input and target selection.

use bevy::{
    ecs::system::SystemParam,
    prelude::{Res, ResMut, Resource},
};
use client_world::ActorSnapshot;
use protocol::{
    ActorKind, ActorMetadataValue, EntityInteractionAction, NetworkItemStack, Packet,
    PlayerGameMode, missed_swing_packet, use_item_on_entity_packet,
};
use semantic_input::Action;
use sim::{Aabb, CollisionWorld, PaletteWorld, Vec3};

use crate::{
    local_player::InteractionOriginSnapshot,
    movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    runtime::{
        network::{NetworkHandle, PacketSendError},
        shutdown::record_fatal_error,
        world::ClientWorld,
    },
    semantic_controls::SemanticInputSnapshot,
    ui_runtime::UiRuntime,
};

/// The normal player reach used by this bounded PvP lane. Game-mode-specific
/// reach extensions remain a native/live acceptance gate in the parity matrix.
const PLAYER_ENTITY_REACH: f64 = 3.0;
const RAY_EPSILON: f64 = 1.0e-6;
const OCCLUSION_EPSILON: f64 = 1.0e-5;
const QUERY_EPSILON: f64 = 1.0e-5;
const CROUCHING_PLAYER_HEIGHT: f64 = 1.5;
const SLEEPING_PLAYER_HEIGHT: f64 = 0.2;
const MAX_COMBAT_OUTBOX_AGE_FRAMES: u64 = 2;

#[derive(Debug)]
struct PendingCombatPacket {
    session_generation: u64,
    frame_sequence: u64,
    packet: Packet,
}

/// Bounded transport state for one-shot combat actions.
///
/// The resource never predicts damage or knockback. A packet that cannot be
/// admitted to the network command queue is retained briefly with its frozen
/// target snapshot, then discarded as stale rather than replayed indefinitely.
#[derive(Debug, Default, Resource)]
pub(crate) struct CombatRuntime {
    session_generation: u64,
    pending: Option<PendingCombatPacket>,
    dropped_stale: u64,
    dropped_backpressure: u64,
    unsupported_use_presses: u64,
}

enum SendOutcome {
    Sent,
    Deferred,
    Closed,
}

#[derive(SystemParam)]
pub(crate) struct CombatFrame<'w> {
    input: Res<'w, SemanticInputSnapshot>,
    interaction: Res<'w, InteractionOriginSnapshot>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    local_physics: Res<'w, LocalPhysicsController>,
    runtime: Res<'w, UiRuntime>,
    client_world: ResMut<'w, ClientWorld>,
    network: Res<'w, NetworkHandle>,
    combat: ResMut<'w, CombatRuntime>,
}

/// Samples gameplay attack/use edges after the physics-owned interaction ray
/// has been published and sends only server-authoritative transactions.
pub(crate) fn send_combat_inputs(mut frame: CombatFrame) {
    let session_generation = frame.runtime.session_id();
    frame.combat.begin_session(session_generation);
    if session_generation == 0 || frame.runtime.ui_focused() {
        frame.combat.invalidate_pending();
        return;
    }

    let Some(input_snapshot) = frame.input.snapshot() else {
        frame.combat.invalidate_pending();
        return;
    };
    let frame_sequence = input_snapshot.frame_sequence;
    match frame
        .combat
        .flush_pending(&frame.network, session_generation, frame_sequence)
    {
        SendOutcome::Sent => {}
        SendOutcome::Deferred => return,
        SendOutcome::Closed => {
            record_fatal_error(
                &mut frame.client_world.fatal_error,
                "combat send failed because the network command channel closed".to_owned(),
            );
            return;
        }
    }

    let Some(game_mode) = frame.runtime.player_game_mode() else {
        return;
    };
    if matches!(
        game_mode,
        PlayerGameMode::Spectator | PlayerGameMode::Unknown
    ) {
        return;
    }
    let Some(ray) = frame.interaction.outbound_ray().filter(|ray| {
        ray.session_generation() == session_generation && ray.direction().is_finite()
    }) else {
        return;
    };
    let Some(stream) = frame
        .client_world
        .stream
        .as_ref()
        .filter(|stream| stream.actor_session_id() == session_generation)
    else {
        return;
    };
    let Some(player_state) = frame.local_physics.state() else {
        return;
    };
    if !player_state.position.is_finite() {
        return;
    }

    let attack_pressed = frame.input.phase(Action::Attack).pressed;
    let use_pressed = frame.input.phase(Action::Use).pressed;
    if !attack_pressed && !use_pressed {
        return;
    }

    let target = select_player_target(ray, stream, &frame.collisions);
    if let Some((actor, hit_distance)) = target {
        let Some(slot) = frame.runtime.selected_hotbar_slot() else {
            return;
        };
        let held_item = frame
            .runtime
            .selected_stack()
            .cloned()
            .unwrap_or_else(NetworkItemStack::empty);
        let click_position = click_position(ray, actor, hit_distance);
        let action = if attack_pressed {
            EntityInteractionAction::Attack
        } else {
            EntityInteractionAction::Interact
        };
        let Ok(packet) = use_item_on_entity_packet(
            actor.runtime_id,
            action,
            slot,
            &held_item,
            [
                player_state.position.x as f32,
                player_state.position.y as f32,
                player_state.position.z as f32,
            ],
            click_position,
        ) else {
            return;
        };
        match frame
            .combat
            .admit(&frame.network, session_generation, frame_sequence, packet)
        {
            SendOutcome::Sent | SendOutcome::Deferred => {}
            SendOutcome::Closed => record_fatal_error(
                &mut frame.client_world.fatal_error,
                "combat send failed because the network command channel closed".to_owned(),
            ),
        }
        return;
    }

    if attack_pressed {
        let Some(runtime_id) = frame.runtime.local_runtime_id() else {
            return;
        };
        let Ok(packet) = missed_swing_packet(runtime_id) else {
            return;
        };
        match frame
            .combat
            .admit(&frame.network, session_generation, frame_sequence, packet)
        {
            SendOutcome::Sent | SendOutcome::Deferred => {}
            SendOutcome::Closed => record_fatal_error(
                &mut frame.client_world.fatal_error,
                "combat send failed because the network command channel closed".to_owned(),
            ),
        }
    } else if use_pressed {
        // Entity interaction is implemented here; block/item use needs its
        // own transaction target and is intentionally not fabricated from a
        // miss because that would change server semantics.
        frame.combat.unsupported_use_presses =
            frame.combat.unsupported_use_presses.saturating_add(1);
    }
}

impl CombatRuntime {
    fn begin_session(&mut self, session_generation: u64) {
        if self.session_generation == session_generation {
            return;
        }
        self.session_generation = session_generation;
        self.pending = None;
    }

    fn invalidate_pending(&mut self) {
        self.pending = None;
    }

    fn flush_pending(
        &mut self,
        network: &NetworkHandle,
        session_generation: u64,
        frame_sequence: u64,
    ) -> SendOutcome {
        let Some(pending) = self.pending.take() else {
            return SendOutcome::Sent;
        };
        if pending.session_generation != session_generation
            || frame_sequence.saturating_sub(pending.frame_sequence) > MAX_COMBAT_OUTBOX_AGE_FRAMES
        {
            self.dropped_stale = self.dropped_stale.saturating_add(1);
            return SendOutcome::Sent;
        }
        self.admit(
            network,
            session_generation,
            pending.frame_sequence,
            pending.packet,
        )
    }

    fn admit(
        &mut self,
        network: &NetworkHandle,
        session_generation: u64,
        frame_sequence: u64,
        packet: Packet,
    ) -> SendOutcome {
        match network.send_combat_packet(packet) {
            Ok(()) => SendOutcome::Sent,
            Err(PacketSendError::Full(packet)) => {
                if self.pending.is_none() {
                    self.pending = Some(PendingCombatPacket {
                        session_generation,
                        frame_sequence,
                        packet,
                    });
                } else {
                    self.dropped_backpressure = self.dropped_backpressure.saturating_add(1);
                }
                SendOutcome::Deferred
            }
            Err(PacketSendError::Closed(_)) => SendOutcome::Closed,
        }
    }
}

fn select_player_target<'a>(
    ray: &crate::local_player::FrozenInteractionOrigin,
    stream: &'a client_world::WorldStream,
    collisions: &PhysicsCollisionRegistries,
) -> Option<(&'a ActorSnapshot, f64)> {
    let origin = Vec3::new(
        f64::from(ray.origin().x),
        f64::from(ray.origin().y),
        f64::from(ray.origin().z),
    );
    let direction = Vec3::new(
        f64::from(ray.direction().x),
        f64::from(ray.direction().y),
        f64::from(ray.direction().z),
    );
    if !origin.is_finite() || !direction.is_finite() {
        return None;
    }
    let length_squared = direction.length_squared();
    if !length_squared.is_finite() || length_squared <= RAY_EPSILON {
        return None;
    }
    let direction = direction * (1.0 / length_squared.sqrt());
    let end = origin + direction * PLAYER_ENTITY_REACH;
    let query = Aabb::new(
        origin.component_min(end) - Vec3::new(QUERY_EPSILON, QUERY_EPSILON, QUERY_EPSILON),
        origin.component_max(end) + Vec3::new(QUERY_EPSILON, QUERY_EPSILON, QUERY_EPSILON),
    );
    let palette = PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
    );
    let collision_query = palette.collision_boxes(query).ok()?;
    if collision_query.identity != *ray.world_collision_identity() {
        return None;
    }
    let nearest_solid = collision_query
        .value
        .iter()
        .filter_map(|block| ray_aabb_hit(origin, direction, PLAYER_ENTITY_REACH, *block))
        .min_by(f64::total_cmp);

    stream
        .render_players()
        .into_iter()
        .filter_map(|(actor, _)| {
            if !matches!(&actor.kind, ActorKind::Player { .. }) {
                return None;
            }
            let hitbox = player_hitbox(actor)?;
            let hit_distance = ray_aabb_hit(origin, direction, PLAYER_ENTITY_REACH, hitbox)?;
            if nearest_solid.is_some_and(|solid| solid + OCCLUSION_EPSILON < hit_distance) {
                return None;
            }
            Some((actor, hit_distance))
        })
        .min_by(|(left, left_distance), (right, right_distance)| {
            left_distance
                .total_cmp(right_distance)
                .then_with(|| left.runtime_id.cmp(&right.runtime_id))
        })
}

fn player_hitbox(actor: &ActorSnapshot) -> Option<Aabb> {
    if !actor.position.into_iter().all(f32::is_finite) {
        return None;
    }
    let feet = Vec3::new(
        f64::from(actor.position[0]),
        f64::from(actor.position[1]),
        f64::from(actor.position[2]),
    );
    let height = if is_sleeping(actor) {
        SLEEPING_PLAYER_HEIGHT
    } else if is_sneaking(actor) {
        CROUCHING_PLAYER_HEIGHT
    } else {
        sim::PLAYER_HEIGHT
    };
    let half_width = sim::PLAYER_WIDTH * 0.5 - sim::PLAYER_HORIZONTAL_EPSILON;
    Some(Aabb::new(
        Vec3::new(feet.x - half_width, feet.y, feet.z - half_width),
        Vec3::new(feet.x + half_width, feet.y + height, feet.z + half_width),
    ))
}

fn is_sneaking(actor: &ActorSnapshot) -> bool {
    matches!(
        actor.metadata.get(&0),
        Some(ActorMetadataValue::Flags(flags)) if flags & (1 << 1) != 0
    )
}

fn is_sleeping(actor: &ActorSnapshot) -> bool {
    matches!(
        actor.metadata.get(&26),
        Some(ActorMetadataValue::Byte(flags)) if (*flags as u8) & (1 << 1) != 0
    ) || matches!(
        actor.metadata.get(&92),
        Some(ActorMetadataValue::FlagsExtended(flags)) if flags & (1 << 11) != 0
    )
}

fn ray_aabb_hit(origin: Vec3, direction: Vec3, max_distance: f64, aabb: Aabb) -> Option<f64> {
    let mut near: f64 = 0.0;
    let mut far: f64 = max_distance;
    for axis in 0..3 {
        let start = origin[axis];
        let delta = direction[axis];
        let min = aabb.min[axis];
        let max = aabb.max[axis];
        if delta.abs() <= RAY_EPSILON {
            if start < min || start > max {
                return None;
            }
            continue;
        }
        let inverse = 1.0 / delta;
        let mut axis_near = (min - start) * inverse;
        let mut axis_far = (max - start) * inverse;
        if axis_near > axis_far {
            std::mem::swap(&mut axis_near, &mut axis_far);
        }
        near = near.max(axis_near);
        far = far.min(axis_far);
        if near > far {
            return None;
        }
    }
    (near <= max_distance && far >= 0.0).then_some(near.max(0.0))
}

fn click_position(
    ray: &crate::local_player::FrozenInteractionOrigin,
    actor: &ActorSnapshot,
    hit_distance: f64,
) -> [f32; 3] {
    let direction = Vec3::new(
        f64::from(ray.direction().x),
        f64::from(ray.direction().y),
        f64::from(ray.direction().z),
    );
    let direction = direction * (1.0 / direction.length_squared().sqrt());
    let point = Vec3::new(
        f64::from(ray.origin().x),
        f64::from(ray.origin().y),
        f64::from(ray.origin().z),
    ) + direction * hit_distance;
    [
        (point.x - f64::from(actor.position[0])) as f32,
        (point.y - f64::from(actor.position[1])) as f32,
        (point.z - f64::from(actor.position[2])) as f32,
    ]
}
