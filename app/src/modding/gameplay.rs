//! App-owned snapshot and camera adapter; the component host has no world access.

use bevy::{ecs::system::SystemParam, prelude::*};
use client_presentation::{
    camera::{AutoFly, PITCH_LIMIT, ServerCameraView},
    local_player::LocalViewPose,
};
use mod_host::{
    CameraDelta, GameplayMob, GameplayMovementSnapshot, GameplayPlayer, GameplaySnapshot,
    GameplayVector3, ModGrants,
};

use crate::{runtime::world::ClientWorld, semantic_controls::SemanticInputSnapshot};

#[derive(SystemParam)]
pub(super) struct GameplayContext<'w> {
    world: Option<Res<'w, ClientWorld>>,
    view: Option<ResMut<'w, LocalViewPose>>,
    input: Option<ResMut<'w, SemanticInputSnapshot>>,
    auto_fly: Option<Res<'w, AutoFly>>,
    server_camera: Option<Res<'w, ServerCameraView>>,
    time: Option<Res<'w, Time>>,
    real_time: Option<Res<'w, Time<Real>>>,
    clock: Option<Res<'w, crate::environment::WorldClock>>,
    physics: Option<ResMut<'w, crate::movement::LocalPhysicsController>>,
    movement: Option<Res<'w, crate::movement::MovementTicker>>,
    player: Option<Res<'w, crate::player_runtime::PlayerRuntime>>,
    ui: Option<Res<'w, client_ui::ui_runtime::UiRuntime>>,
}

impl GameplayContext<'_> {
    /// Layout editing requires the same current connected world as player presentation.
    pub(super) fn hud_editor_session(&self, ui: &client_ui::ui_runtime::UiRuntime) -> Option<u64> {
        self.world.as_ref()?.stream.as_ref()?;
        let session = self.clock.as_ref()?.session_generation();
        (session == ui.session_id()).then_some(session)
    }

    /// Local facts stay readable while a screen owns input, but never without a live session.
    pub(super) fn player_state(
        &self,
        enabled: bool,
        player: &player_state::PlayerState,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) -> Option<mod_host::PlayerStateSnapshot> {
        if !enabled {
            return None;
        }
        let authority = self.world.as_ref()?.stream.as_ref()?.authority();
        let session = self.clock.as_ref()?.session_generation();
        // Effect event anchors use real elapsed time; virtual time may pause or clamp long frames.
        let now_millis = self.real_time.as_ref().map_or(0, |time| {
            u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX)
        });
        super::player_state::snapshot(authority, session, player, ui, now_millis)
    }

    /// Returns the current walking authority only while local movement can accept input.
    fn movement_scope(&self) -> Option<(u64, i32)> {
        self.input.as_ref()?.snapshot()?;
        let world = self.world.as_ref()?;
        if world.dimension_transfer.active()
            || world.respawn.input_held()
            || !self.movement.as_ref()?.physics_is_authorized()
            || self.auto_fly.as_ref().is_some_and(|auto| auto.enabled())
            || self
                .server_camera
                .as_ref()
                .is_some_and(|camera| camera.is_active())
            || self.ui.as_ref().is_some_and(|ui| {
                ui.hud()
                    .health()
                    .is_some_and(|health| health.current() == 0)
            })
        {
            return None;
        }
        let player = self.player.as_ref()?;
        let capabilities = player.facts.game_mode_capabilities();
        if player.facts.is_immobile()
            || player.facts.mount_unique_id().is_some()
            || capabilities.is_some_and(|abilities| abilities.flying)
            || !self.physics.as_ref()?.jump_pulse_eligible()
        {
            return None;
        }
        let authority = world.stream.as_ref()?.authority();
        Some((authority.actor_session_id(), authority.current_dimension()))
    }

    /// Revokes pending jump input when its gameplay ownership changes.
    pub(super) fn synchronize_jump_scope(&mut self, granted: bool) {
        let scope = granted.then(|| self.movement_scope()).flatten();
        if let Some(physics) = self.physics.as_mut() {
            physics.set_jump_pulse_scope(scope);
        }
    }

    /// Samples local movement only for a granted, captured gameplay callback.
    pub(super) fn movement_snapshot(
        &self,
        allowed: bool,
        grants: &ModGrants,
    ) -> Option<GameplayMovementSnapshot> {
        if !allowed || !grants.movement {
            return None;
        }
        let (session, dimension) = self.movement_scope()?;
        let physics = self.physics.as_ref()?;
        let state = physics.state()?;
        let velocity = state.velocity;
        Some(GameplayMovementSnapshot {
            session,
            dimension,
            tick: state.tick,
            velocity: GameplayVector3 {
                x: velocity.x as f32,
                y: velocity.y as f32,
                z: velocity.z as f32,
            },
            on_ground: state.on_ground,
            jump_held: self.input.as_ref()?.snapshot()?.phases
                [semantic_input::Action::Jump as usize]
                .held,
            eligible: true,
            knockback_sequence: physics.knockback_sequence(),
        })
    }

    /// Queues an ordinary jump while the current walking authority remains valid.
    pub(super) fn pulse_jump(&mut self, requested: bool) {
        if requested
            && self.movement_scope().is_some()
            && let Some(physics) = self.physics.as_mut()
        {
            physics.request_jump_pulse();
        }
    }

    /// No snapshot exists outside captured gameplay or without explicit grants.
    pub(super) fn snapshot(&self, allowed: bool, grants: &ModGrants) -> Option<GameplaySnapshot> {
        if !allowed
            || !(grants.players
                || grants.camera
                || grants.item_use
                || grants.movement
                || grants.interaction
                || grants.entities
                || !grants.commands.is_empty())
            || self
                .auto_fly
                .as_ref()
                .is_some_and(|auto| auto.controls_acceptance_camera())
            || self
                .server_camera
                .as_ref()
                .is_some_and(|camera| camera.is_active())
        {
            return None;
        }
        let input = self.input.as_ref()?.snapshot()?;
        let authority = self.world.as_ref()?.stream.as_ref()?.authority();
        let view = self.view.as_ref()?;
        let (yaw, pitch, _) = view.rotation().to_euler(EulerRot::YXZ);
        let eye = view.eye_translation();
        let players = if grants.players {
            nearest_players(authority.remote_actors(), eye)
        } else {
            Vec::new()
        };
        Some(GameplaySnapshot {
            session: authority.actor_session_id(),
            dimension: authority.current_dimension(),
            eye: vector(eye),
            yaw,
            pitch,
            frame_seconds: self
                .time
                .as_ref()
                .map_or(0.0, |time| time.delta_secs().clamp(0.0, 1.0)),
            attack_held: input.phases[semantic_input::Action::Attack as usize].held,
            players,
        })
    }

    /// Nearby mobs around the snapshot eye, only with the entities grant.
    pub(super) fn mobs(
        &self,
        snapshot: Option<&GameplaySnapshot>,
        grants: &ModGrants,
    ) -> Vec<GameplayMob> {
        let (Some(frame), true) = (snapshot, grants.entities) else {
            return Vec::new();
        };
        let Some(stream) = self.world.as_ref().and_then(|world| world.stream.as_ref()) else {
            return Vec::new();
        };
        let eye = Vec3::new(frame.eye.x, frame.eye.y, frame.eye.z);
        nearest_mobs(stream.authority().remote_actors(), eye)
    }

    /// Called only for a successfully committed, once-consumed current-frame delta.
    pub(super) fn apply(&mut self, delta: CameraDelta) {
        if let Some(view) = self.view.as_mut() {
            apply_delta(view, delta);
        }
    }

    pub(super) fn pulse_attack(&mut self) -> bool {
        self.input
            .as_mut()
            .is_some_and(|input| input.request_mod_attack_press())
    }
}

/// Reports protocol-classified remote players, never named mobs or list-only users.
fn nearest_players<'a>(
    actors: impl Iterator<Item = &'a client_world::ActorSnapshot>,
    eye: Vec3,
) -> Vec<GameplayPlayer> {
    let mut players: Vec<_> = actors
        .filter(|actor| matches!(actor.kind, protocol::ActorKind::Player { .. }))
        .filter(|actor| actor.runtime_id != 0 && Vec3::from_array(actor.position).is_finite())
        .map(|actor| GameplayPlayer {
            runtime_id: actor.runtime_id,
            position: vector(Vec3::from_array(actor.position)),
        })
        .collect();
    players.sort_by(|a, b| {
        distance_squared(a, eye)
            .total_cmp(&distance_squared(b, eye))
            .then_with(|| a.runtime_id.cmp(&b.runtime_id))
    });
    players.truncate(mod_host::MAX_GAMEPLAY_PLAYERS);
    players
}

/// Non-player actors within the published range, nearest first; health is the replicated attribute.
fn nearest_mobs<'a>(
    actors: impl Iterator<Item = &'a client_world::ActorSnapshot>,
    eye: Vec3,
) -> Vec<GameplayMob> {
    let range = mod_host::MAX_MOB_RANGE_BLOCKS * mod_host::MAX_MOB_RANGE_BLOCKS;
    let mut mobs: Vec<_> = actors
        .filter_map(|actor| {
            let protocol::ActorKind::Entity { identifier } = &actor.kind else {
                return None;
            };
            let position = Vec3::from_array(actor.position);
            if actor.runtime_id == 0
                || !position.is_finite()
                || position.distance_squared(eye) > range
                || identifier.is_empty()
                || identifier.len() > mod_host::MAX_MOB_TYPE_BYTES
            {
                return None;
            }
            let health = actor
                .attributes
                .get("minecraft:health")
                .filter(|health| health.current.is_finite() && health.max.is_finite());
            Some(GameplayMob {
                runtime_id: actor.runtime_id,
                unique_id: actor.unique_id,
                type_id: identifier.to_string(),
                position: vector(position),
                health: health.map(|health| health.current),
                max_health: health.map(|health| health.max),
            })
        })
        .collect();
    let distance = |mob: &GameplayMob| {
        Vec3::new(mob.position.x, mob.position.y, mob.position.z).distance_squared(eye)
    };
    mobs.sort_by(|a, b| {
        distance(a)
            .total_cmp(&distance(b))
            .then_with(|| a.runtime_id.cmp(&b.runtime_id))
    });
    mobs.truncate(mod_host::MAX_GAMEPLAY_MOBS);
    mobs
}

fn distance_squared(player: &GameplayPlayer, eye: Vec3) -> f32 {
    Vec3::new(player.position.x, player.position.y, player.position.z).distance_squared(eye)
}

fn vector(value: Vec3) -> GameplayVector3 {
    GameplayVector3 {
        x: value.x,
        y: value.y,
        z: value.z,
    }
}

/// Uses the existing actor pitch limit and preserves roll, position and ordinary input.
fn apply_delta(view: &mut LocalViewPose, delta: CameraDelta) {
    if delta == CameraDelta::default() {
        return;
    }
    let (yaw, pitch, roll) = view.rotation().to_euler(EulerRot::YXZ);
    let yaw = (yaw + delta.yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
        - std::f32::consts::PI;
    let pitch = (pitch + delta.pitch).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    view.set_rotation(Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll));
}

#[cfg(test)]
mod tests;
