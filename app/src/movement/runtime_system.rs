#[cfg(not(feature = "acceptance"))]
use crate::acceptance::AcceptanceRun;
#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;
use std::time::Instant;

use bevy::{
    prelude::{EulerRot, Local, Res, ResMut, Time, Vec3},
    time::Real,
};
use protocol::PlayerInputMode;
use semantic_input::Action;
use {
    crate::player_runtime::PlayerRuntime, crate::runtime::world::ClientWorld,
    crate::semantic_controls::SemanticInputSnapshot, crate::settings_runtime::RuntimeSettings,
    client_presentation::camera::AutoFly, client_presentation::local_player::LocalViewPose,
};

use super::{
    LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
    MovementTicker, PhysicsCollisionRegistries,
};

use gameplay::movement::LocomotionState as LocomotionLocals;

#[allow(clippy::too_many_arguments)]
pub(crate) fn advance_local_physics(
    time: Res<Time<Real>>,
    input: Res<SemanticInputSnapshot>,
    auto_fly: Res<AutoFly>,
    client_world: Res<ClientWorld>,
    collisions: Res<PhysicsCollisionRegistries>,
    acceptance: Res<AcceptanceRun>,
    mut physics: ResMut<LocalPhysicsController>,
    mut movement_effects: ResMut<LocalMovementEffectTimeline>,
    mut movement_speed: ResMut<LocalMovementSpeedAuthority>,
    mut movement_ticker: ResMut<MovementTicker>,
    mut view: ResMut<LocalViewPose>,
    settings: Option<Res<RuntimeSettings>>,
    player: Option<Res<PlayerRuntime>>,
    item_use: Option<Res<crate::item_use::ItemUseRuntime>>,
    mut locals: Local<LocomotionLocals>,
) {
    if acceptance.deadline_reached(Instant::now()) {
        movement_ticker.begin_terminal_drain();
    }
    if auto_fly.enabled() || !physics.is_active() {
        locals.reset();
        return;
    }
    if !movement_ticker.can_advance_physics_frame() {
        // Transport admission pauses ticks, not publication of the retained visual pose.
        publish_physics_view(&physics, &mut view);
        return;
    }
    let Some(stream) = client_world.stream.as_ref() else {
        return;
    };
    let semantic = input.snapshot();
    let active = semantic.is_some();
    let input_mode = semantic.map_or(PlayerInputMode::Mouse, |snapshot| {
        match snapshot.input_mode {
            semantic_input::InputMode::KeyboardMouse => PlayerInputMode::Mouse,
            semantic_input::InputMode::GamePad => PlayerInputMode::GamePad,
            semantic_input::InputMode::Touch => PlayerInputMode::Touch,
        }
    });
    let movement = input.movement();
    let raw_movement = input.raw_movement();
    let analogue_movement = input.analogue_movement();
    let (bevy_yaw, bevy_pitch, _) = view.rotation().to_euler(EulerRot::YXZ);
    let yaw = gameplay::movement::wire_yaw(180.0 - bevy_yaw.to_degrees());
    let facts = gameplay::movement::local_facts::read(
        player.as_deref().map(|player| &**player),
        &super::GameplayWorldView(stream),
        item_use
            .as_deref()
            .is_some_and(|item_use| item_use.is_using()),
    );
    let gameplay = settings
        .as_deref()
        .map(|settings| settings.user_settings_update().1.gameplay)
        .unwrap_or_default();
    let world = sim::PaletteWorld::new(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
    );
    let advanced = locals.advance(
        gameplay::movement::PhysicsFrameInput {
            delta: time.delta(),
            now: time.elapsed(),
            active,
            movement,
            raw_movement,
            analogue_movement,
            movement_buttons: semantic
                .map_or_else(Default::default, |snapshot| snapshot.movement_buttons),
            yaw,
            pitch: -bevy_pitch.to_degrees(),
            camera_orientation: (view.rotation() * Vec3::NEG_Z).to_array(),
            input_mode,
            jump: input.phase(Action::Jump),
            sprint: input.phase(Action::Sprint),
            sneak: input.phase(Action::Sneak),
            toggle_sprint: gameplay.toggle_sprint,
            always_sprint: gameplay.always_sprint && input_mode == PlayerInputMode::Mouse,
            toggle_sneak: gameplay.toggle_sneak,
            facts,
            item_use_modifier: item_use
                .as_deref()
                .and_then(|item_use| item_use.movement_modifier()),
            hold: (client_world.dimension_transfer.active() || client_world.respawn.input_held())
                .then(|| gameplay::movement::PhysicsFrameHold {
                    registry: collisions.registry(stream.network_id_mode()).identity(),
                    withhold_input: client_world.respawn.input_held(),
                }),
        },
        &mut physics,
        &mut movement_ticker,
        &mut movement_effects,
        &mut movement_speed,
        &world,
    );
    if !advanced {
        return;
    }
    publish_physics_view(&physics, &mut view);
}

/// Publishes the retained interpolated pose even while transport pauses simulation.
fn publish_physics_view(physics: &LocalPhysicsController, view: &mut LocalViewPose) {
    if let (Some(eye), Some(feet)) = (
        physics.render_eye_position(),
        physics.render_feet_position(),
    ) {
        view.set_subject_position(Vec3::from_array(eye), Vec3::from_array(feet));
    }
}
