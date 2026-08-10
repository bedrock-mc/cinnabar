#[test]
fn frozen_local_player_frame_samples_pose_and_interaction_identity_atomically() {
    let sample = frozen_local_player_sample();
    let expected = sample.clone();
    let mut carrier = LocalPlayerFrameCarrier::default();

    carrier.publish(sample).unwrap();

    let frozen = carrier.snapshot().expect("one frozen local-player frame");
    assert_eq!(frozen.session_generation(), expected.session_generation);
    assert_eq!(frozen.fifo_sequence(), expected.fifo_sequence);
    assert_eq!(frozen.physics_tick(), expected.physics_tick);
    assert_eq!(frozen.pose_generation(), 1);
    assert_eq!(frozen.perspective(), expected.perspective);
    assert_eq!(
        frozen.world_collision_identity(),
        &expected.world_collision_identity
    );
    assert_eq!(frozen.pose(), &expected.pose);
    assert_eq!(frozen.eye(), expected.eye);
    assert_eq!(frozen.rotation(), expected.rotation);
    assert!(
        frozen
            .direction()
            .abs_diff_eq(expected.rotation * Vec3::NEG_Z, 1.0e-6)
    );
}

#[test]
fn correction_session_and_dimension_resets_invalidate_the_frozen_frame_generation() {
    for reset in [
        LocalPlayerFrameReset::Correction,
        LocalPlayerFrameReset::Session,
        LocalPlayerFrameReset::Dimension,
    ] {
        let mut carrier = LocalPlayerFrameCarrier::default();
        let sample = frozen_local_player_sample();
        carrier.publish(sample.clone()).unwrap();
        let stale_generation = carrier.snapshot().unwrap().pose_generation();

        carrier.reset(reset);

        assert!(carrier.snapshot().is_none());
        let mut replacement = sample;
        replacement.session_generation += u64::from(reset == LocalPlayerFrameReset::Session);
        replacement.fifo_sequence += 1;
        replacement.physics_tick += 1;
        carrier.publish(replacement).unwrap();
        assert!(carrier.snapshot().unwrap().pose_generation() > stale_generation);
    }
}

#[test]
fn committed_movement_correction_updates_position_without_overwriting_local_view_rotation() {
    let correction = PlayerMovementCorrectionEvent {
        position: [27.5, 111.0, 91.5],
        delta: [0.25, -0.5, 0.75],
        pitch: -15.0,
        yaw: 90.0,
        on_ground: true,
        tick: 55,
    };
    let local_rotation = Quat::from_euler(bevy::math::EulerRot::YXZ, 0.35, -0.2, 0.0);
    let mut view = LocalViewPose::new(Vec3::ZERO, local_rotation);
    let mut settings = CameraSettingsAuthority::default();
    let mut pending_surface_spawn = Some([3, 4]);

    apply_committed_control(
        CommittedControlEvent::PlayerMovementCorrection {
            sequence: 7,
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

    assert_eq!(view.eye_translation(), Vec3::new(27.5, 111.0, 91.5));
    assert!(view.rotation().abs_diff_eq(local_rotation, 0.0001));
    assert_eq!(pending_surface_spawn, None);
}

#[test]
fn committed_correction_offsets_only_the_third_person_view() {
    let correction = PlayerMovementCorrectionEvent {
        position: [12.0, 72.62, -8.0],
        delta: [0.0; 3],
        pitch: 10.0,
        yaw: 135.0,
        on_ground: true,
        tick: 91,
    };
    let mut view = LocalViewPose::default();
    let mut settings = CameraSettingsAuthority::default();
    let mut pending_surface_spawn = None;

    apply_committed_control(
        CommittedControlEvent::PlayerMovementCorrection {
            sequence: 12,
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

    let subject = Vec3::from_array(correction.position);
    let camera = perspective_pose(
        view.eye_translation(),
        view.rotation(),
        semantic_input::PerspectiveMode::ThirdPersonBack,
    );
    assert_eq!(view.eye_translation(), subject);
    assert!((camera.translation.distance(subject) - 4.0).abs() < 1.0e-5);
    assert_eq!(pending_surface_spawn, None);
}

#[test]
fn interaction_origin_consumes_and_invalidates_with_the_atomic_local_player_frame() {
    for perspective in [
        semantic_input::PerspectiveMode::FirstPerson,
        semantic_input::PerspectiveMode::ThirdPersonBack,
        semantic_input::PerspectiveMode::ThirdPersonFront,
    ] {
        let sample = frozen_local_player_sample_for(perspective);
        let mut frame = LocalPlayerFrameCarrier::default();
        frame.publish(sample.clone()).unwrap();
        let frozen = frame.snapshot().unwrap();
        let camera = CameraPose::new(sample.pose);
        let mut interaction = InteractionOriginSnapshot::default();

        assert!(interaction.outbound_ray().is_none());
        interaction.publish_from_local_player_frame(&frame);

        let outbound = interaction
            .outbound_ray()
            .expect("one atomic interaction/outbound ray");
        assert_eq!(outbound.session_generation(), frozen.session_generation());
        assert_eq!(outbound.fifo_sequence(), frozen.fifo_sequence());
        assert_eq!(outbound.physics_tick(), frozen.physics_tick());
        assert_eq!(outbound.pose_generation(), frozen.pose_generation());
        assert_eq!(outbound.perspective(), frozen.perspective());
        assert_eq!(
            outbound.world_collision_identity(),
            frozen.world_collision_identity()
        );
        assert_eq!(outbound.origin(), frozen.eye());
        assert!(outbound.direction().abs_diff_eq(frozen.direction(), 1.0e-6));
        assert!(
            interaction
                .outbound_ray_for_authority(
                    frozen.session_generation(),
                    frozen.fifo_sequence(),
                    frozen.physics_tick(),
                    frozen.pose_generation(),
                    frozen.world_collision_identity(),
                )
                .is_some()
        );
        assert!(
            interaction
                .outbound_ray_for_authority(
                    frozen.session_generation() + 1,
                    frozen.fifo_sequence(),
                    frozen.physics_tick(),
                    frozen.pose_generation(),
                    frozen.world_collision_identity(),
                )
                .is_none()
        );
        assert!(
            interaction
                .outbound_ray_for_authority(
                    frozen.session_generation(),
                    frozen.fifo_sequence() + 1,
                    frozen.physics_tick(),
                    frozen.pose_generation(),
                    frozen.world_collision_identity(),
                )
                .is_none()
        );
        assert!(
            interaction
                .outbound_ray_for_authority(
                    frozen.session_generation(),
                    frozen.fifo_sequence(),
                    frozen.physics_tick() + 1,
                    frozen.pose_generation(),
                    frozen.world_collision_identity(),
                )
                .is_none()
        );
        assert!(
            interaction
                .outbound_ray_for_authority(
                    frozen.session_generation(),
                    frozen.fifo_sequence(),
                    frozen.physics_tick(),
                    frozen.pose_generation() + 1,
                    frozen.world_collision_identity(),
                )
                .is_none()
        );
        let mismatched_world = sim::WorldCollisionIdentity::new(
            sim::CollisionRegistryIdentity {
                protocol: 1001,
                id_space: sim::CollisionIdSpace::Sequential,
                preg_sha256: [0x4b; 32],
            },
            [],
        )
        .unwrap();
        assert!(
            interaction
                .outbound_ray_for_authority(
                    frozen.session_generation(),
                    frozen.fifo_sequence(),
                    frozen.physics_tick(),
                    frozen.pose_generation(),
                    &mismatched_world,
                )
                .is_none()
        );
        if perspective != semantic_input::PerspectiveMode::FirstPerson {
            assert_ne!(outbound.origin(), camera.transform().translation);
        }

        for reset in [
            LocalPlayerFrameReset::Correction,
            LocalPlayerFrameReset::Session,
            LocalPlayerFrameReset::Dimension,
        ] {
            frame.reset(reset);
            interaction.publish_from_local_player_frame(&frame);
            assert!(
                interaction.outbound_ray().is_none(),
                "{reset:?} must invalidate the interaction/outbound ray"
            );
            frame.publish(sample.clone()).unwrap();
            interaction.publish_from_local_player_frame(&frame);
            assert!(interaction.outbound_ray().is_some());
        }
        interaction.invalidate();
        assert!(interaction.outbound_ray().is_none());
    }
}

#[test]
fn phase3_evidence_is_production_shaped_exact_bounded_and_dimension_correlated() {
    let frame = |physics_tick, pose_generation, dimension| Phase3EvidenceFrame {
        session_generation: 7,
        fifo_sequence: if dimension == 0 {
            41 + physics_tick
        } else {
            83 + physics_tick
        },
        physics_tick,
        pose_generation,
        dimension,
        network_position: [8.0 + physics_tick as f32, 72.62, -4.0],
        input_mode: semantic_input::InputMode::Touch,
        perspective: semantic_input::PerspectiveMode::ThirdPersonFront,
        camera_blocked: false,
        camera_fallback: false,
        local_avatar_visible: true,
        movement: [-0.25, 0.75],
        look_delta: [-0.5, 0.25],
        jump_held: true,
        outbound_authorized: true,
        outbox_depth: 2,
        outbox_drops: 0,
        free_camera_packet_count: 0,
        grounded_before_tick: true,
        grounded_after_tick: false,
        jump_started: true,
        jump_repeated: false,
        jump_released: false,
    };
    let mut evidence = Phase3EvidenceEmitter::default();

    let identity = Phase3EvidenceIdentity::new(
        "0123456789abcdef0123456789abcdef01234567",
        crate::args::Phase3Target::Zeqa,
        7,
        [0x11; 32],
        [0x22; 32],
        true,
    )
    .unwrap();
    let identity_markers = evidence.observe_identity(identity.clone());
    assert_eq!(identity_markers.len(), 1);
    let identity_json: serde_json::Value = serde_json::from_str(
        identity_markers[0]
            .strip_prefix("RUST_MCBE_PHASE3_IDENTITY=")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(identity_json.as_object().unwrap().len(), 15);
    assert_eq!(identity_json["schema"], "rust-mcbe-phase3-identity-v1");
    assert_eq!(identity_json["target"], "Zeqa");
    assert_eq!(identity_json["protocol"], 2168);
    assert_eq!(identity_json["session_generation"], 7);
    assert_eq!(identity_json["preg_sha256"], "11".repeat(32));
    assert_eq!(identity_json["breg_sha256"], "22".repeat(32));
    assert_eq!(identity_json["candidate_physics"], true);
    assert_eq!(identity_json["source_dirty"], false);
    assert_eq!(identity_json["run_id"], "0123456789abcdef0123456789abcdef");
    assert_eq!(identity_json["endpoint"], "127.0.0.1:19132");
    assert_eq!(identity_json["bridge_endpoint"], "127.0.0.1:19133");
    assert_eq!(identity_json["core_sha256"], "33".repeat(32));
    assert_eq!(identity_json["core_process_id"], 41);
    assert_eq!(identity_json["app_process_id"], 42);
    assert!(evidence.observe_identity(identity).is_empty());

    let first = evidence.observe(frame(41, 101, 0));
    assert_eq!(first.len(), 1);
    assert!(first[0].starts_with("RUST_MCBE_PHASE3_FRAME="));
    let json: serde_json::Value =
        serde_json::from_str(first[0].strip_prefix("RUST_MCBE_PHASE3_FRAME=").unwrap()).unwrap();
    assert_eq!(json.as_object().unwrap().len(), 24);
    assert_eq!(json["schema"], "rust-mcbe-phase3-frame-v2");
    assert_eq!(json["physics_tick"], 41);
    assert_eq!(json["dimension"], 0);
    assert_eq!(
        json["network_position"],
        serde_json::json!([49.0_f32, 72.62_f32, -4.0_f32])
    );

    let duplicate = evidence.observe(frame(41, 102, 0));
    assert_eq!(duplicate.len(), 1);
    assert!(duplicate[0].starts_with("RUST_MCBE_PHASE3_VIOLATION="));

    let mut transition_evidence = Phase3EvidenceEmitter::default();
    assert_eq!(transition_evidence.observe(frame(41, 101, 0)).len(), 1);
    transition_evidence.note_event(Phase3EvidenceEventKind::Dimension);
    let transitioned = transition_evidence.observe(frame(0, 103, 1));
    assert_eq!(transitioned.len(), 2);
    assert!(transitioned[0].starts_with("RUST_MCBE_PHASE3_FRAME="));
    assert!(transitioned[1].starts_with("RUST_MCBE_PHASE3_EVENT="));
    let event: serde_json::Value = serde_json::from_str(
        transitioned[1]
            .strip_prefix("RUST_MCBE_PHASE3_EVENT=")
            .unwrap(),
    )
    .unwrap();
    assert_eq!(event.as_object().unwrap().len(), 7);
    assert_eq!(event["event_sequence"], 0);
    assert_eq!(event["kind"], "dimension");
    assert_eq!(event["physics_tick"], 0);
    assert_eq!(event["dimension"], 1);
}

#[test]
fn phase3_evidence_emits_one_frame_for_every_completed_catch_up_tick() {
    let base = Phase3EvidenceFrame {
        session_generation: 7,
        fifo_sequence: 88,
        physics_tick: 0,
        pose_generation: 101,
        dimension: 0,
        network_position: [8.0, 72.62, -4.0],
        input_mode: semantic_input::InputMode::KeyboardMouse,
        perspective: semantic_input::PerspectiveMode::FirstPerson,
        camera_blocked: false,
        camera_fallback: false,
        local_avatar_visible: false,
        movement: [0.0; 2],
        look_delta: [0.25, -0.5],
        jump_held: false,
        outbound_authorized: true,
        outbox_depth: 3,
        outbox_drops: 0,
        free_camera_packet_count: 0,
        grounded_before_tick: false,
        grounded_after_tick: false,
        jump_started: false,
        jump_repeated: false,
        jump_released: false,
    };
    let ticks = [101, 102, 103].map(|tick| PhysicsTickEvidence {
        session_generation: 7,
        tick,
        network_position: [8.0, 72.62, tick as f32],
        input_mode: protocol::PlayerInputMode::GamePad,
        movement: [0.0, 1.0],
        jump_held: true,
        grounded_before_tick: true,
        grounded_after_tick: false,
        jump_started: true,
        jump_repeated: tick != 101,
        jump_released: false,
        context: PhysicsTickEvidenceContext {
            fifo_sequence: base.fifo_sequence,
            pose_generation: base.pose_generation,
            dimension: base.dimension,
            perspective: base.perspective,
            camera_blocked: base.camera_blocked,
            camera_fallback: base.camera_fallback,
            local_avatar_visible: base.local_avatar_visible,
            look_delta: base.look_delta,
            outbound_authorized: base.outbound_authorized,
            outbox_depth: base.outbox_depth,
            outbox_drops: base.outbox_drops,
            free_camera_packet_count: base.free_camera_packet_count,
        },
    });
    let mut evidence = Phase3EvidenceEmitter::default();

    let markers = evidence.observe_completed_ticks(&ticks);

    assert_eq!(markers.len(), 3);
    assert_eq!(
        markers
            .iter()
            .map(|marker| {
                let json: serde_json::Value =
                    serde_json::from_str(marker.strip_prefix("RUST_MCBE_PHASE3_FRAME=").unwrap())
                        .unwrap();
                json["physics_tick"].as_u64().unwrap()
            })
            .collect::<Vec<_>>(),
        [101, 102, 103]
    );
}

#[test]
fn phase3_correction_evidence_records_only_bounded_successful_replay_and_snap_outcomes() {
    let mut evidence = Phase3EvidenceEmitter::default();
    evidence.note_correction(
        PhysicsCorrectionOutcome::Replayed {
            corrected_tick: 40,
            replayed_ticks: 2,
        },
        3.5,
    );
    evidence.note_correction(PhysicsCorrectionOutcome::Snapped { tick: 43 }, 1.25);

    let markers = evidence.observe(Phase3EvidenceFrame {
        session_generation: 7,
        fifo_sequence: 88,
        physics_tick: 44,
        pose_generation: 101,
        dimension: 0,
        network_position: [8.0, 72.62, -4.0],
        input_mode: semantic_input::InputMode::KeyboardMouse,
        perspective: semantic_input::PerspectiveMode::FirstPerson,
        camera_blocked: false,
        camera_fallback: false,
        local_avatar_visible: false,
        movement: [0.0; 2],
        look_delta: [0.0; 2],
        jump_held: false,
        outbound_authorized: true,
        outbox_depth: 1,
        outbox_drops: 0,
        free_camera_packet_count: 0,
        grounded_before_tick: false,
        grounded_after_tick: false,
        jump_started: false,
        jump_repeated: false,
        jump_released: false,
    });
    let corrections = markers
        .iter()
        .filter_map(|marker| marker.strip_prefix("RUST_MCBE_PHASE3_EVENT="))
        .map(|json| serde_json::from_str::<serde_json::Value>(json).unwrap())
        .collect::<Vec<_>>();

    assert_eq!(corrections.len(), 2);
    assert_eq!(corrections[0]["correction_outcome"], "replayed");
    assert_eq!(corrections[0]["corrected_tick"], 40);
    assert_eq!(corrections[0]["replayed_ticks"], 2);
    assert_eq!(corrections[0]["correction_magnitude"], 3.5);
    assert_eq!(corrections[1]["correction_outcome"], "snapped");
    assert_eq!(corrections[1]["corrected_tick"], 43);
    assert_eq!(corrections[1]["replayed_ticks"], 0);
    assert_eq!(corrections[1]["correction_magnitude"], 1.25);
}

#[test]
fn phase3_evidence_identity_rejects_unattributable_builds_and_hashes() {
    for invalid in [
        Phase3EvidenceIdentity::new(
            "not-a-commit",
            crate::args::Phase3Target::Bds,
            7,
            [0x11; 32],
            [0x22; 32],
            true,
        ),
        Phase3EvidenceIdentity::new(
            "0123456789abcdef0123456789abcdef01234567",
            crate::args::Phase3Target::Bds,
            0,
            [0x11; 32],
            [0x22; 32],
            true,
        ),
        Phase3EvidenceIdentity::new(
            "0123456789abcdef0123456789abcdef01234567",
            crate::args::Phase3Target::Bds,
            7,
            [0; 32],
            [0x22; 32],
            true,
        ),
    ] {
        assert!(invalid.is_err());
    }
}

#[test]
fn phase3_candidate_identity_rejects_dirty_or_unattributed_builds() {
    assert!(validate_phase3_build_source(Some("false")).is_ok());
    for dirty in [None, Some("true"), Some("False"), Some("")] {
        assert!(validate_phase3_build_source(dirty).is_err());
    }
}

#[test]
fn phase3_evidence_fails_closed_on_invalid_or_unauthorized_frames() {
    let invalid = Phase3EvidenceFrame {
        session_generation: 7,
        fifo_sequence: 41,
        physics_tick: 1,
        pose_generation: 1,
        dimension: 0,
        network_position: [8.0, 72.62, -4.0],
        input_mode: semantic_input::InputMode::KeyboardMouse,
        perspective: semantic_input::PerspectiveMode::FirstPerson,
        camera_blocked: false,
        camera_fallback: false,
        local_avatar_visible: false,
        movement: [f32::NAN, 0.0],
        look_delta: [0.0; 2],
        jump_held: false,
        outbound_authorized: true,
        outbox_depth: 0,
        outbox_drops: 0,
        free_camera_packet_count: 0,
        grounded_before_tick: false,
        grounded_after_tick: false,
        jump_started: false,
        jump_repeated: false,
        jump_released: false,
    };
    let mut evidence = Phase3EvidenceEmitter::default();
    let violation = evidence.observe(invalid);
    assert_eq!(violation.len(), 1);
    assert!(violation[0].starts_with("RUST_MCBE_PHASE3_VIOLATION="));

    for frame in [
        Phase3EvidenceFrame {
            movement: [0.0; 2],
            network_position: [f32::NAN, 72.62, -4.0],
            ..invalid
        },
        Phase3EvidenceFrame {
            movement: [0.0; 2],
            outbound_authorized: false,
            ..invalid
        },
        Phase3EvidenceFrame {
            movement: [0.0; 2],
            free_camera_packet_count: 1,
            ..invalid
        },
        Phase3EvidenceFrame {
            movement: [0.0; 2],
            camera_blocked: true,
            camera_fallback: true,
            ..invalid
        },
    ] {
        let markers = Phase3EvidenceEmitter::default().observe(frame);
        assert_eq!(markers.len(), 1);
        assert!(markers[0].starts_with("RUST_MCBE_PHASE3_VIOLATION="));
    }
}

#[test]
fn phase3_invalid_correction_and_non_monotonic_tick_emit_durable_violations() {
    let frame = |tick| Phase3EvidenceFrame {
        session_generation: 7,
        fifo_sequence: tick,
        physics_tick: tick,
        pose_generation: tick + 1,
        dimension: 0,
        network_position: [8.0, 72.62, tick as f32],
        input_mode: semantic_input::InputMode::KeyboardMouse,
        perspective: semantic_input::PerspectiveMode::FirstPerson,
        camera_blocked: false,
        camera_fallback: false,
        local_avatar_visible: false,
        movement: [0.0; 2],
        look_delta: [0.0; 2],
        jump_held: false,
        outbound_authorized: true,
        outbox_depth: 0,
        outbox_drops: 0,
        free_camera_packet_count: 0,
        grounded_before_tick: false,
        grounded_after_tick: false,
        jump_started: false,
        jump_repeated: false,
        jump_released: false,
    };
    let mut invalid_correction = Phase3EvidenceEmitter::default();
    invalid_correction.note_correction(PhysicsCorrectionOutcome::Snapped { tick: 1 }, f32::NAN);
    let markers = invalid_correction.observe(frame(1));
    assert_eq!(markers.len(), 1);
    assert!(markers[0].contains("invalid_correction"));

    let mut non_monotonic = Phase3EvidenceEmitter::default();
    assert_eq!(non_monotonic.observe(frame(1)).len(), 1);
    let markers = non_monotonic.observe(frame(1));
    assert_eq!(markers.len(), 1);
    assert!(markers[0].contains("non_monotonic_frame"));
    assert!(non_monotonic.observe(frame(2)).is_empty());
}

#[test]
fn phase3_terminal_binds_candidate_and_free_camera_packet_silence() {
    let identity = |candidate| {
        Phase3EvidenceIdentity::new(
            "0123456789abcdef0123456789abcdef01234567",
            crate::args::Phase3Target::Bds,
            7,
            [0x11; 32],
            [0x22; 32],
            candidate,
        )
        .unwrap()
    };
    let mut candidate = Phase3EvidenceEmitter::default();
    let markers = candidate.observe_terminal(
        identity(true),
        MovementSource::Physics,
        3,
        0,
        0,
        MovementOutboxReconciliation::Drained,
    );
    assert_eq!(markers.len(), 2);
    assert!(markers[0].starts_with("RUST_MCBE_PHASE3_IDENTITY="));
    assert!(markers[1].starts_with("RUST_MCBE_PHASE3_TERMINAL="));
    assert!(
        candidate
            .observe_terminal(
                identity(true),
                MovementSource::Physics,
                3,
                0,
                0,
                MovementOutboxReconciliation::Drained,
            )
            .is_empty()
    );

    let mut free = Phase3EvidenceEmitter::default();
    let markers = free.observe_terminal(
        identity(false),
        MovementSource::FreeCamera,
        0,
        0,
        0,
        MovementOutboxReconciliation::NotAuthoritative,
    );
    assert_eq!(markers.len(), 2);
    assert!(markers[1].contains("\"source\":\"FreeCamera\""));
    assert!(markers[1].contains("\"pending_outbox_depth\":0"));
    assert!(markers[1].contains("\"outbox_reconciliation\":\"NotAuthoritative\""));

    let mut leaked = Phase3EvidenceEmitter::default();
    let markers = leaked.observe_terminal(
        identity(false),
        MovementSource::FreeCamera,
        0,
        1,
        0,
        MovementOutboxReconciliation::NotAuthoritative,
    );
    assert!(
        markers
            .iter()
            .any(|marker| marker.starts_with("RUST_MCBE_PHASE3_VIOLATION="))
    );

    let mut full = Phase3EvidenceEmitter::default();
    let markers = full.observe_terminal(
        identity(true),
        MovementSource::Physics,
        3,
        0,
        1,
        MovementOutboxReconciliation::FullRestored,
    );
    assert!(
        markers
            .iter()
            .any(|marker| marker.contains("terminal_outbox_not_drained"))
    );
    assert!(markers.iter().any(|marker| {
        marker.contains("\"pending_outbox_depth\":1")
            && marker.contains("\"outbox_reconciliation\":\"FullRestored\"")
    }));
}

#[test]
fn phase3_terminal_fails_closed_when_a_correction_has_no_following_frame() {
    let identity = Phase3EvidenceIdentity::new(
        "0123456789abcdef0123456789abcdef01234567",
        crate::args::Phase3Target::Bds,
        7,
        [0x11; 32],
        [0x22; 32],
        true,
    )
    .unwrap();
    let mut evidence = Phase3EvidenceEmitter::default();
    evidence.note_correction(PhysicsCorrectionOutcome::Snapped { tick: 44 }, 1.25);

    let markers = evidence.observe_terminal(
        identity,
        MovementSource::Physics,
        3,
        0,
        0,
        MovementOutboxReconciliation::Drained,
    );

    assert!(markers.iter().any(|marker| {
        marker.starts_with("RUST_MCBE_PHASE3_VIOLATION=")
            && marker.contains("terminal_pending_correction")
    }));
    assert!(
        markers
            .iter()
            .all(|marker| !marker.starts_with("RUST_MCBE_PHASE3_EVENT="))
    );
    assert!(
        markers
            .last()
            .unwrap()
            .starts_with("RUST_MCBE_PHASE3_TERMINAL=")
    );
}

#[test]
fn phase3_authority_fault_evidence_survives_deauthorization_and_is_bounded() {
    let fault = |next_tick| PhysicsAuthorityFaultRecord {
        session_generation: 7,
        fault: PhysicsAuthorityFault::OutboxOverflow,
        next_tick,
        pending_count: crate::movement::OUTBOX_CAPACITY,
    };
    let mut evidence = Phase3EvidenceEmitter::default();
    let marker = evidence.observe_authority_fault(fault(41));
    assert_eq!(marker.len(), 2);
    let json: serde_json::Value =
        serde_json::from_str(marker[0].strip_prefix("RUST_MCBE_PHASE3_EVENT=").unwrap()).unwrap();
    assert_eq!(json["kind"], "authority_fault");
    assert_eq!(json["session_generation"], 7);
    assert_eq!(json["fault"], "outbox_overflow");
    assert_eq!(json["next_tick"], 41);
    assert_eq!(json["pending_count"], crate::movement::OUTBOX_CAPACITY);
    assert!(marker[1].starts_with("RUST_MCBE_PHASE3_VIOLATION="));

    let mut emitted = marker.len();
    for tick in 42..42 + u64::try_from(MAX_PHASE3_FAULT_RECORDS + 4).unwrap() {
        emitted += evidence.observe_authority_fault(fault(tick)).len();
    }
    assert_eq!(emitted, MAX_PHASE3_FAULT_RECORDS + 1);
}

#[test]
fn phase3_simulation_fault_evidence_preserves_frame_and_error_context() {
    let fault = PhysicsAuthorityFaultRecord {
        session_generation: 7,
        fault: PhysicsAuthorityFault::PhysicsSimulationError {
            due: 5,
            tick_index: 2,
            error: sim::SimulationError::NonFiniteInput { field: "forward" },
        },
        next_tick: 41,
        pending_count: 3,
    };
    let mut evidence = Phase3EvidenceEmitter::default();

    let markers = evidence.observe_authority_fault(fault);
    let json: serde_json::Value =
        serde_json::from_str(markers[0].strip_prefix("RUST_MCBE_PHASE3_EVENT=").unwrap())
            .unwrap();
    assert_eq!(json["fault"], "physics_simulation_error");
    assert_eq!(json["detail"]["due"], 5);
    assert_eq!(json["detail"]["tick_index"], 2);
    assert_eq!(
        json["detail"]["simulation_error"]["kind"],
        "non_finite_input"
    );
    assert_eq!(json["detail"]["simulation_error"]["field"], "forward");
    assert_eq!(
        json["detail"]["simulation_error"]["message"],
        "movement input field forward is not finite"
    );
}

#[test]
fn phase3_tick_overflow_evidence_preserves_due_and_dropped_counts() {
    let fault = PhysicsAuthorityFaultRecord {
        session_generation: 7,
        fault: PhysicsAuthorityFault::PhysicsTickOverflow {
            due: 200,
            dropped: 192,
        },
        next_tick: 41,
        pending_count: 0,
    };
    let mut evidence = Phase3EvidenceEmitter::default();

    let markers = evidence.observe_authority_fault(fault);
    let json: serde_json::Value =
        serde_json::from_str(markers[0].strip_prefix("RUST_MCBE_PHASE3_EVENT=").unwrap())
            .unwrap();
    assert_eq!(json["fault"], "physics_tick_overflow");
    assert_eq!(json["detail"]["due"], 200);
    assert_eq!(json["detail"]["dropped"], 192);
}

#[test]
fn phase3_evidence_producer_stays_bounded_after_record_limits() {
    let frame = |physics_tick| Phase3EvidenceFrame {
        session_generation: 7,
        fifo_sequence: physics_tick,
        physics_tick,
        pose_generation: physics_tick + 1,
        dimension: 0,
        network_position: [8.0, 72.62, physics_tick as f32],
        input_mode: semantic_input::InputMode::KeyboardMouse,
        perspective: semantic_input::PerspectiveMode::FirstPerson,
        camera_blocked: false,
        camera_fallback: false,
        local_avatar_visible: false,
        movement: [0.0; 2],
        look_delta: [0.0; 2],
        jump_held: false,
        outbound_authorized: true,
        outbox_depth: 0,
        outbox_drops: 0,
        free_camera_packet_count: 0,
        grounded_before_tick: false,
        grounded_after_tick: false,
        jump_started: false,
        jump_repeated: false,
        jump_released: false,
    };
    let mut frame_evidence = Phase3EvidenceEmitter::default();
    let mut emitted_frames = 0;
    for tick in 0..u64::try_from(MAX_PHASE3_FRAME_RECORDS + 4).unwrap() {
        emitted_frames += frame_evidence.observe(frame(tick)).len();
    }
    assert_eq!(emitted_frames, MAX_PHASE3_FRAME_RECORDS + 1);

    let mut event_evidence = Phase3EvidenceEmitter::default();
    let mut emitted_events = 0;
    for tick in 0..u64::try_from(MAX_PHASE3_EVENT_RECORDS + 4).unwrap() {
        event_evidence.note_event(Phase3EvidenceEventKind::Session);
        emitted_events += event_evidence
            .observe(frame(tick))
            .into_iter()
            .filter(|marker| marker.starts_with("RUST_MCBE_PHASE3_EVENT="))
            .count();
    }
    assert_eq!(emitted_events, MAX_PHASE3_EVENT_RECORDS);
}
