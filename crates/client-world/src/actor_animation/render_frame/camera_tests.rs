use super::*;
use assets::{
    CompiledMolangExpression, EntityAnimationChannel, EntityAnimationClip, EntityAnimationProperty,
    EntityAssetSymbol, EntityGeometryScalar, EntityRenderGeometry, MolangCall, MolangSymbol,
    MolangSymbolKind,
};

fn camera_compiled() -> assets::CompiledEntityAssets {
    let mut compiled = super::super::attachable::tests::compiled_fixture();
    compiled.sources[1].path = "entity/item.json".into();
    compiled.symbols[4].kind = EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:test".into();
    compiled.symbols.rotate_right(1);
    compiled.geometries[0].bones[0].name = "root".into();
    let mut title = compiled.geometries[0].clone();
    title.identifier = "geometry.title".into();
    let root = title.bones[0].clone();
    let mut child = root.clone();
    child.name = "label".into();
    child.parent = Some("root".into());
    title.bones = vec![child, root].into_boxed_slice();
    let mut symbols = compiled.symbols.into_vec();
    symbols.insert(
        2,
        EntityAssetSymbol {
            kind: EntityAssetKind::Geometry,
            identifier: title.identifier.clone(),
            source_index: title.source_index,
            dependencies: Box::new([]),
        },
    );
    compiled.symbols = symbols.into_boxed_slice();
    let mut geometries = compiled.geometries.into_vec();
    geometries.push(title);
    compiled.geometries = geometries.into_boxed_slice();
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = 4;
    compiled.rig_bindings[0].pre_animation = None;
    compiled.molang_symbols = vec![
        MolangSymbol {
            kind: MolangSymbolKind::Name,
            identifier: "face".into(),
        },
        MolangSymbol {
            kind: MolangSymbolKind::Query,
            identifier: "query.rotation_to_camera".into(),
        },
    ]
    .into_boxed_slice();
    let scalar = |value| EntityGeometryScalar::new(value).unwrap();
    compiled.molang_ops = [0.0, 1.0]
        .into_iter()
        .flat_map(|axis| {
            [
                MolangOp::Push(scalar(axis)),
                MolangOp::CallQuery(MolangCall {
                    symbol: 1,
                    arguments: 1,
                }),
            ]
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compiled.molang_expressions = [0, 2]
        .into_iter()
        .map(|first_op| CompiledMolangExpression {
            first_op,
            op_count: 2,
            max_stack: 1,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let clip = compiled.animation_clips[0];
    compiled.animation_clips = (0..2)
        .map(|geometry| EntityAnimationClip {
            symbol: 3,
            first_channel: geometry,
            geometry: Some(geometry),
            ..clip
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let channel = compiled.animation_channels[0];
    compiled.animation_channels = (0..2)
        .map(|bone| EntityAnimationChannel {
            bone,
            property: EntityAnimationProperty::Rotation,
            ..channel
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compiled.animation_keyframes[0].expressions = [Some(0), Some(1), None];
    let body = compiled.render.layers[0].clone();
    let title = assets::EntityRenderLayer {
        geometry_count: 1,
        ..body.clone()
    };
    compiled.render.layers = vec![body, title.clone(), title].into_boxed_slice();
    compiled.render.geometries = vec![EntityRenderGeometry {
        condition: None,
        geometry: 1,
    }]
    .into_boxed_slice();
    compiled
}

fn camera_assets() -> Arc<RuntimeEntityAssets> {
    Arc::new(RuntimeEntityAssets::from_compiled(camera_compiled()).unwrap())
}

fn fixture() -> crate::actor_store::ActorStore {
    fixture_with_assets(camera_assets())
}

fn fixture_with_assets(assets: Arc<RuntimeEntityAssets>) -> crate::actor_store::ActorStore {
    let mut store = crate::actor_store::ActorStore::new_with_entity_assets(1, 0, assets);
    store.set_camera_position([0.0, 0.0, 4.0]);
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: ActorKind::Entity {
                identifier: "minecraft:test".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    store.advance_interpolation_ticks(1);
    store
}

#[test]
fn light_multiplier_defaults_to_one_and_evaluates_unclamped_values_between_ticks() {
    let mut compiled = camera_compiled();
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.push(MolangSymbol {
        kind: MolangSymbolKind::Query,
        identifier: "query.frame_alpha".into(),
    });
    compiled.molang_symbols = symbols.into_boxed_slice();
    let mut ops = compiled.molang_ops.into_vec();
    ops.extend([
        MolangOp::Push(EntityGeometryScalar::new(0.5).unwrap()),
        MolangOp::LoadQuery(2),
        MolangOp::Add,
        MolangOp::Push(EntityGeometryScalar::new(-0.25).unwrap()),
    ]);
    compiled.molang_ops = ops.into_boxed_slice();
    let mut expressions = compiled.molang_expressions.into_vec();
    expressions.extend([
        CompiledMolangExpression {
            first_op: 4,
            op_count: 3,
            max_stack: 2,
        },
        CompiledMolangExpression {
            first_op: 7,
            op_count: 1,
            max_stack: 1,
        },
    ]);
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.render.layers[1].light_color_multiplier = Some(2);
    compiled.render.layers[1].ignore_lighting = true;
    compiled.render.layers[2].light_color_multiplier = Some(3);
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    assert!(needs_frame_sampling(&assets, 0));
    let store = fixture_with_assets(assets);
    let completed_tick = store.actor_rig(1).unwrap().completed_tick;
    for alpha in [0.0, 0.75] {
        let mut frame = store.render_frame(alpha);
        let layers = frame.layers(1).unwrap();
        assert_eq!(layers[0].light_color_multiplier, 1.0);
        assert_eq!(layers[1].light_color_multiplier, 0.5 + alpha);
        assert!(layers[1].ignore_lighting);
        assert_eq!(layers[2].light_color_multiplier, -0.25);
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
}

#[test]
fn unchanged_camera_and_frame_input_retains_completed_geometry_poses_without_resampling() {
    let store = fixture();
    let completed = store.actor_rig(1).unwrap().render;
    for _ in 0..3 {
        let mut frame = store.render_frame(0.0);
        let layers = frame.layers(1).unwrap();
        for (layer, completed) in layers.iter().zip(&completed) {
            assert!(Arc::ptr_eq(&layer.pose, &completed.pose));
            assert!(Arc::ptr_eq(&layer.previous_pose, &completed.previous_pose));
        }
    }
}

#[test]
fn camera_distance_pre_animation_updates_channel_variables_between_ticks() {
    let mut compiled = camera_compiled();
    let scalar = |value| EntityGeometryScalar::new(value).unwrap();
    compiled.rig_bindings[0].pre_animation = Some(0);
    compiled.molang_symbols[1].identifier = "query.camera_distance_range_lerp".into();
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.push(MolangSymbol {
        kind: MolangSymbolKind::Variable,
        identifier: "variable.camera_blend".into(),
    });
    compiled.molang_symbols = symbols.into_boxed_slice();
    compiled.molang_ops = vec![
        MolangOp::Push(scalar(2.0)),
        MolangOp::Push(scalar(6.0)),
        MolangOp::CallQuery(MolangCall {
            symbol: 1,
            arguments: 2,
        }),
        MolangOp::StoreVariable(2),
        MolangOp::Push(scalar(0.0)),
        MolangOp::LoadVariable(2),
    ]
    .into_boxed_slice();
    compiled.molang_expressions = vec![
        CompiledMolangExpression {
            first_op: 0,
            op_count: 5,
            max_stack: 2,
        },
        CompiledMolangExpression {
            first_op: 5,
            op_count: 1,
            max_stack: 1,
        },
    ]
    .into_boxed_slice();
    compiled.animation_keyframes[0].expressions = [Some(1), None, None];
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut store = fixture_with_assets(assets);
    let completed_tick = store.actor_rig(1).unwrap().completed_tick;
    store.set_camera_position([4.0, 3.0, 0.0]);
    for alpha in [0.0, 0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
        let expected = pose::quat_from_euler([0.75, 0.0, 0.0]);
        assert_rotation(layers[0].pose[0].rotation, expected);
        assert_rotation(layers[1].pose[1].rotation, expected);
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
}

fn assert_rotation(actual: [f32; 4], expected: [f32; 4]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1e-5, "{actual} != {expected}");
    }
}

#[test]
fn camera_position_samples_body_and_selected_geometry_without_advancing_a_tick() {
    let mut store = fixture();
    let tick = store.actor_rig(1).unwrap();
    let completed_tick = tick.completed_tick;
    let tick_pose = tick.current.to_vec();
    let tick_layers = tick.render.to_vec();
    let actor = store.get(1).unwrap().clone();
    let stats = store.animation_stats();
    for (position, rotation) in [
        (
            [4.0, 3.0, 0.0],
            [3.0_f32.atan2(4.0).to_degrees(), 90.0, 0.0],
        ),
        (
            [-4.0, -3.0, 0.0],
            [-3.0_f32.atan2(4.0).to_degrees(), -90.0, 0.0],
        ),
    ] {
        store.set_camera_position(position);
        for alpha in [0.0, 0.25, 0.75] {
            let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
            assert_eq!(layers.len(), 3);
            let expected = pose::quat_from_euler(rotation);
            assert_rotation(layers[0].pose[0].rotation, expected);
            for layer in &layers[1..] {
                assert_eq!(layer.geometry, Some(1));
                assert_eq!(layer.pose.len(), 2);
                assert_rotation(layer.pose[1].rotation, expected);
                assert_rotation(layer.pose[0].rotation, expected);
                assert_eq!(layer.previous_pose, layer.pose);
            }
            assert!(Arc::ptr_eq(&layers[1].pose, &layers[2].pose));
            assert_eq!(layers[0].previous_pose, layers[0].pose);
            assert_ne!(layers[1].pose, tick_layers[1].pose);
        }
    }
    let tick = store.actor_rig(1).unwrap();
    assert_eq!(tick.completed_tick, completed_tick);
    assert_eq!(tick.current, tick_pose);
    assert_eq!(tick.render, tick_layers);
    assert_eq!(store.get(1).unwrap(), &actor);
    assert_eq!(store.animation_stats(), stats);
}

#[test]
fn camera_frame_budget_exhaustion_keeps_all_completed_geometry_poses() {
    let mut store = fixture();
    store.set_camera_position([4.0, 3.0, 0.0]);
    let completed = store.actor_rig(1).unwrap().render;
    let mut saw_exhausted_frame = false;
    let mut saw_sampled_frame = false;
    for remaining_ops in 1..32 {
        let mut frame = store.render_frame(0.5);
        frame.remaining_ops = remaining_ops;
        let sampled = frame.layers(1).unwrap();
        if matches!(sampled, Cow::Borrowed(_)) {
            assert_eq!(sampled.as_ref(), completed);
            saw_exhausted_frame = true;
        } else {
            assert_ne!(sampled[1].pose, completed[1].pose);
            assert!(Arc::ptr_eq(&sampled[1].pose, &sampled[2].pose));
            saw_sampled_frame = true;
        }
    }
    assert!(saw_exhausted_frame);
    assert!(saw_sampled_frame);
}
