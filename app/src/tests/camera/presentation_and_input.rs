#[test]
fn local_avatar_publishes_only_frozen_visibility_without_owning_the_render_arena() {
    let mut presentation = LocalAvatarPresentation::default();
    presentation.begin_session(7, 91);
    let mut frame = LocalPlayerFrameCarrier::default();
    frame.publish(frozen_local_player_sample()).unwrap();
    let mut visibility = LocalAvatarVisibilityCarrier::default();

    presentation.publish_visibility(frame.snapshot().unwrap(), &mut visibility);

    let frozen = visibility
        .snapshot()
        .expect("third-person local avatar visibility");
    assert_eq!(frozen.session_generation(), 7);
    assert_eq!(frozen.runtime_id(), 91);
    assert_eq!(
        frozen.pose_generation(),
        frame.snapshot().unwrap().pose_generation()
    );
    assert!(frozen.visible());
    assert_eq!(frozen.eye(), frame.snapshot().unwrap().eye());
    assert_eq!(frozen.rotation(), frame.snapshot().unwrap().rotation());

    frame
        .publish(frozen_local_player_sample_for(
            semantic_input::PerspectiveMode::FirstPerson,
        ))
        .unwrap();
    presentation.publish_visibility(frame.snapshot().unwrap(), &mut visibility);
    assert!(!visibility.snapshot().unwrap().visible());

    presentation.clear();
    presentation.publish_visibility(frame.snapshot().unwrap(), &mut visibility);
    assert!(visibility.snapshot().is_none());

    let local_player_source = include_str!("../../local_player.rs");
    let network_source = include_str!("../../runtime/network.rs");
    assert!(!local_player_source.contains("ActorRenderSource"));
    assert!(!local_player_source.contains("MAX_RENDERED_PLAYERS"));
    assert!(!network_source.contains("reconcile_sources"));
}

#[test]
fn actor_culling_precedes_the_remote_cap_and_preserves_visible_high_id_and_local_avatar() {
    let mut presentation = LocalAvatarPresentation::default();
    presentation.begin_session(7, 91);
    let mut local_frame = LocalPlayerFrameCarrier::default();
    let mut sample = frozen_local_player_sample();
    sample.eye = Vec3::new(0.0, 65.62, 0.0);
    sample.rotation = Quat::IDENTITY;
    sample.pose = perspective_pose(sample.eye, sample.rotation, sample.perspective);
    local_frame.publish(sample).unwrap();
    let mut local_visibility = LocalAvatarVisibilityCarrier::default();
    presentation.publish_visibility(local_frame.snapshot().unwrap(), &mut local_visibility);

    let source = |runtime_id: u64, position: [f32; 3]| ActorRenderSource {
        runtime_id,
        unique_id: i64::try_from(runtime_id).unwrap(),
        spawn_revision: 1,
        movement_revision: 1,
        previous_position: position,
        previous_pitch_degrees: 0.0,
        previous_yaw_degrees: 0.0,
        previous_head_yaw_degrees: 0.0,
        position,
        pitch_degrees: 0.0,
        yaw_degrees: 0.0,
        head_yaw_degrees: 0.0,
        teleported: false,
        skin: None,
    };
    let mut remote_sources = (1..=u64::try_from(MAX_RENDERED_PLAYERS + 1).unwrap())
        .map(|runtime_id| source(runtime_id, [500.0, 64.0, 0.0]))
        .collect::<Vec<_>>();
    remote_sources.push(source(999, [1.0, 64.0, 0.0]));
    let cull_view = ActorCullView {
        clip_from_world: Mat4::from_translation(Vec3::new(0.0, -64.0, 0.0)),
        camera_position: Vec3::new(0.0, 65.0, 0.0),
        max_distance: 192.0,
    };
    let mut scene = ActorRenderScene::default();

    let frame = update_actor_render_scene(
        &mut scene,
        1.0,
        Some(cull_view),
        remote_sources,
        local_visibility.snapshot(),
    );

    assert_eq!(
        frame
            .instances
            .iter()
            .map(|actor| actor.runtime_id)
            .collect::<Vec<_>>(),
        vec![999, 91],
        "Phase 4 must cull and cap remote actors before consuming the frozen local carrier",
    );
}

#[test]
fn app_semantic_runtime_preserves_keyboard_controller_touch_equivalence() {
    let frames = [
        DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                keys: vec![0x1a, 0x2c],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        },
        DeviceFrame {
            controllers: vec![ControllerFrame {
                device_id: 1,
                axes: [0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                buttons: vec![0],
                ..ControllerFrame::default()
            }],
            ..DeviceFrame::default()
        },
        DeviceFrame {
            touches: vec![
                TouchContact {
                    contact_id: 1,
                    activity_sequence: 0,
                    position: [0.25, 0.5],
                    delta: [0.0, 0.0],
                    hit_id: None,
                },
                TouchContact {
                    contact_id: 2,
                    activity_sequence: 0,
                    position: [0.75, 0.75],
                    delta: [0.0, 0.0],
                    hit_id: Some(semantic_input::touch::JUMP),
                },
            ],
            ..DeviceFrame::default()
        },
    ];
    let projections = frames.map(|frame| {
        let mut runtime = SemanticInputRuntime::default();
        let snapshot = runtime.route_and_finalize(frame).unwrap();
        (
            snapshot.movement,
            snapshot.phases[Action::Jump as usize].pressed,
            snapshot.phases[Action::Jump as usize].held,
        )
    });
    assert_eq!(projections[0], projections[1]);
    assert_eq!(projections[1], projections[2]);
}

#[test]
fn semantic_controller_activity_ignores_drift_without_refiring_held_buttons() {
    let mut runtime = SemanticInputRuntime::default();
    let controller_frame = |drift| DeviceFrame {
        controllers: vec![
            ControllerFrame {
                device_id: 7,
                buttons: vec![0],
                ..ControllerFrame::default()
            },
            ControllerFrame {
                device_id: 8,
                axes: [drift, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
                ..ControllerFrame::default()
            },
        ],
        ..DeviceFrame::default()
    };

    let controller_owned = runtime.route_and_finalize(controller_frame(0.0)).unwrap();
    assert_eq!(
        controller_owned.input_mode,
        semantic_input::InputMode::GamePad
    );
    assert!(controller_owned.phases[Action::Jump as usize].pressed);
    assert!(controller_owned.phases[Action::Jump as usize].held);

    let keyboard = KeyboardMouseFrame {
        keys: vec![0x1a],
        ..KeyboardMouseFrame::default()
    };
    let mut keyboard_frame = controller_frame(0.0);
    keyboard_frame.keyboard_mouse = Some(keyboard.clone());
    let keyboard_owned = runtime.route_and_finalize(keyboard_frame).unwrap();
    assert_eq!(
        keyboard_owned.input_mode,
        semantic_input::InputMode::KeyboardMouse
    );
    assert_eq!(keyboard_owned.movement, [0.0, 1.0]);

    let mut sub_deadzone_drift = controller_frame(0.05);
    sub_deadzone_drift.keyboard_mouse = Some(keyboard.clone());
    let drift = runtime.route_and_finalize(sub_deadzone_drift).unwrap();
    assert_eq!(drift.input_mode, semantic_input::InputMode::KeyboardMouse);
    assert_eq!(drift.movement, [0.0, 1.0]);
    assert!(!drift.phases[Action::Jump as usize].pressed);

    let mut crossed_deadzone = controller_frame(0.2);
    crossed_deadzone.keyboard_mouse = Some(keyboard);
    let crossing = runtime.route_and_finalize(crossed_deadzone).unwrap();
    assert_eq!(crossing.input_mode, semantic_input::InputMode::GamePad);
    assert!(crossing.movement[0] > 0.0);
    assert!(!crossing.phases[Action::Jump as usize].pressed);
    assert!(crossing.phases[Action::Jump as usize].held);
}

#[test]
fn semantic_runtime_wires_context_bindings_authority_and_release_at_finalize() {
    let mut runtime = SemanticInputRuntime::default();
    let held_jump = DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            keys: vec![0x2c],
            ..KeyboardMouseFrame::default()
        }),
        ..DeviceFrame::default()
    };
    assert!(runtime.route_and_finalize(held_jump).unwrap().phases[Action::Jump as usize].held);

    let generation = NonZeroU64::new(9).unwrap();
    runtime.set_context(InputContext::UiFocused);
    runtime
        .replace_bindings(ControlSettings::default())
        .unwrap();
    runtime.replace_authority(generation);
    runtime.release_all(ReleaseReason::SessionReplaced);
    let released = runtime.route_and_finalize(DeviceFrame::default()).unwrap();

    assert_eq!(released.authority_generation, generation);
    assert!(released.phases[Action::Jump as usize].released);
    assert_eq!(
        released.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::SessionReplaced)
    );
}

#[test]
fn semantic_authority_tracks_ui_settings_session_and_dimension_transitions_in_production_order() {
    let mut runtime = SemanticInputRuntime::default();
    let mut ui = UiRuntime::new(1);
    let controls = ControlSettings::default();
    let held_jump = || DeviceFrame {
        keyboard_mouse: Some(KeyboardMouseFrame {
            keys: vec![0x2c],
            ..KeyboardMouseFrame::default()
        }),
        ..DeviceFrame::default()
    };
    let authority =
        |context, controls_generation, session_generation, dimension| SemanticInputAuthorityFrame {
            context,
            controls_generation,
            controls: controls.clone(),
            session_generation: NonZeroU64::new(session_generation).unwrap(),
            dimension,
        };

    runtime
        .synchronize_authority(authority(InputContext::Gameplay, 1, 1, 0))
        .unwrap();
    assert!(runtime.route_and_finalize(held_jump()).unwrap().phases[Action::Jump as usize].held);

    let ui_transition = ui.open_chat();
    runtime
        .synchronize_authority(authority(ui_transition.requested_input_context(), 1, 1, 0))
        .unwrap();
    let ui_release = runtime.route_and_finalize(DeviceFrame::default()).unwrap();
    assert_eq!(
        ui_release.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::UiFocusTaken),
    );

    runtime
        .synchronize_authority(authority(InputContext::Gameplay, 1, 1, 0))
        .unwrap();
    assert!(runtime.route_and_finalize(held_jump()).unwrap().phases[Action::Jump as usize].held);
    runtime
        .synchronize_authority(authority(InputContext::Gameplay, 1, 2, 0))
        .unwrap();
    let session_release = runtime.route_and_finalize(DeviceFrame::default()).unwrap();
    assert_eq!(
        session_release.authority_generation,
        NonZeroU64::new(2).unwrap()
    );
    assert_eq!(
        session_release.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::SessionReplaced),
    );

    assert!(runtime.route_and_finalize(held_jump()).unwrap().phases[Action::Jump as usize].held);
    runtime
        .synchronize_authority(authority(InputContext::Gameplay, 1, 2, 1))
        .unwrap();
    let dimension_release = runtime.route_and_finalize(DeviceFrame::default()).unwrap();
    assert_eq!(
        dimension_release.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::DimensionReplaced),
    );

    assert!(runtime.route_and_finalize(held_jump()).unwrap().phases[Action::Jump as usize].held);
    runtime
        .synchronize_authority(authority(InputContext::Gameplay, 2, 2, 1))
        .unwrap();
    let binding_release = runtime.route_and_finalize(DeviceFrame::default()).unwrap();
    assert_eq!(
        binding_release.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::BindingChanged),
    );
}

#[test]
fn pending_mouse_motion_cannot_cross_session_authority_in_production_order() {
    let mut runtime = SemanticInputRuntime::default();
    let authority = |session_generation| SemanticInputAuthorityFrame {
        context: InputContext::Gameplay,
        controls_generation: 1,
        controls: ControlSettings::default(),
        session_generation: NonZeroU64::new(session_generation).unwrap(),
        dimension: 0,
    };

    runtime.route_device_frame(DeviceFrame::default()).unwrap();
    runtime.synchronize_authority(authority(1)).unwrap();
    assert_eq!(
        runtime.finalize_routed_input().unwrap().look_delta,
        [0.0, 0.0]
    );

    runtime
        .route_device_frame(DeviceFrame {
            keyboard_mouse: Some(KeyboardMouseFrame {
                mouse_motion: [8.0, -4.0],
                ..KeyboardMouseFrame::default()
            }),
            ..DeviceFrame::default()
        })
        .unwrap();
    runtime.synchronize_authority(authority(2)).unwrap();
    let transitioned = runtime.finalize_routed_input().unwrap();

    assert_eq!(transitioned.look_delta, [0.0, 0.0]);
    assert_eq!(
        transitioned.authority_generation,
        NonZeroU64::new(2).unwrap()
    );
}

#[test]
fn semantic_runtime_synthesizes_controller_disconnect_and_releases_stale_touch_targets() {
    let mut runtime = SemanticInputRuntime::default();
    let held_controller_jump = DeviceFrame {
        controllers: vec![ControllerFrame {
            device_id: 7,
            buttons: vec![0],
            ..ControllerFrame::default()
        }],
        ..DeviceFrame::default()
    };
    assert!(
        runtime
            .route_and_finalize(held_controller_jump)
            .unwrap()
            .phases[Action::Jump as usize]
            .held
    );

    let disconnected = runtime.route_and_finalize(DeviceFrame::default()).unwrap();
    assert!(disconnected.phases[Action::Jump as usize].released);
    assert_eq!(
        disconnected.release_reasons[Action::Jump as usize],
        Some(ReleaseReason::ControllerDisconnected)
    );

    let mut targets = SemanticTouchTargets::default();
    targets.set(1, semantic_input::touch::JUMP);
    targets.set(2, semantic_input::touch::USE);
    targets.retain_active_contacts([2]);
    assert_eq!(targets.target(1), None);
    assert_eq!(targets.target(2), Some(semantic_input::touch::USE));
    targets.release_all();
    assert_eq!(targets.target(2), None);

    let physical_source = include_str!("../../semantic_controls/physical.rs");
    assert!(physical_source.contains("ResMut<'w, SemanticTouchTargets>"));
    assert!(physical_source.contains("retain_active_contacts"));
    let touch_source = include_str!("../../ui_runtime/gameplay_touch.rs");
    assert!(touch_source.contains("PRODUCTION_TOUCH_LAYOUT_AVAILABLE: bool = false"));
    assert!(touch_source.contains("targets.release_all()"));
    assert!(!touch_source.contains("targets.set("));
}
