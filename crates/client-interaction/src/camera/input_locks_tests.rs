use super::*;
use protocol::{ClientInputLock, ClientInputLocks};
use semantic_input::{
    ControllerFrame, DeviceFrame, KeyboardMouseFrame, SemanticInputRouter, TouchContact,
};

/// Exercises a physical look source while locked and after its permission is restored.
fn assert_look_locks(mut router: SemanticInputRouter, mut device: impl FnMut(u64) -> DeviceFrame) {
    let mut app = App::new();
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(7));
    player.facts.apply_input_locks(
        7,
        ClientInputLocks::from_bits(ClientInputLock::Camera as u32),
    );
    let initial = Quat::from_rotation_y(0.4);
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_millis(50));
    app.insert_resource(time);
    app.insert_resource(player)
        .insert_resource(LocalViewPose::new(Vec3::Y * 70.0, initial))
        .insert_resource(AutoFly::new(false))
        .init_resource::<CameraSettingsAuthority>()
        .init_resource::<Time>()
        .init_resource::<look::LookSmoother>()
        .add_systems(Update, update_look);
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    for (sequence, locked) in [(1, true), (2, false)] {
        app.world_mut()
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .apply_input_locks(
                7,
                ClientInputLocks::from_bits(if locked {
                    ClientInputLock::Camera as u32
                } else {
                    0
                }),
            );
        router.route(device(sequence)).unwrap();
        let mut input = SemanticInputSnapshot::default();
        input.replace(router.finalize().unwrap());
        app.insert_resource(input);
        app.update();
        assert_eq!(
            app.world().resource::<LocalViewPose>().rotation() == initial,
            locked
        );
    }
}

#[test]
fn server_input_locks_stop_mouse_look_and_unlock_resumes_input() {
    assert_look_locks(Default::default(), |sequence| DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            activity_sequence: sequence,
            mouse_motion: [80.0, 20.0],
            ..Default::default()
        }),
        ..Default::default()
    });
}

#[test]
fn server_input_locks_stop_gamepad_look_and_unlock_resumes_input() {
    assert_look_locks(Default::default(), |sequence| DeviceFrame {
        controllers: vec![ControllerFrame {
            activity_sequence: sequence,
            axes: [0.0, 0.0, 0.8, 0.5, 0.0, 0.0, 0.0, 0.0],
            ..Default::default()
        }],
        ..Default::default()
    });
}

#[test]
fn server_input_locks_stop_touch_look_and_unlock_resumes_input() {
    assert_look_locks(Default::default(), |sequence| DeviceFrame {
        touches: vec![TouchContact {
            contact_id: 1,
            activity_sequence: sequence,
            position: [0.0; 2],
            delta: [80.0, 20.0],
            hit_id: Some(semantic_input::touch::LOOK_SURFACE),
        }],
        ..Default::default()
    });
}

#[test]
fn server_input_locks_stop_rebound_keyboard_look() {
    let settings = semantic_input::ControlSettings::default();
    let chord = settings
        .bindings()
        .iter()
        .find(|binding| {
            binding.context == semantic_input::InputContext::Gameplay
                && binding.action == semantic_input::Action::MoveRight
                && matches!(
                    binding.chord.control,
                    semantic_input::PhysicalControl::KeyboardUsage(_)
                )
        })
        .unwrap()
        .chord;
    let semantic_input::PhysicalControl::KeyboardUsage(usage) = chord.control else {
        unreachable!()
    };
    let mut bindings = settings.bindings().to_vec();
    bindings.push(semantic_input::ActionBinding {
        action: semantic_input::Action::LookRight,
        context: semantic_input::InputContext::Gameplay,
        chord,
    });
    let settings = semantic_input::ControlSettings::new(
        bindings,
        settings.mouse_sensitivity,
        settings.gamepad_look_sensitivity,
        settings.touch_look_sensitivity,
        settings.invert_mouse_y,
        settings.invert_gamepad_y,
        settings.gamepad_move_deadzone,
        settings.gamepad_look_deadzone,
    )
    .unwrap();
    let router = SemanticInputRouter::with_settings_and_touch_layout(
        settings,
        semantic_input::TouchControlLayout::default(),
    )
    .unwrap();
    assert_look_locks(router, |sequence| DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            activity_sequence: sequence,
            keys: vec![usage],
            ..Default::default()
        }),
        ..Default::default()
    });
}

#[test]
fn server_input_locks_discard_a_retained_cinematic_look_tail() {
    let mut app = App::new();
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_millis(20));
    let mut settings = CameraSettingsAuthority::default();
    let mut preferences = ui::UserSettings::default();
    preferences.video.cinematic_camera = true;
    settings.replace(1, &preferences).unwrap();
    let initial = Quat::IDENTITY;
    let mut smoother = look::LookSmoother::default();
    smoother.filter(Vec2::new(100.0, 20.0), 0.02);
    assert_ne!(smoother.filter(Vec2::ZERO, 0.02), Vec2::ZERO);
    app.insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
        7,
    )))
    .insert_resource(LocalViewPose::new(Vec3::Y, initial))
    .insert_resource(AutoFly::new(false))
    .insert_resource(settings)
    .insert_resource(time)
    .insert_resource(smoother)
    .init_resource::<SemanticInputSnapshot>()
    .add_systems(Update, update_look);
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    for bits in [protocol::ClientInputLock::Camera as u32, 0] {
        app.world_mut()
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .apply_input_locks(7, ClientInputLocks::from_bits(bits));
        app.update();
        assert_eq!(app.world().resource::<LocalViewPose>().rotation(), initial);
    }
}

#[test]
fn server_input_locks_gate_non_physics_fly_movement() {
    let mut router = SemanticInputRouter::default();
    router
        .route(DeviceFrame {
            controllers: vec![ControllerFrame {
                activity_sequence: 1,
                axes: [0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                buttons: vec![0],
                ..Default::default()
            }],
            ..Default::default()
        })
        .unwrap();
    let mut input = SemanticInputSnapshot::default();
    input.replace(router.finalize().unwrap());
    let mut time = Time::<()>::default();
    time.advance_by(std::time::Duration::from_millis(50));
    let mut app = App::new();
    app.insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
        7,
    )))
    .insert_resource(LocalViewPose::new(Vec3::Y, Quat::IDENTITY))
    .insert_resource(AutoFly::new(false))
    .insert_resource(time)
    .insert_resource(input)
    .add_systems(Update, update_movement);
    app.world_mut().spawn(FlyCamera::default());
    for (bits, blocked) in [(ClientInputLock::Movement as u32, true), (0, false)] {
        app.world_mut()
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .apply_input_locks(7, ClientInputLocks::from_bits(bits));
        app.update();
        assert_eq!(
            app.world().resource::<LocalViewPose>().eye_translation() == Vec3::Y,
            blocked
        );
    }
}
