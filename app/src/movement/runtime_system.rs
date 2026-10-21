#[cfg(not(feature = "acceptance"))]
use crate::acceptance::AcceptanceRun;
#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;
use web_time::Instant;

use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    ecs::system::SystemParam,
    prelude::{EulerRot, Res, ResMut, Time, Vec3},
    time::Real,
};
use client_runtime::network::NetworkHandle;
use protocol::PlayerInputMode;
use semantic_input::Action;
use {
    crate::runtime::world::ClientWorld,
    client_presentation::{camera::AutoFly, local_player::LocalViewPose},
    client_state::{PlayerRuntime, RuntimeSettings, SemanticInputSnapshot},
};

use client_interaction::movement::{
    LocalMovementEffectTimeline, LocalMovementSpeedAuthority, LocalPhysicsController,
    MovementTicker, PhysicsCollisionRegistries,
};

use client_interaction::movement::LocomotionState;

/// Reads user movement settings and any native extension policy in one system parameter.
#[derive(SystemParam)]
pub(crate) struct MovementSettings<'w> {
    camera: Option<Res<'w, client_presentation::camera::CameraSettingsAuthority>>,
    settings: Option<Res<'w, RuntimeSettings>>,
    #[cfg(not(target_arch = "wasm32"))]
    mods: Option<Res<'w, crate::modding::ModRuntime>>,
}

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
    settings: MovementSettings<'_>,
    player: Option<Res<PlayerRuntime>>,
    item_use: Option<Res<client_interaction::item_use::ItemUseRuntime>>,
    mut locals: ResMut<LocomotionState>,
    network: Option<Res<NetworkHandle<PackApplication>>>,
) {
    movement_effects.begin_frame();
    if acceptance.deadline_reached(Instant::now()) {
        movement_ticker.begin_terminal_drain();
    }
    if auto_fly.enabled() || !physics.is_active() {
        locals.reset();
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
        &client_interaction::movement::world_view::GameplayWorldView(stream),
        item_use
            .as_deref()
            .is_some_and(|item_use| item_use.is_using()),
    );
    let gameplay = settings
        .settings
        .as_deref()
        .map(|settings| settings.user_settings_update().1.gameplay)
        .unwrap_or_default();
    #[cfg(not(target_arch = "wasm32"))]
    let gameplay = {
        let mut gameplay = gameplay;
        if let Some(policy) = settings
            .mods
            .as_deref()
            .and_then(|mods| mods.movement_toggles())
        {
            gameplay.toggle_sprint = policy.toggle_sprint;
            gameplay.toggle_sneak = policy.toggle_sneak;
            gameplay.always_sprint = policy.always_sprint;
        }
        gameplay
    };
    let mut frame = gameplay::movement::PhysicsFrameInput {
        delta: time.delta(),
        now: time.elapsed(),
        active,
        input_authority_released: semantic.is_none_or(|snapshot| snapshot.authority_released),
        resolved_movement: input.movement_is_resolved(),
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
        hold: (client_world.dimension_transfer.active() || client_world.respawn.input_held()).then(
            || gameplay::movement::PhysicsFrameHold {
                registry: collisions.registry(stream.network_id_mode()).identity(),
                withhold_input: client_world.respawn.input_held(),
            },
        ),
    };
    if settings.camera.as_deref().is_some_and(|camera| {
        matches!(
            camera.detached_view(),
            Some(client_presentation::camera::DetachedView::Fly { .. })
        )
    }) {
        frame.movement = [0.0; 2];
        frame.raw_movement = [0.0; 2];
        frame.analogue_movement = [0.0; 2];
        frame.movement_buttons = Default::default();
        frame.jump = Default::default();
        frame.sprint = Default::default();
        frame.sneak = Default::default();
        frame.always_sprint = false;
        frame.toggle_sprint = false;
        frame.toggle_sneak = false;
    }

    if !movement_ticker.can_advance_physics_frame() {
        locals.observe_dismount(&frame, movement_ticker.session_generation());
        // Retain presses and publish the pose while transport pauses simulation.
        publish_physics_view(&physics, &mut view);
        return;
    }
    let world = super::collision_world(
        stream,
        collisions.registry(stream.network_id_mode()),
        &client_world.movement_colliders,
    );
    let advanced = locals.advance(
        frame,
        &mut physics,
        &mut movement_ticker,
        &mut movement_effects,
        &mut movement_speed,
        &world,
    );
    if !advanced {
        return;
    }
    if let (Some(position), Some(network)) = (physics.network_position(), network) {
        locals.flush_dismount(position, |packet| {
            network.send_inventory_packets(vec![packet])
        });
    }
    publish_physics_view(&physics, &mut view);
}

/// Publishes retained physics while the shared presentation source resolves visual overrides.
fn publish_physics_view(physics: &LocalPhysicsController, view: &mut LocalViewPose) {
    if let (Some(eye), Some(feet)) = (
        physics.render_eye_position(),
        physics.render_feet_position(),
    ) {
        view.set_subject_position(Vec3::from_array(eye), Vec3::from_array(feet));
    }
}
