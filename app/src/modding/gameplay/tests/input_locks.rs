use super::*;
use protocol::{ClientInputLock, ClientInputLocks};

#[test]
fn server_input_locks_gate_mod_camera_deltas_and_allow_fresh_deltas_after_unlock() {
    /// Applies a committed extension output through its production system parameter.
    fn rotate(mut context: GameplayContext) {
        context.apply(CameraDelta {
            yaw: 0.3,
            pitch: 0.2,
        });
    }
    let mut app = App::new();
    let initial = Quat::from_rotation_y(0.4);
    app.insert_resource(client_state::PlayerRuntime(player_state::PlayerState::new(
        7,
    )))
    .insert_resource(LocalViewPose::new(Vec3::Y * 70.0, initial))
    .add_systems(Update, rotate);
    for (bits, blocked) in [
        (ClientInputLock::Camera as u32, true),
        (0, false),
        (ClientInputLock::Movement as u32, false),
    ] {
        app.world_mut()
            .resource_mut::<LocalViewPose>()
            .set_rotation(initial);
        app.world_mut()
            .resource_mut::<client_state::PlayerRuntime>()
            .facts
            .apply_input_locks(7, ClientInputLocks::from_bits(bits));
        app.update();
        assert_eq!(
            app.world().resource::<LocalViewPose>().rotation() == initial,
            blocked
        );
    }
}
