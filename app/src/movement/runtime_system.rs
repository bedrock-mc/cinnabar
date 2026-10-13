#[cfg(not(feature = "acceptance"))]
use crate::acceptance::AcceptanceRun;
#[cfg(feature = "acceptance")]
use ::acceptance::AcceptanceRun;
use std::time::Instant;

use crate::runtime::network::resource_packs::PackApplication;
use bevy::{
    prelude::{EulerRot, Local, Res, ResMut, Time, Vec3},
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
    item_use: Option<Res<client_interaction::item_use::ItemUseRuntime>>,
    mut locals: Local<(LocomotionLocals, Option<[i32; 3]>)>,
    network: Option<Res<NetworkHandle<PackApplication>>>,
) {
    let (locals, presented_bed) = &mut *locals;
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
        .as_deref()
        .map(|settings| settings.user_settings_update().1.gameplay)
        .unwrap_or_default();
    let frame = gameplay::movement::PhysicsFrameInput {
        delta: time.delta(),
        now: time.elapsed(),
        active,
        input_authority_released: semantic.is_none_or(|snapshot| snapshot.authority_released),
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
    locals.observe_dismount(&frame, movement_ticker.session_generation());
    if !movement_ticker.can_advance_physics_frame() {
        // Retain presses and publish the pose while transport pauses simulation.
        publish_physics_view(&physics, &mut view, sleeping_bed(stream), presented_bed);
        return;
    }
    let world = sim::PaletteWorld::for_movement(
        stream.collision_store(),
        collisions.registry(stream.network_id_mode()),
        stream.current_dimension(),
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
    publish_physics_view(&physics, &mut view, sleeping_bed(stream), presented_bed);
}

/// The bed the local player sleeps in, once the server says so.
fn sleeping_bed(stream: &chunk_pipeline::WorldStream) -> Option<[i32; 3]> {
    stream.authority().local_sleeping_bed().map(|(bed, _)| bed)
}

/// Publishes the retained interpolated pose even while transport pauses simulation. A sleeper
/// presents lying on its bed, where the native client puts it, while its physics stays put.
fn publish_physics_view(
    physics: &LocalPhysicsController,
    view: &mut LocalViewPose,
    bed: Option<[i32; 3]>,
    presented_bed: &mut Option<[i32; 3]>,
) {
    if *presented_bed != bed {
        view.reanchor_camera();
        *presented_bed = bed;
    }
    if let Some(bed) = bed {
        let (eye, feet) = client_presentation::local_player::sleeping_subject_position(bed);
        view.set_subject_position(eye, feet);
        return;
    }
    if let (Some(eye), Some(feet)) = (
        physics.render_eye_position(),
        physics.render_feet_position(),
    ) {
        view.set_subject_position(Vec3::from_array(eye), Vec3::from_array(feet));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::prelude::{App, Camera3d, Update};
    use client_presentation::camera::{CameraSettingsAuthority, FlyCamera, ServerCameraView};
    use render_scene::CameraMotionBlur;

    #[test]
    fn sleep_and_wake_clear_motion_blur_once_without_moving_physics() {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([2.5, 2.0, 5.5], 100, false);
        let position = physics.network_position();
        let mut app = App::new();
        app.init_resource::<LocalViewPose>()
            .init_resource::<CameraSettingsAuthority>()
            .init_resource::<ServerCameraView>()
            .add_systems(
                Update,
                client_presentation::camera::motion_blur::apply_camera_motion_blur,
            );
        let camera = app
            .world_mut()
            .spawn((FlyCamera::default(), Camera3d::default()))
            .id();
        let mut settings = ui::UserSettings::default();
        settings.gameplay.default_perspective = semantic_input::PerspectiveMode::ThirdPersonBack;
        settings.video.motion_blur = ui::MotionBlurQuality::High;
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .replace(1, &settings)
            .unwrap();
        let bed = [2, 0, 5];
        let mut presented_bed = None;
        let mut epoch = 0;
        for (sleeping, transition) in [
            (false, false),
            (false, false),
            (true, true),
            (true, false),
            (false, true),
            (false, false),
        ] {
            publish_physics_view(
                &physics,
                &mut app.world_mut().resource_mut::<LocalViewPose>(),
                sleeping.then_some(bed),
                &mut presented_bed,
            );
            let view = *app.world().resource::<LocalViewPose>();
            if transition {
                epoch += 1;
            }
            assert_eq!(view.camera_reanchor_epoch(), epoch);
            assert_eq!(view.rotation(), bevy::math::Quat::IDENTITY);
            let expected = if sleeping {
                client_presentation::local_player::sleeping_subject_position(bed).0
            } else {
                Vec3::from_array(physics.render_eye_position().unwrap())
            };
            assert_eq!(view.eye_translation(), expected);
            app.update();
            let blur = app.world().get::<CameraMotionBlur>(camera).unwrap();
            if transition {
                assert_eq!(blur.exposure_seconds, 0.0);
            } else if epoch > 0 {
                assert!(blur.exposure_seconds > 0.0);
            }
            assert_eq!(physics.network_position(), position);
        }
    }
}
