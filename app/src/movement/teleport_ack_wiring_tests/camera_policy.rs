use super::*;
use bevy::prelude::{EulerRot, Quat, Vec3};

#[test]
fn teleport_camera_opt_in_keeps_production_position_reconciliation_and_ack() {
    let aim = Quat::from_euler(EulerRot::YXZ, 0.35, -0.2, 0.0);
    for enabled in [false, true] {
        let mut physics = LocalPhysicsController::default();
        physics.reanchor_network_position([0.0, 70.0, 0.0], 100, true);
        let mut app = wiring_app(authorized_ticker(false), physics);
        app.world_mut()
            .resource_mut::<CameraSettingsAuthority>()
            .set_preserve_teleport_rotation(enabled);
        app.world_mut()
            .insert_resource(LocalViewPose::new(Vec3::ZERO, aim));
        submit(
            &mut app,
            1,
            WorldEvent::MovePlayer(MovePlayerEvent {
                runtime_id: 1,
                position: [30.5, 71.620_01, 0.5],
                yaw: 90.0,
                pitch: 15.0,
                mode: protocol::MovePlayerMode::Teleport,
                teleported: true,
                source_tick: 90,
                ..Default::default()
            }),
        );
        app.update();
        let physics_position = app
            .world()
            .resource::<LocalPhysicsController>()
            .network_position()
            .unwrap();
        assert!(
            Vec3::from_array(physics_position).abs_diff_eq(Vec3::new(30.5, 71.620_01, 0.5), 0.0001)
        );
        let view = app.world().resource::<LocalViewPose>();
        assert!(
            view.eye_translation()
                .abs_diff_eq(Vec3::new(30.5, 71.620_01, 0.5), 0.0001)
        );
        let expected = if enabled {
            aim
        } else {
            crate::runtime::telemetry::bedrock_camera_rotation(90.0, 15.0)
        };
        assert!(view.rotation().abs_diff_eq(expected, 0.0001));
        assert_eq!(
            app.world()
                .resource::<MovementTicker>()
                .pending_teleport_ack_admitted_ticks(),
            Some(TELEPORT_ACK_ADMITTED_TICK_BUDGET)
        );
        let mut ticker = app.world_mut().remove_resource::<MovementTicker>().unwrap();
        let packets = transmit_two_after_reconciliation(&mut ticker);
        assert_eq!(packets.len(), 2);
        assert!(carries_handled_teleport(&packets[0]));
        assert!(!carries_handled_teleport(&packets[1]));
        assert_eq!(ticker.pending_teleport_ack_admitted_ticks(), None);
    }
}
