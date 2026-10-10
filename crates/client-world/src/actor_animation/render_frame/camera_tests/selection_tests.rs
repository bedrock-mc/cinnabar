use super::*;

/// Provides walking clips without any reachable camera-sensitive animation.
fn walking_compiled() -> assets::CompiledEntityAssets {
    let mut compiled = inactive_camera_compiled();
    compiled.rig_animations = compiled.rig_animations[..1].into();
    compiled.rig_geometries[0].animation_count = 1;
    compiled
}

#[test]
fn frame_only_geometry_selection_retains_the_active_walking_clip() {
    let mut compiled = walking_compiled();
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
    let expression = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op,
        op_count: 1,
        max_stack: 1,
    });
    compiled.molang_expressions = expressions.into_boxed_slice();
    for choice in &mut compiled.render.geometries {
        choice.condition = Some(expression);
    }
    let store = walking_fixture(compiled);
    let rig = store.actor_rig(1).unwrap();
    let previous = rig.previous[0];
    let current = rig.current[0];
    let completed_tick = rig.completed_tick;
    assert_ne!(previous, current);
    assert!(rig.render.iter().all(|layer| layer.geometry.is_none()));
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
        assert!(layers[0].pose.is_empty());
        for layer in &layers[1..] {
            assert_eq!(layer.geometry, Some(1));
            assert_eq!(layer.pose[1], current);
            assert_eq!(layer.previous_pose, layer.pose);
        }
        assert!(Arc::ptr_eq(&layers[1].pose, &layers[2].pose));
    }
    let rig = store.actor_rig(1).unwrap();
    assert_eq!(rig.completed_tick, completed_tick);
    assert_eq!(rig.previous[0], previous);
    assert_eq!(rig.current[0], current);
}

/// Makes only the alternate geometry's walking clip read the physical swing fraction.
fn swing_layer_compiled() -> assets::CompiledEntityAssets {
    let mut compiled = walking_compiled();
    compiled.molang_symbols[2].kind = MolangSymbolKind::Variable;
    compiled.molang_symbols[2].identifier = "variable.attack_time".into();
    compiled.molang_ops = vec![
        MolangOp::LoadVariable(2),
        MolangOp::LoadVariable(2),
        MolangOp::LoadQuery(1),
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
    ]
    .into_boxed_slice();
    for (index, expression) in compiled.molang_expressions.iter_mut().enumerate() {
        expression.first_op = index as u32;
        expression.op_count = 1;
    }
    let channel = &compiled.animation_channels[1];
    compiled.animation_keyframes[channel.first_keyframe as usize].expressions =
        [Some(0), None, None];
    compiled
}

#[test]
fn unused_swing_geometry_preserves_body_walking_endpoints() {
    for disabled_layer in [false, true] {
        let mut compiled = swing_layer_compiled();
        if disabled_layer {
            for layer in &mut compiled.render.layers[1..] {
                layer.condition = Some(3);
            }
        } else {
            for choice in &mut compiled.render.geometries {
                choice.condition = Some(3);
            }
        }
        let mut store = walking_fixture(compiled);
        store.exclude_remote_state_for(1);
        let rig = store.actor_rig(1).unwrap();
        let previous = rig.previous[0];
        let current = rig.current[0];
        let completed_tick = rig.completed_tick;
        assert_ne!(previous, current);
        store.sync_local_swing(
            1,
            crate::LocalSwingProgress {
                bedrock: [0.25, 0.5],
                java: [0.25, 0.5],
                frame_alpha: Some(0.75),
            },
        );
        store.advance_interpolation_frame(0);
        for alpha in [0.25, 0.75] {
            let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
            for layer in &layers {
                assert_eq!(layer.geometry, None);
                assert_eq!(*layer.previous_pose.first().unwrap_or(&previous), previous);
                assert_eq!(*layer.pose.first().unwrap_or(&current), current);
            }
        }
        assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
    }
}

#[test]
fn selected_swing_geometry_samples_without_replacing_body_walking_endpoints() {
    let mut store = walking_fixture(swing_layer_compiled());
    store.exclude_remote_state_for(1);
    let rig = store.actor_rig(1).unwrap();
    let previous = rig.previous[0];
    let current = rig.current[0];
    let completed_tick = rig.completed_tick;
    assert_ne!(previous, current);
    store.sync_local_swing(
        1,
        crate::LocalSwingProgress {
            bedrock: [0.25, 0.5],
            java: [0.25, 0.5],
            frame_alpha: Some(0.75),
        },
    );
    store.advance_interpolation_frame(0);
    for alpha in [0.0, 0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
        assert!(layers[0].pose.is_empty());
        for layer in &layers[1..] {
            assert_eq!(layer.geometry, Some(1));
            assert_rotation(
                layer.pose[1].rotation,
                pose::quat_from_euler([-0.4375, 0.0, 0.0]),
            );
            assert_eq!(layer.previous_pose, layer.pose);
        }
    }
    let rig = store.actor_rig(1).unwrap();
    assert_eq!(rig.completed_tick, completed_tick);
    assert_eq!(rig.previous[0], previous);
    assert_eq!(rig.current[0], current);
}
