//! App-owned snapshot and camera adapter; the component host has no world access.

use bevy::{ecs::system::SystemParam, prelude::*};
use client_presentation::{
    camera::{AutoFly, PITCH_LIMIT, ServerCameraView},
    local_player::LocalViewPose,
};
use mod_host::{CameraDelta, GameplayPlayer, GameplaySnapshot, GameplayVector3, ModGrants};

use crate::{runtime::world::ClientWorld, semantic_controls::SemanticInputSnapshot};

#[derive(SystemParam)]
pub(super) struct GameplayContext<'w> {
    world: Option<Res<'w, ClientWorld>>,
    view: Option<ResMut<'w, LocalViewPose>>,
    input: Option<Res<'w, SemanticInputSnapshot>>,
    auto_fly: Option<Res<'w, AutoFly>>,
    server_camera: Option<Res<'w, ServerCameraView>>,
    time: Option<Res<'w, Time>>,
}

impl GameplayContext<'_> {
    /// No snapshot exists outside captured gameplay or without explicit grants.
    pub(super) fn snapshot(&self, allowed: bool, grants: ModGrants) -> Option<GameplaySnapshot> {
        if !allowed
            || !(grants.players || grants.camera)
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
            attack_held: self
                .input
                .as_ref()
                .is_some_and(|input| input.phase(semantic_input::Action::Attack).held),
            players,
        })
    }

    /// Called only for a successfully committed, once-consumed current-frame delta.
    pub(super) fn apply(&mut self, delta: CameraDelta) {
        if let Some(view) = self.view.as_mut() {
            apply_delta(view, delta);
        }
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
