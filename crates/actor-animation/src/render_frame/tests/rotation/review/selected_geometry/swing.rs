use super::*;

/// Poses a selected player geometry from camera values assigned by its render controller.
fn camera_write_geometry(body_reads_camera: bool, chained: bool) -> CompiledEntityAssets {
    let mut compiled = swelling_geometry(true);
    compiled.symbols[0].identifier = "minecraft:player".into();
    let angle = symbol(&mut compiled, MolangSymbolKind::Variable, "variable.angle");
    let attack = symbol(
        &mut compiled,
        MolangSymbolKind::Variable,
        "variable.attack_time",
    );
    let layer_angle = symbol(
        &mut compiled,
        MolangSymbolKind::Variable,
        "variable.layer_angle",
    );
    let camera = compiled
        .molang_symbols
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == "query.camera_rotation")
        .unwrap() as u32;
    let attack = expression(&mut compiled, &[MolangOp::LoadVariable(attack)], 1);
    let read = expression(
        &mut compiled,
        &[MolangOp::LoadVariable(if chained {
            layer_angle
        } else {
            angle
        })],
        1,
    );
    let copy = expression(
        &mut compiled,
        &[
            MolangOp::LoadVariable(angle),
            MolangOp::StoreVariable(layer_angle),
            MolangOp::Push(crate::test_support::scalar(1.0)),
        ],
        1,
    );
    let write = expression(
        &mut compiled,
        &[
            MolangOp::Push(crate::test_support::scalar(0.0)),
            MolangOp::CallQuery(MolangCall {
                symbol: camera,
                arguments: 1,
            }),
            MolangOp::StoreVariable(angle),
            MolangOp::Push(crate::test_support::scalar(1.0)),
        ],
        1,
    );
    compiled.render.layers[0].color = Some([write, copy, copy, copy]);
    compiled.animation_keyframes[0].expressions =
        [Some(attack), body_reads_camera.then_some(read), None];
    let selected_clip = &compiled.animation_clips[1];
    let selected_channel = &compiled.animation_channels[selected_clip.first_channel as usize];
    compiled.animation_keyframes[selected_channel.first_keyframe as usize].expressions =
        [Some(attack), Some(read), None];
    compiled
}

#[test]
fn selected_geometry_keeps_live_camera_assignments_during_local_swings() {
    for body_reads_camera in [false, true] {
        for chained in [false, true] {
            let context = ActorTickContext {
                is_local: true,
                is_local_player: true,
                camera_rotation: [7.0, 0.0],
                combat_visuals: render_api::CombatVisuals::Modern,
                ..Default::default()
            };
            let (mut store, actors) = fixture_with(
                camera_write_geometry(body_reads_camera, chained),
                0.0,
                context.clone(),
            );
            for _ in 0..2 {
                store.advance_tick(&actors, None, Some(1), true, true, |_| context.clone());
            }
            let rig = store.get(1).unwrap();
            let completed = (rig.previous.to_vec(), rig.current.to_vec());
            let authored = authored_state(&store);
            for camera_pitch in [35.0, -20.0, 7.0] {
                for physics_alpha in [0.0, 0.25, 0.75, 1.0, 0.25] {
                    let progress = LocalSwingProgress {
                        bedrock: [0.2, 0.6],
                        java: [0.2, 0.6],
                        frame_alpha: Some(physics_alpha),
                    };
                    store.sync_local_swing(1, progress);
                    let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
                    let sampled = store
                        .render_layers(
                            &actors[&1],
                            0.5,
                            [camera_pitch, 0.0],
                            [0.0; 3],
                            &mut budget,
                            false,
                        )
                        .unwrap();
                    let layer = &sampled.render[0];
                    assert_eq!(layer.geometry, Some(1));
                    for bones in [&layer.previous_pose, &layer.pose] {
                        assert_angles(
                            bones[0].translation_scale,
                            [-progress.bedrock_progress(0.5), camera_pitch, 0.0],
                        );
                    }
                    assert_eq!(authored_state(&store), authored);
                    let unchanged = store.get(1).unwrap();
                    assert_eq!(unchanged.previous, completed.0);
                    assert_eq!(unchanged.current, completed.1);
                }
            }
        }
    }
}
