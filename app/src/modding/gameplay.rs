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

use {crate::runtime::world::ClientWorld, client_state::SemanticInputSnapshot};

#[derive(SystemParam)]
pub(super) struct GameplayContext<'w> {
    pub(super) world: Option<Res<'w, ClientWorld>>,
    pub(super) view: Option<ResMut<'w, LocalViewPose>>,
    pub(super) input: Option<ResMut<'w, SemanticInputSnapshot>>,
    auto_fly: Option<Res<'w, AutoFly>>,
    server_camera: Option<Res<'w, ServerCameraView>>,
    time: Option<Res<'w, Time>>,
    real_time: Option<Res<'w, Time<Real>>>,
    clock: Option<Res<'w, client_presentation::environment::WorldClock>>,
    pub(super) physics: Option<ResMut<'w, client_interaction::movement::LocalPhysicsController>>,
    pub(super) movement: Option<Res<'w, client_interaction::movement::MovementTicker>>,
    pub(super) player: Option<Res<'w, client_state::PlayerRuntime>>,
    pub(super) origin:
        Option<Res<'w, client_presentation::local_player::InteractionOriginSnapshot>>,
    pub(super) collisions:
        Option<Res<'w, client_interaction::movement::PhysicsCollisionRegistries>>,
    pub(super) aim: Option<Res<'w, client_presentation::aim_assist::AimAssistFrame>>,
    pub(super) actor_partial:
        Option<Res<'w, client_presentation::actor_publication::ActorFramePartialTick>>,
    pub(super) melee: Option<Res<'w, client_interaction::melee::MeleeRuntime>>,
    pub(super) session: Option<Res<'w, crate::session::SessionController>>,
}

const _: () = assert!(mod_api::BEDROCK_TICKS_PER_SECOND == simulation_clock::TICKS_PER_SECOND);

impl GameplayContext<'_> {
    /// Reads the real server clock in this scope; visual time overrides never enter this observation.
    pub(super) fn world_day(
        &self,
        world: &mod_host::WorldObservation,
    ) -> Option<mod_host::module_observations::WorldDay> {
        let clock = self.clock.as_deref()?;
        if world.session == 0 || clock.session_generation() != world.session {
            return None;
        }
        Some(mod_host::module_observations::WorldDay {
            session: world.session,
            dimension: world.dimension,
            day: clock.world_day(self.time.as_ref()?.elapsed_secs_f64())?,
        })
    }

    /// Native packet frames share the current input snapshot and connected-world fences.
    pub(super) fn native_frame(
        &self,
        allowed: bool,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) -> Option<mod_api::native_frame::Frame> {
        let world = self.world.as_ref()?;
        if world.dimension_transfer.active()
            || world.respawn.input_held()
            || self.clock.as_ref()?.session_generation() != ui.session_id()
            || ui
                .hud()
                .health()
                .is_some_and(|health| health.current() == 0)
        {
            return None;
        }
        let grants = ModGrants {
            players: true,
            camera: true,
            ..Default::default()
        };
        let Some(snapshot) = self.snapshot(allowed, &grants) else {
            let eye = self.view.as_ref()?.eye_translation().to_array();
            return Some(mod_api::native_frame::Frame {
                version: mod_api::native_frame::VERSION,
                session: world.stream.as_ref()?.authority().actor_session_id(),
                dimension: world.stream.as_ref()?.current_dimension(),
                eye,
                yaw: 0.0,
                pitch: 0.0,
                attack_held: false,
                seconds: self
                    .real_time
                    .as_ref()
                    .map_or(0.0, |time| time.delta_secs().min(1.0)),
                players: Vec::new(),
                suspended: true,
            });
        };
        Some(mod_api::native_frame::Frame {
            version: mod_api::native_frame::VERSION,
            session: snapshot.session,
            dimension: snapshot.dimension,
            eye: [snapshot.eye.x, snapshot.eye.y, snapshot.eye.z],
            yaw: snapshot.yaw,
            pitch: snapshot.pitch,
            attack_held: snapshot.attack_held,
            seconds: snapshot.frame_seconds,
            suspended: false,
            players: snapshot
                .players
                .into_iter()
                .map(|player| mod_api::native_frame::Player {
                    runtime_id: player.runtime_id,
                    feet: [player.position.x, player.position.y, player.position.z],
                })
                .collect(),
        })
    }

    /// Read-only current world identity and feet pose, independent of input capture.
    pub(super) fn world_observation(
        &self,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) -> Option<mod_host::WorldObservation> {
        let world = self.world.as_ref()?;
        let authority = world.stream.as_ref()?.authority();
        let session = self.clock.as_ref()?.session_generation();
        if session == 0 || session != ui.session_id() || world.dimension_transfer.active() {
            return None;
        }
        let position = self.view.as_ref()?.feet_translation();
        if !position.is_finite() {
            return None;
        }
        let identity = self.session.as_ref()?.destination_identity()?;
        if identity.len() > mod_api::MAX_WORLD_IDENTITY_BYTES
            || identity.chars().any(char::is_control)
        {
            return None;
        }
        Some(mod_host::WorldObservation {
            session,
            world: identity.to_owned(),
            dimension: authority.current_dimension(),
            position: (position.x, position.y, position.z),
            alive: ui.hud().health().is_none_or(|health| health.current() > 0),
        })
    }

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
        client_extensions::modding::player_state::snapshot(
            authority, session, player, ui, now_millis,
        )
    }

    /// Returns walking ownership independently of temporary server input permissions.
    fn walking_scope(&self, ui: &client_ui::ui_runtime::UiRuntime) -> Option<(u64, i32)> {
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
            || ui
                .hud()
                .health()
                .is_some_and(|health| health.current() == 0)
        {
            return None;
        }
        let player = self.player.as_ref()?;
        let capabilities = player.facts.game_mode_capabilities();
        if player.facts.is_immobile()
            || player.facts.mount_unique_id().is_some()
            || capabilities.is_some_and(|abilities| abilities.flight.is_flying())
            || !self.physics.as_ref()?.jump_pulse_eligible()
        {
            return None;
        }
        let authority = world.stream.as_ref()?.authority();
        Some((authority.actor_session_id(), authority.current_dimension()))
    }

    /// Admits new mod jump requests only while their walking owner has permission.
    fn movement_scope(&self, ui: &client_ui::ui_runtime::UiRuntime) -> Option<(u64, i32)> {
        let scope = self.walking_scope(ui)?;
        (!self
            .player
            .as_ref()?
            .facts
            .input_locks()
            .blocks(protocol::ClientInputLock::Jump))
        .then_some(scope)
    }

    /// Revokes pending jump input when its gameplay ownership changes.
    pub(super) fn synchronize_jump_scope(
        &mut self,
        granted: bool,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) {
        let scope = granted.then(|| self.walking_scope(ui)).flatten();
        if let Some(physics) = self.physics.as_mut() {
            physics.set_jump_pulse_scope(scope);
        }
    }

    /// Samples local movement only for a granted, captured gameplay callback.
    pub(super) fn movement_snapshot(
        &self,
        allowed: bool,
        grants: &ModGrants,
        ui: &client_ui::ui_runtime::UiRuntime,
    ) -> Option<GameplayMovementSnapshot> {
        if !allowed || !grants.movement {
            return None;
        }
        let (session, dimension) = self.movement_scope(ui)?;
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
    pub(super) fn pulse_jump(&mut self, requested: bool, ui: &client_ui::ui_runtime::UiRuntime) {
        if requested
            && self.movement_scope(ui).is_some()
            && let Some(physics) = self.physics.as_mut()
        {
            physics.request_jump_pulse();
        }
    }

    /// No snapshot exists outside captured gameplay or without explicit grants.
    pub(super) fn snapshot(&self, allowed: bool, grants: &ModGrants) -> Option<GameplaySnapshot> {
        if !allowed
            || !(grants.terrain
                || grants.players
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
        if self.player.as_ref().is_some_and(|player| {
            player
                .facts
                .input_locks()
                .blocks(protocol::ClientInputLock::Camera)
        }) {
            return;
        }
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

/// Selects at most K borrowed actors before constructing any guest-owned payloads.
fn nearest_actors<'a>(
    actors: impl Iterator<Item = &'a client_world::ActorSnapshot>,
    eye: Vec3,
    limit: usize,
) -> Vec<&'a client_world::ActorSnapshot> {
    let mut nearest = std::collections::BinaryHeap::with_capacity(limit);
    for actor in actors {
        let candidate = Candidate {
            actor,
            distance: Vec3::from_array(actor.position).distance_squared(eye),
        };
        if nearest.len() < limit {
            nearest.push(candidate);
        } else if nearest.peek().is_some_and(|furthest| candidate < *furthest) {
            *nearest.peek_mut().unwrap() = candidate;
        }
    }
    nearest
        .into_sorted_vec()
        .into_iter()
        .map(|candidate| candidate.actor)
        .collect()
}

struct Candidate<'a> {
    actor: &'a client_world::ActorSnapshot,
    distance: f32,
}

impl PartialEq for Candidate<'_> {
    /// Compares the same distance and identity order used for deterministic selection.
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}
impl Eq for Candidate<'_> {}
impl PartialOrd for Candidate<'_> {
    /// Uses the total ordering even when distance arithmetic overflows to infinity.
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for Candidate<'_> {
    /// Keeps the furthest candidate at the heap root, breaking ties by runtime ID.
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.distance
            .total_cmp(&other.distance)
            .then(self.actor.runtime_id.cmp(&other.actor.runtime_id))
    }
}

/// Reports protocol-classified remote players, never named mobs or list-only users.
fn nearest_players<'a>(
    actors: impl Iterator<Item = &'a client_world::ActorSnapshot>,
    eye: Vec3,
) -> Vec<GameplayPlayer> {
    let eligible = actors
        .filter(|actor| matches!(actor.kind, protocol::ActorKind::Player { .. }))
        .filter(|actor| actor.runtime_id != 0 && Vec3::from_array(actor.position).is_finite());
    nearest_actors(eligible, eye, mod_api::MAX_GAMEPLAY_PLAYERS)
        .into_iter()
        .map(|actor| GameplayPlayer {
            runtime_id: actor.runtime_id,
            position: vector(Vec3::from_array(actor.position)),
        })
        .collect()
}

/// Non-player actors within the published range, nearest first; health is the replicated attribute.
fn nearest_mobs<'a>(
    actors: impl Iterator<Item = &'a client_world::ActorSnapshot>,
    eye: Vec3,
) -> Vec<GameplayMob> {
    let range = mod_api::MAX_MOB_RANGE_BLOCKS * mod_api::MAX_MOB_RANGE_BLOCKS;
    let eligible = actors.filter(|actor| {
        let protocol::ActorKind::Entity { identifier } = &actor.kind else {
            return false;
        };
        let position = Vec3::from_array(actor.position);
        if actor.runtime_id == 0
            || !position.is_finite()
            || position.distance_squared(eye) > range
            || identifier.is_empty()
            || identifier.len() > mod_api::MAX_MOB_TYPE_BYTES
        {
            return false;
        }
        true
    });
    nearest_actors(eligible, eye, mod_api::MAX_GAMEPLAY_MOBS)
        .into_iter()
        .map(|actor| {
            let protocol::ActorKind::Entity { identifier } = &actor.kind else {
                unreachable!()
            };
            let health = actor
                .attributes
                .get("minecraft:health")
                .filter(|health| health.current.is_finite() && health.max.is_finite());
            GameplayMob {
                runtime_id: actor.runtime_id,
                unique_id: actor.unique_id,
                type_id: identifier.to_string(),
                position: vector(Vec3::from_array(actor.position)),
                health: health.map(|health| health.current),
                max_health: health.map(|health| health.max),
            }
        })
        .collect()
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
