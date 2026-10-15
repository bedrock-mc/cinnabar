use super::*;
use protocol::{ClientInputLock, ClientInputLocks};
use semantic_input::{DeviceFrame, KeyboardMouseFrame, SemanticInputRouter};

#[test]
fn server_input_locks_stop_look_and_unlock_resumes_held_device_input() {
    let mut app = App::new();
    let mut player = client_state::PlayerRuntime(player_state::PlayerState::new(7));
    player.facts.apply_input_locks(
        7,
        ClientInputLocks::from_bits(ClientInputLock::Camera as u32),
    );
    let initial = Quat::from_rotation_y(0.4);
    app.insert_resource(player)
        .insert_resource(LocalViewPose::new(Vec3::Y * 70.0, initial))
        .insert_resource(AutoFly::new(false))
        .init_resource::<CameraSettingsAuthority>()
        .init_resource::<Time>()
        .init_resource::<look::LookSmoother>()
        .add_systems(Update, update_look);
    app.world_mut().spawn((Window::default(), PrimaryWindow));
    let mut router = SemanticInputRouter::default();
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
        router
            .route(DeviceFrame {
                keyboard_mouse: Some(KeyboardMouseFrame {
                    activity_sequence: sequence,
                    mouse_motion: [80.0, 20.0],
                    ..Default::default()
                }),
                ..Default::default()
            })
            .unwrap();
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
