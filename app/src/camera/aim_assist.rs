//! Packet-driven aim selection publishes a separate interaction ray.

use std::sync::Arc;

use bevy::{ecs::system::SystemParam, prelude::*};
use client_presentation::{
    aim_assist::{AimAssistFrame, ServerAimAssist},
    server_camera::ServerCameraInstructions,
};
use protocol::CameraAimAssistPresetSettings;
use semantic_input::PerspectiveMode;

use crate::{
    camera::{CameraSettingsAuthority, ServerCameraView},
    environment::WorldClock,
    local_player::{InteractionOriginSnapshot, LocalViewPose},
    movement::{LocalPhysicsController, PhysicsCollisionRegistries},
    player_runtime::PlayerRuntime,
    runtime::{network::NetworkHandle, world::ClientWorld},
};

/// Retains only activation identity; steady frames create no outbound packets.
#[derive(Default)]
pub(crate) struct AimActivation {
    identity: Option<(u64, i32)>,
    camera: Option<Arc<str>>,
    settings: Option<CameraAimAssistPresetSettings>,
    supported: Option<bool>,
}

/// The published ray and server policies share one ordered frame boundary.
#[derive(SystemParam)]
pub(crate) struct AimContext<'w> {
    world: Res<'w, ClientWorld>,
    clock: Res<'w, WorldClock>,
    collisions: Res<'w, PhysicsCollisionRegistries>,
    player: Res<'w, PlayerRuntime>,
    instructions: Res<'w, ServerCameraInstructions>,
    camera: Res<'w, ServerCameraView>,
    camera_settings: Res<'w, CameraSettingsAuthority>,
    network: Res<'w, NetworkHandle>,
    physics: Res<'w, LocalPhysicsController>,
}

/// Native assistance changes interaction aim on every device without adding continuous look input.
pub(crate) fn publish_assisted_interaction(
    context: AimContext,
    mut state: ResMut<ServerAimAssist>,
    mut frame: ResMut<AimAssistFrame>,
    mut interaction: ResMut<InteractionOriginSnapshot>,
    mut activation: Local<AimActivation>,
) {
    let identity = context.world.stream.as_ref().map(|stream| {
        (
            context.clock.session_generation(),
            stream.current_dimension(),
        )
    });
    state.observe_identity(identity);
    let previous_skips = state.semantic_skips();
    for instruction in context.instructions.iter() {
        state.apply(instruction.sequence, &instruction.event);
    }
    frame.synchronize(&state);
    if state.semantic_skips() != previous_skips {
        warn!(
            skipped = state.semantic_skips() - previous_skips,
            "skipped unknown aim-assist presets"
        );
    }
    let Some(stream) = context.world.stream.as_ref() else {
        return;
    };
    activation.publish(&context, &state, identity);
    let Some(ray) = interaction.outbound_ray() else {
        return;
    };
    let tick = ray.physics_tick();
    let mode = stream.network_id_mode();
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        context.collisions.registry(mode),
        stream.current_dimension(),
    );
    let item = context
        .player
        .selected_stack()
        .filter(|stack| !stack.is_empty())
        .and_then(|stack| stream.authority().item_identifier(stack.network_id));
    let previous_queries = frame.skipped_queries;
    for (tick, eye, forward) in context.physics.aim_poses_after(frame.last_tick(), tick) {
        frame.evaluate(
            &state,
            stream.authority(),
            &world,
            tick,
            Vec3::from_array(eye),
            Vec3::from_array(forward),
            stream.authority().actor(stream.local_player_runtime_id()),
            item.as_ref(),
            |id| context.collisions.block_identifier(mode, id),
            |id| context.collisions.block_tags(mode, id),
        );
    }
    if frame.skipped_queries != previous_queries
        && (previous_queries == 0 || previous_queries / 64 != frame.skipped_queries / 64)
    {
        warn!(
            total = frame.skipped_queries,
            "skipped unavailable aim-assist target data"
        );
    }
    if let Some(direction) = frame.interaction_direction() {
        interaction.assist_direction(direction);
    }
}

impl AimActivation {
    /// Camera activation requests policy from the server, which owns the resulting settings.
    fn publish(
        &mut self,
        context: &AimContext<'_>,
        state: &ServerAimAssist,
        identity: Option<(u64, i32)>,
    ) {
        let camera = context.camera.active_preset_name();
        let settings = context.camera.active_aim_assist();
        let supported = context.camera.active_base_preset_name().map_or_else(
            || context.camera_settings.perspective() != PerspectiveMode::FirstPerson,
            |base| base != "minecraft:first_person",
        );
        if self.identity == identity
            && self.camera.as_deref() == camera
            && self.settings.as_ref() == settings
            && self.supported == Some(supported)
        {
            return;
        }
        let clear = !supported || settings.is_none();
        if !clear
            && !state.has_preset(
                settings
                    .and_then(|settings| settings.preset_id.as_deref())
                    .unwrap_or(client_presentation::aim_assist::DEFAULT_AIM_ASSIST_PRESET),
            )
        {
            return;
        }
        let packet = protocol::camera_aim_assist_activation_packet(
            if clear { "" } else { camera.unwrap_or("") },
            supported,
            clear,
        );
        if context
            .network
            .send_settings_packet(context.clock.session_generation(), packet)
            .is_ok()
        {
            self.identity = identity;
            self.camera = camera.map(Arc::from);
            self.settings = settings.cloned();
            self.supported = Some(supported);
        }
    }
}

/// Applies the native action override to player facing and the same unsent movement tick.
pub(crate) fn rotate_for_action(
    frame: &AimAssistFrame,
    camera: &ServerCameraView,
    view: &mut LocalViewPose,
    movement: &mut crate::movement::MovementTicker,
    tick: u64,
) {
    if let Some(rotation) = action_rotation(frame, camera) {
        apply_action_rotation(rotation, view, movement, tick);
    }
}

/// The facing an attack or release turns the player to, when the camera and scheme allow it.
pub(crate) fn action_rotation(frame: &AimAssistFrame, camera: &ServerCameraView) -> Option<Quat> {
    use client_presentation::aim_assist::{AimAssistControlScheme, rotates_player_on_projectile};
    let direction = frame.interaction_direction()?;
    let scheme = AimAssistControlScheme::from_wire(camera.active_control_scheme().unwrap_or(0))?;
    camera
        .active_base_preset_name()
        .is_some_and(|camera| rotates_player_on_projectile(camera, scheme))
        .then(|| crate::camera::look_at_target(Vec3::ZERO, direction))
}

/// Writes an action facing to the view and to `tick`'s unsent movement input.
pub(crate) fn apply_action_rotation(
    rotation: Quat,
    view: &mut LocalViewPose,
    movement: &mut crate::movement::MovementTicker,
    tick: u64,
) {
    let (yaw, pitch, _) = rotation.to_euler(EulerRot::YXZ);
    if movement.override_action_rotation(
        tick,
        -pitch.to_degrees(),
        (180.0 - yaw.to_degrees()).rem_euclid(360.0),
    ) {
        view.set_rotation(rotation);
    }
}
