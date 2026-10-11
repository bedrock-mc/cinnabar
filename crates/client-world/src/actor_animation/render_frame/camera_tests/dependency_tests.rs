use super::*;

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
