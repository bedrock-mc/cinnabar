use super::*;

#[test]
fn camera_derived_selection_writes_follow_live_presentation_inputs() {
    for clock_write in [false, true] {
        let mut compiled = camera_compiled();
        compiled.molang_symbols[1].identifier = "query.camera_rotation".into();
        let mut symbols = compiled.molang_symbols.into_vec();
        symbols.push(MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "variable.angle".into(),
        });
        compiled.molang_symbols = symbols.into_boxed_slice();
        let scalar = |value| EntityGeometryScalar::new(value).unwrap();
        compiled.molang_ops = vec![
            MolangOp::Push(scalar(0.0)),
            MolangOp::CallQuery(MolangCall {
                symbol: 1,
                arguments: 1,
            }),
            MolangOp::StoreVariable(2),
            MolangOp::Push(scalar(0.0)),
            MolangOp::LoadVariable(2),
            MolangOp::Push(scalar(10.0)),
            MolangOp::Add,
            MolangOp::StoreVariable(2),
            MolangOp::Push(scalar(1.0)),
            MolangOp::LoadVariable(2),
        ]
        .into_boxed_slice();
        compiled.molang_expressions = [(0, 4, 1), (4, 5, 2), (9, 1, 1)]
            .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
                first_op,
                op_count,
                max_stack,
            })
            .into();
        compiled.rig_bindings[0].pre_animation = Some(0);
        if clock_write {
            compiled.animation_clips[0].anim_time_update = Some(1);
        } else {
            compiled.rig_animations[0].weight = Some(1);
        }
        for key in &mut compiled.animation_keyframes {
            key.expressions = [Some(2), None, None];
        }
        let mut store = fixture_with_assets(Arc::new(
            RuntimeEntityAssets::from_compiled(compiled).unwrap(),
        ));
        let completed_tick = store.actor_rig(1).unwrap().completed_tick;
        store.set_camera_rotation([30.0, 0.0]);
        for alpha in [0.25, 0.75] {
            let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
            let expected = pose::quat_from_euler([-40.0, 0.0, 0.0]);
            assert_rotation(layers[0].pose[0].rotation, expected);
            assert_rotation(layers[1].pose[1].rotation, expected);
        }
        assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
    }
}

#[test]
fn layer_only_swing_sampling_preserves_body_walking_interpolation() {
    for selected in [false, true] {
        let mut compiled = inactive_camera_compiled();
        let mut symbols = compiled.molang_symbols.into_vec();
        symbols.push(MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "variable.attack_time".into(),
        });
        compiled.molang_symbols = symbols.into_boxed_slice();
        let mut ops = compiled.molang_ops.into_vec();
        ops.push(MolangOp::LoadVariable(3));
        compiled.molang_ops = ops.into_boxed_slice();
        let mut expressions = compiled.molang_expressions.into_vec();
        expressions.push(CompiledMolangExpression {
            first_op: 6,
            op_count: 1,
            max_stack: 1,
        });
        compiled.molang_expressions = expressions.into_boxed_slice();
        let channel = &compiled.animation_channels[1];
        compiled.animation_keyframes[channel.first_keyframe as usize].expressions =
            [Some(4), None, None];
        if !selected {
            for layer in &mut compiled.render.layers[1..] {
                layer.condition = Some(3);
            }
        }
        let mut store = walking_fixture(compiled);
        store.exclude_remote_state_for(1);
        let progress = crate::LocalSwingProgress {
            bedrock: [0.25, 0.5],
            java: [0.25, 0.5],
            frame_alpha: Some(0.75),
        };
        store.sync_local_swing(1, progress);
        store.advance_interpolation_frame(0);
        let rig = store.actor_rig(1).unwrap();
        let previous = rig.previous[0];
        let current = rig.current[0];
        assert_ne!(previous, current);
        let layers = store.render_frame(0.25).layers(1).unwrap().into_owned();
        assert_eq!(
            *layers[0].previous_pose.first().unwrap_or(&previous),
            previous
        );
        assert_eq!(*layers[0].pose.first().unwrap_or(&current), current);
        if selected {
            assert_rotation(
                layers[1].pose[1].rotation,
                pose::quat_from_euler([-progress.bedrock_progress(0.25), 0.0, 0.0]),
            );
        } else {
            assert_eq!(layers.len(), 1);
        }
    }
}

#[test]
fn expression_local_temporaries_preserve_walking_layer_interpolation() {
    let mut compiled = inactive_camera_compiled();
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.insert(
        1,
        MolangSymbol {
            kind: MolangSymbolKind::Query,
            identifier: "query.frame_alpha".into(),
        },
    );
    symbols.push(MolangSymbol {
        kind: MolangSymbolKind::Temporary,
        identifier: "temp.x".into(),
    });
    compiled.molang_symbols = symbols.into_boxed_slice();
    let mut ops = compiled.molang_ops.into_vec();
    for op in &mut ops {
        match op {
            MolangOp::LoadQuery(symbol) => *symbol += 1,
            MolangOp::CallQuery(call) => call.symbol += 1,
            _ => {}
        }
    }
    ops.extend([
        MolangOp::LoadQuery(2),
        MolangOp::StoreVariable(4),
        MolangOp::LoadVariable(4),
        MolangOp::LoadQuery(1),
        MolangOp::StoreVariable(4),
        MolangOp::LoadVariable(4),
    ]);
    compiled.molang_ops = ops.into_boxed_slice();
    let mut expressions = compiled.molang_expressions.into_vec();
    expressions.extend([6, 9].map(|first_op| CompiledMolangExpression {
        first_op,
        op_count: 3,
        max_stack: 1,
    }));
    compiled.molang_expressions = expressions.into_boxed_slice();
    let channel = &compiled.animation_channels[1];
    compiled.animation_keyframes[channel.first_keyframe as usize].expressions =
        [Some(4), None, None];
    for layer in &mut compiled.render.layers[1..] {
        layer.color = Some([5; 4]);
    }
    let store = walking_fixture(compiled);
    let completed = store.actor_rig(1).unwrap().render.to_vec();
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
        for (layer, completed) in layers.iter().zip(&completed).skip(1) {
            assert_eq!(layer.color, [alpha; 4]);
            assert_ne!(completed.previous_pose, completed.pose);
            assert_eq!(layer.previous_pose, completed.previous_pose);
            assert_eq!(layer.pose, completed.pose);
        }
    }
}

#[test]
fn render_controller_camera_write_updates_only_a_layer_that_reads_it() {
    for reads_camera_write in [false, true] {
        let mut compiled = inactive_camera_compiled();
        let mut symbols = compiled.molang_symbols.into_vec();
        symbols.push(MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "variable.camera_layer_angle".into(),
        });
        compiled.molang_symbols = symbols.into_boxed_slice();
        let mut ops = compiled.molang_ops.into_vec();
        ops.extend([
            MolangOp::LoadVariable(3),
            MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
            MolangOp::CallQuery(MolangCall {
                symbol: 2,
                arguments: 1,
            }),
            MolangOp::StoreVariable(3),
            MolangOp::Push(EntityGeometryScalar::new(1.0).unwrap()),
        ]);
        compiled.molang_ops = ops.into_boxed_slice();
        let mut expressions = compiled.molang_expressions.into_vec();
        expressions.extend([
            CompiledMolangExpression {
                first_op: 6,
                op_count: 1,
                max_stack: 1,
            },
            CompiledMolangExpression {
                first_op: 7,
                op_count: 4,
                max_stack: 1,
            },
        ]);
        compiled.molang_expressions = expressions.into_boxed_slice();
        compiled.render.layers[1].color = Some([5; 4]);
        if reads_camera_write {
            let channel = &compiled.animation_channels[1];
            compiled.animation_keyframes[channel.first_keyframe as usize].expressions =
                [Some(4), None, None];
        }
        let mut store = if reads_camera_write {
            fixture_with_assets(Arc::new(
                RuntimeEntityAssets::from_compiled(compiled).unwrap(),
            ))
        } else {
            walking_fixture(compiled)
        };
        let completed = store.actor_rig(1).unwrap().render.to_vec();
        store.set_camera_position([4.0, 3.0, 0.0]);
        for alpha in [0.25, 0.75] {
            let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
            for (layer, completed) in layers.iter().zip(&completed).skip(1) {
                if reads_camera_write {
                    let expected =
                        pose::quat_from_euler([3.0_f32.atan2(4.0).to_degrees(), 0.0, 0.0]);
                    assert_rotation(layer.pose[1].rotation, expected);
                    assert_rotation(layer.pose[0].rotation, expected);
                } else {
                    assert_ne!(completed.previous_pose, completed.pose);
                    assert_eq!(layer.previous_pose, completed.previous_pose);
                    assert_eq!(layer.pose, completed.pose);
                }
            }
        }
    }
}

#[test]
fn selected_camera_layer_retains_weight_and_clock_variable_writes() {
    for clock_write in [false, true] {
        let mut compiled = inactive_camera_compiled();
        let mut symbols = compiled.molang_symbols.into_vec();
        symbols.push(MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "variable.layer_offset".into(),
        });
        compiled.molang_symbols = symbols.into_boxed_slice();
        let mut ops = compiled.molang_ops.into_vec();
        ops.extend([
            MolangOp::LoadQuery(1),
            MolangOp::StoreVariable(3),
            MolangOp::Push(EntityGeometryScalar::new(1.0).unwrap()),
            MolangOp::LoadVariable(3),
        ]);
        compiled.molang_ops = ops.into_boxed_slice();
        let mut expressions = compiled.molang_expressions.into_vec();
        expressions.extend([
            CompiledMolangExpression {
                first_op: 6,
                op_count: 3,
                max_stack: 1,
            },
            CompiledMolangExpression {
                first_op: 9,
                op_count: 1,
                max_stack: 1,
            },
        ]);
        compiled.molang_expressions = expressions.into_boxed_slice();
        if clock_write {
            compiled.animation_clips[0].anim_time_update = Some(4);
        } else {
            compiled.rig_animations[0].weight = Some(4);
        }
        let channel = &compiled.animation_channels[1];
        compiled.animation_keyframes[channel.first_keyframe as usize].expressions =
            [Some(0), Some(1), Some(5)];
        let store = walking_fixture(compiled);
        let rig = store.actor_rig(1).unwrap();
        let completed_tick = rig.completed_tick;
        let expected = rig.render[1].pose[1].rotation;
        assert!(expected[2].abs() > 1e-5);
        for alpha in [0.25, 0.75] {
            let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
            for layer in &layers[1..] {
                assert_rotation(layer.pose[1].rotation, expected);
                assert_rotation(layer.pose[0].rotation, expected);
            }
        }
        assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
    }
}

#[test]
fn long_numeric_clip_keeps_frame_color_updates_and_completed_poses() {
    let mut compiled = inactive_camera_compiled();
    compiled.rig_geometries[0].animation_count = 1;
    compiled.rig_animations = compiled.rig_animations[..1].to_vec().into_boxed_slice();
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.insert(
        1,
        MolangSymbol {
            kind: MolangSymbolKind::Query,
            identifier: "query.frame_alpha".into(),
        },
    );
    compiled.molang_symbols = symbols.into_boxed_slice();
    let mut ops = compiled.molang_ops.into_vec();
    for op in &mut ops {
        match op {
            MolangOp::LoadQuery(symbol) => *symbol += 1,
            MolangOp::CallQuery(call) => call.symbol += 1,
            _ => {}
        }
    }
    let first_op = ops.len() as u32;
    ops.push(MolangOp::LoadQuery(1));
    compiled.molang_ops = ops.into_boxed_slice();
    let mut expressions = compiled.molang_expressions.into_vec();
    expressions.push(CompiledMolangExpression {
        first_op,
        op_count: 1,
        max_stack: 1,
    });
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.render.layers[1].color = Some([4; 4]);
    let mut keys = vec![compiled.animation_keyframes[0]];
    let mut numeric = compiled.animation_keyframes[1];
    numeric.expressions = [None; 3];
    numeric.value[0] = EntityGeometryScalar::new(30.0).unwrap();
    let keyframes = 4096;
    for index in 0..keyframes {
        numeric.time_seconds = EntityGeometryScalar::new(index as f32 / keyframes as f32).unwrap();
        keys.push(numeric);
    }
    keys.push(compiled.animation_keyframes[2]);
    compiled.animation_keyframes = keys.into_boxed_slice();
    compiled.animation_channels[1].keyframe_count = keyframes;
    compiled.animation_channels[2].first_keyframe = keyframes + 1;
    compiled.animation_clips[1].length_seconds = EntityGeometryScalar::new(1.0).unwrap();
    let store = walking_fixture(compiled);
    let completed = store.actor_rig(1).unwrap().render.to_vec();
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
        assert_eq!(layers[1].color, [alpha; 4]);
        for (layer, completed) in layers.iter().zip(&completed) {
            assert_eq!(layer.previous_pose, completed.previous_pose);
            assert_eq!(layer.pose, completed.pose);
        }
    }
}
