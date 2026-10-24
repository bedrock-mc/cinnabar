#[test]
fn zeqa_correction_burst_preserves_outbound_look() {
    #[derive(serde::Deserialize)]
    struct Sample {
        tick: u64,
        position: [f32; 3],
        pos_delta: [f32; 3],
        move_vector: [f32; 2],
        analog_move_vector: [f32; 2],
        raw_move_vector: [f32; 2],
        pitch: f32,
        yaw: f32,
        head_yaw: f32,
        camera_orientation: [f32; 3],
    }
    #[derive(serde::Deserialize)]
    struct Correction {
        tick: u64,
        position: [f32; 3],
    }
    #[derive(serde::Deserialize)]
    struct Fixture {
        before: Sample,
        corrections: Vec<Correction>,
    }

    // Numeric fields from the first correction burst in the October 1 trace.
    let fixture: Fixture =
        serde_json::from_str(include_str!("fixtures/zeqa_rotation.json")).unwrap();
    let before = fixture.before;
    let mut view = LocalViewPose::new(
        Vec3::from_array(before.position),
        bedrock_camera_rotation(before.yaw, before.pitch),
    );
    let mut settings = CameraSettingsAuthority::default();
    let mut pending_surface_spawn = None;
    for recorded in fixture.corrections {
        // Rotation and velocity were absent from the correction log.
        let correction = PlayerMovementCorrectionEvent {
            position: recorded.position,
            delta: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            subject: protocol::MovementCorrectionSubject::Player,
            on_ground: false,
            tick: recorded.tick,
            vehicle_angular_velocity: None,
        };
        apply_committed_control(
            CommittedControlEvent::PlayerMovementCorrection {
                sequence: recorded.tick,
                correction,
                resolved: client_world::ResolvedServerPosition {
                    position: correction.position,
                    surface_anchor: None,
                },
            },
            &mut view,
            &mut settings,
            &mut pending_surface_spawn,
        );
        assert_eq!(view.eye_translation().to_array(), recorded.position);
        // Use the same view-to-wire conversion as the fixed movement system.
        let (bevy_yaw, bevy_pitch, _) = view.rotation().to_euler(bevy::math::EulerRot::YXZ);
        let yaw = (180.0 - bevy_yaw.to_degrees()).rem_euclid(360.0);
        let packet = protocol::player_auth_input(protocol::PlayerAuthInputSnapshot {
            tick: before.tick + 1,
            position: view.eye_translation().to_array(),
            delta: before.pos_delta,
            move_vector: before.move_vector,
            analogue_move_vector: before.analog_move_vector,
            raw_move_vector: before.raw_move_vector,
            pitch: -bevy_pitch.to_degrees(),
            yaw,
            head_yaw: yaw,
            camera_orientation: (view.rotation() * Vec3::NEG_Z).to_array(),
            flags: protocol::PlayerInputFlags::NONE,
            input_mode: protocol::PlayerInputMode::Mouse,
            vehicle: None,
        })
        .unwrap();
        let sent = protocol::player_auth_input_trace_sample(&packet).unwrap();
        assert!((sent.yaw - before.yaw).abs() < 1.0e-4);
        assert!((sent.pitch - before.pitch).abs() < 1.0e-4);
        assert!((sent.head_yaw - before.head_yaw).abs() < 1.0e-4);
        assert!(
            Vec3::from_array(sent.camera_orientation)
                .abs_diff_eq(Vec3::from_array(before.camera_orientation), 1.0e-6)
        );
    }
}
