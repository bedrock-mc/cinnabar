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
    let channel = compiled.animation_channels[0].clone();
    compiled.animation_channels = (0..2)
        .map(|bone| EntityAnimationChannel {
            bone_name: None,
            bone,
            property: EntityAnimationProperty::Rotation,
            first_keyframe: bone * channel.keyframe_count,
            ..channel.clone()
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compiled.animation_keyframes[0].expressions = [Some(0), Some(1), None];
    // Each channel owns a contiguous keyframe range of its own.
    compiled.animation_keyframes = compiled.animation_keyframes[..channel.keyframe_count as usize]
        .repeat(2)
        .into_boxed_slice();
    // Layers, slots and candidates each own contiguous ranges, as the carrier requires.
    let body = compiled.render.layers[0];
    compiled.render.layers = (0..3)
        .map(|layer| assets::EntityRenderLayer {
            first_slot: layer,
            first_geometry: layer.saturating_sub(1),
            geometry_count: u16::from(layer > 0),
            ..body
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let slot = compiled.render.slots[0];
    compiled.render.slots = (0..3)
        .map(|first_candidate| assets::EntityRenderSlot {
            first_candidate,
            ..slot
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compiled.render.candidates = [compiled.render.candidates[0]; 3].into();
    compiled.render.geometries = [EntityRenderGeometry {
        condition: None,
        geometry: 1,
    }; 2]
        .into();
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
        if let MolangOp::CallQuery(call) = op {
            call.symbol = 2;
        }
    }
    ops.extend([
        MolangOp::Push(EntityGeometryScalar::new(0.5).unwrap()),
        MolangOp::LoadQuery(1),
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
    assert!(sampling::needs_frame_sampling(&assets, 0));
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
        for (layer, completed) in layers.iter().zip(completed) {
            assert!(Arc::ptr_eq(&layer.pose, &completed.pose));
            assert!(Arc::ptr_eq(&layer.previous_pose, &completed.previous_pose));
        }
    }
}

/// Provides moving body/layer clips alongside a dormant camera-sensitive weapon clip.
fn inactive_camera_compiled() -> assets::CompiledEntityAssets {
    let mut compiled = camera_compiled();
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.insert(
        1,
        MolangSymbol {
            kind: MolangSymbolKind::Query,
            identifier: "query.modified_distance_moved".into(),
        },
    );
    compiled.molang_symbols = symbols.into_boxed_slice();
    let mut ops = compiled.molang_ops.into_vec();
    for op in &mut ops {
        if let MolangOp::CallQuery(call) = op {
            call.symbol = 2;
        }
    }
    ops.extend([
        MolangOp::LoadQuery(1),
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
    ]);
    compiled.molang_ops = ops.into_boxed_slice();
    let mut expressions = compiled.molang_expressions.into_vec();
    expressions.extend([4, 5].map(|first_op| CompiledMolangExpression {
        first_op,
        op_count: 1,
        max_stack: 1,
    }));
    compiled.molang_expressions = expressions.into_boxed_slice();
    let camera_keyframe = compiled.animation_keyframes[0].clone();
    for keyframe in &mut compiled.animation_keyframes {
        keyframe.expressions = [Some(2), None, None];
    }
    let mut keyframes = compiled.animation_keyframes.into_vec();
    let first_keyframe = keyframes.len() as u32;
    keyframes.push(camera_keyframe);
    compiled.animation_keyframes = keyframes.into_boxed_slice();
    let mut channels = compiled.animation_channels.into_vec();
    let first_channel = channels.len() as u32;
    channels.push(EntityAnimationChannel {
        first_keyframe,
        keyframe_count: 1,
        ..channels[0].clone()
    });
    compiled.animation_channels = channels.into_boxed_slice();
    let mut symbols = compiled.symbols.into_vec();
    let symbol = 4;
    symbols.insert(
        4,
        EntityAssetSymbol {
            kind: EntityAssetKind::Animation,
            identifier: "animation.zz_camera".into(),
            source_index: 0,
            dependencies: Box::new([]),
        },
    );
    compiled.symbols = symbols.into_boxed_slice();
    compiled.rig_bindings[0].render_controller += 1;
    let mut clips = compiled.animation_clips.into_vec();
    let clip = clips.len() as u32;
    clips.push(EntityAnimationClip {
        symbol,
        first_channel,
        ..clips[0]
    });
    compiled.animation_clips = clips.into_boxed_slice();
    let mut bindings = compiled.rig_animations.into_vec();
    bindings.push(assets::EntityRigAnimationBinding {
        name: 0,
        clip,
        weight: Some(3),
        order: 1,
    });
    compiled.rig_animations = bindings.into_boxed_slice();
    compiled.rig_geometries[0].animation_count = 2;
    compiled
}

/// Advances a moving actor one tick so ordinary pose endpoints differ.
fn walking_fixture(compiled: assets::CompiledEntityAssets) -> crate::actor_store::ActorStore {
    let mut store = fixture_with_assets(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    store.apply(
        1,
        2,
        protocol::ActorEvent::Move(protocol::ActorMoveEvent {
            dimension: 0,
            runtime_id: 1,
            position: [Some(0.6), None, None],
            position_origin: protocol::ActorPositionOrigin::Feet,
            pitch: None,
            yaw: None,
            head_yaw: None,
            on_ground: Some(true),
            teleported: false,
            player_mode: None,
            source_tick: None,
            interpolation: Default::default(),
        }),
    );
    store.advance_interpolation_ticks(1);
    store
}

#[test]
fn inactive_camera_animation_preserves_walking_pose_interpolation() {
    let mut store = walking_fixture(inactive_camera_compiled());
    let completed = store.actor_rig(1).unwrap().render.to_vec();
    assert_ne!(completed[1].previous_pose, completed[1].pose);
    store.set_camera_position([4.0, 3.0, 0.0]);
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
        for (layer, completed) in layers.iter().zip(&completed).skip(1) {
            assert_eq!(layer.previous_pose, completed.previous_pose);
            assert_eq!(layer.pose, completed.pose);
        }
    }
}

#[test]
fn interpolated_walking_retains_channel_writes_for_render_colors() {
    let mut compiled = inactive_camera_compiled();
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.push(MolangSymbol {
        kind: MolangSymbolKind::Variable,
        identifier: "variable.movement_tint".into(),
    });
    compiled.molang_symbols = symbols.into_boxed_slice();
    let mut ops = compiled.molang_ops[..4].to_vec();
    ops.extend([
        MolangOp::LoadQuery(1),
        MolangOp::StoreVariable(3),
        MolangOp::LoadQuery(1),
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
        MolangOp::LoadVariable(3),
    ]);
    compiled.molang_ops = ops.into_boxed_slice();
    compiled.molang_expressions[2].op_count = 3;
    compiled.molang_expressions[3].first_op = 7;
    let mut expressions = compiled.molang_expressions.into_vec();
    expressions.push(CompiledMolangExpression {
        first_op: 8,
        op_count: 1,
        max_stack: 1,
    });
    compiled.molang_expressions = expressions.into_boxed_slice();
    for layer in &mut compiled.render.layers {
        layer.color = Some([4; 4]);
    }
    let store = walking_fixture(compiled);
    let completed = store.actor_rig(1).unwrap().render.to_vec();
    assert!(completed[0].color[0] > 0.0);
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
        for (layer, completed) in layers.iter().zip(&completed) {
            assert_eq!(layer.color, completed.color);
            assert_eq!(layer.previous_pose, completed.previous_pose);
            assert_eq!(layer.pose, completed.pose);
        }
    }
}

#[test]
fn camera_animation_mapped_only_to_selected_geometry_samples_between_ticks() {
    let mut compiled = inactive_camera_compiled();
    let channel = &compiled.animation_channels[1];
    compiled.animation_keyframes[channel.first_keyframe as usize].expressions =
        [Some(0), Some(1), None];
    let mut store = fixture_with_assets(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    let completed_tick = store.actor_rig(1).unwrap().completed_tick;
    store.set_camera_position([4.0, 3.0, 0.0]);
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
        let expected = pose::quat_from_euler([3.0_f32.atan2(4.0).to_degrees(), 90.0, 0.0]);
        for layer in &layers[1..] {
            assert_rotation(layer.pose[1].rotation, expected);
            assert_rotation(layer.pose[0].rotation, expected);
        }
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
}

#[test]
fn server_camera_animation_outside_rig_bindings_samples_between_ticks() {
    let mut compiled = inactive_camera_compiled();
    let mut symbols = compiled.symbols.into_vec();
    symbols.insert(
        5,
        EntityAssetSymbol {
            kind: EntityAssetKind::Animation,
            identifier: "animation.zz_server_camera".into(),
            source_index: 0,
            dependencies: Box::new([]),
        },
    );
    compiled.symbols = symbols.into_boxed_slice();
    compiled.rig_bindings[0].render_controller += 1;
    let mut keyframes = compiled.animation_keyframes.into_vec();
    let first_keyframe = keyframes.len() as u32;
    keyframes.push(keyframes[compiled.animation_channels[2].first_keyframe as usize]);
    compiled.animation_keyframes = keyframes.into_boxed_slice();
    let mut channels = compiled.animation_channels.into_vec();
    let first_channel = channels.len() as u32;
    channels.push(EntityAnimationChannel {
        first_keyframe,
        ..channels[2].clone()
    });
    compiled.animation_channels = channels.into_boxed_slice();
    let mut clips = compiled.animation_clips.into_vec();
    clips.push(EntityAnimationClip {
        symbol: 5,
        first_channel,
        override_previous: true,
        ..clips[2]
    });
    compiled.animation_clips = clips.into_boxed_slice();
    let mut store = fixture_with_assets(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    store.apply_item_actor(
        1,
        2,
        protocol::ItemActorEvent::Action(protocol::ActorActionEvent {
            actor_runtime_ids: Arc::from([1]),
            kind: protocol::ActorActionKind::Custom {
                animation: "animation.zz_server_camera".into(),
                controller: "fixture.server".into(),
                next_state: "".into(),
                stop_expression: "".into(),
                stop_expression_version: 0,
            },
            data: 0.0,
            swing_source: None,
        }),
    );
    store.advance_interpolation_ticks(1);
    let completed_tick = store.actor_rig(1).unwrap().completed_tick;
    let completed_pose = store.actor_rig(1).unwrap().current[0];
    store.set_camera_position([4.0, 3.0, 0.0]);
    for alpha in [0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
        let actual = layers[0].pose.first().unwrap_or(&completed_pose);
        assert_rotation(
            actual.rotation,
            pose::quat_from_euler([3.0_f32.atan2(4.0).to_degrees(), 90.0, 0.0]),
        );
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
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
    for keyframe in &mut compiled.animation_keyframes {
        keyframe.expressions = [Some(1), None, None];
    }
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut store = fixture_with_assets(assets);
    let completed_tick = store.actor_rig(1).unwrap().completed_tick;
    store.set_camera_position([4.0, 3.0, 0.0]);
    for alpha in [0.0, 0.25, 0.75] {
        let layers = store.render_frame(alpha).layers(1).unwrap().into_owned();
        // Positive authored X rotation turns toward negative X in the mirrored rig frame.
        let expected = pose::quat_from_euler([-0.75, 0.0, 0.0]);
        assert_rotation(layers[0].pose[0].rotation, expected);
        assert_rotation(layers[1].pose[1].rotation, expected);
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
}

#[track_caller]
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

#[test]
fn local_swing_samples_molang_layers_and_poses_at_the_physics_fraction() {
    let mut compiled = camera_compiled();
    compiled.molang_symbols[1].kind = MolangSymbolKind::Variable;
    compiled.molang_symbols[1].identifier = "variable.attack_time".into();
    compiled.molang_ops = vec![MolangOp::LoadVariable(1)].into_boxed_slice();
    compiled.molang_expressions = vec![CompiledMolangExpression {
        first_op: 0,
        op_count: 1,
        max_stack: 1,
    }]
    .into_boxed_slice();
    for keyframe in &mut compiled.animation_keyframes {
        keyframe.expressions = [Some(0), None, None];
    }
    for layer in &mut compiled.render.layers {
        layer.color = Some([0; 4]);
    }
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut store = fixture_with_assets(assets);
    store.exclude_remote_state_for(1);
    let completed_tick = store.actor_rig(1).unwrap().completed_tick;
    let progress = crate::LocalSwingProgress {
        bedrock: [0.25, 0.5],
        java: [0.25, 0.5],
        frame_alpha: Some(0.75),
    };
    store.sync_local_swing(1, progress);
    store.advance_interpolation_frame(0);
    assert_eq!(store.actor_rig(1).unwrap().render[0].color, [0.5; 4]);
    for actor_alpha in [0.0, 0.25, 1.0] {
        let layers = store
            .render_frame(actor_alpha)
            .layers(1)
            .unwrap()
            .into_owned();
        assert_eq!(layers[0].color, [0.4375; 4]);
        let expected = pose::quat_from_euler([-0.4375, 0.0, 0.0]);
        assert_rotation(layers[0].pose[0].rotation, expected);
        assert_rotation(layers[1].pose[1].rotation, expected);
        assert_eq!(layers[0].pose, layers[0].previous_pose);
    }
    assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
    let stats = store.animation_stats();
    store.sync_local_swing(
        1,
        crate::LocalSwingProgress {
            frame_alpha: Some(0.25),
            ..progress
        },
    );
    store.advance_interpolation_frame(0);
    assert_eq!(
        store.render_frame(0.9).layers(1).unwrap()[0].color,
        [0.3125; 4]
    );
    assert_eq!(store.actor_rig(1).unwrap().java.swing_progress(0.9), 0.3125);
    assert_eq!(store.animation_stats(), stats);
    let idle = crate::LocalSwingProgress {
        frame_alpha: Some(0.1),
        ..Default::default()
    };
    store.sync_local_swing(1, idle);
    store.advance_interpolation_frame(0);
    let stats = store.animation_stats();
    let pose = Arc::clone(&store.actor_rig(1).unwrap().render[0].pose);
    store.sync_local_swing(
        1,
        crate::LocalSwingProgress {
            frame_alpha: Some(0.9),
            ..idle
        },
    );
    store.advance_interpolation_frame(0);
    let layers = store.render_frame(0.75).layers(1).unwrap();
    assert!(matches!(layers, Cow::Borrowed(_)));
    assert!(Arc::ptr_eq(&pose, &layers[0].pose));
    assert_eq!(store.animation_stats(), stats);
}

#[test]
fn local_swing_wrap_resamples_controller_weight_without_advancing_tick_state() {
    let mut compiled = camera_compiled();
    compiled.molang_symbols[1].kind = MolangSymbolKind::Variable;
    compiled.molang_symbols[1].identifier = "variable.attack_time".into();
    compiled.molang_ops = vec![
        MolangOp::LoadVariable(1),
        MolangOp::LoadVariable(1),
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
        MolangOp::Greater,
    ]
    .into_boxed_slice();
    compiled.molang_expressions = vec![
        CompiledMolangExpression {
            first_op: 0,
            op_count: 1,
            max_stack: 1,
        },
        CompiledMolangExpression {
            first_op: 1,
            op_count: 3,
            max_stack: 2,
        },
    ]
    .into_boxed_slice();
    for keyframe in &mut compiled.animation_keyframes {
        keyframe.expressions = [Some(0), None, None];
    }
    let mut symbols = compiled.symbols.into_vec();
    symbols.insert(
        4,
        EntityAssetSymbol {
            kind: EntityAssetKind::AnimationController,
            identifier: "controller.animation.test".into(),
            source_index: 0,
            dependencies: Box::new([]),
        },
    );
    compiled.symbols = symbols.into_boxed_slice();
    let mut sources = compiled.sources.into_vec();
    sources.insert(
        0,
        assets::EntityAssetSource {
            path: "animation_controllers/test.json".into(),
            source_bytes: 1,
            source_sha256: [1; 32],
        },
    );
    compiled.sources = sources.into_boxed_slice();
    for symbol in &mut compiled.symbols {
        symbol.source_index += 1;
    }
    compiled.symbols[4].source_index = 0;
    for geometry in &mut compiled.geometries {
        geometry.source_index += 1;
    }
    for clip in &mut compiled.animation_clips {
        clip.source += 1;
    }

    compiled.rig_bindings[0].render_controller = 5;
    for candidate in &mut compiled.render.candidates {
        candidate.source = 5;
    }
    compiled.controllers = vec![assets::EntityAnimationController {
        symbol: 4,
        first_state: 0,
        state_count: 1,
        initial_state: 0,
    }]
    .into_boxed_slice();
    compiled.controller_states = vec![assets::EntityControllerState {
        name: 0,
        first_animation: 0,
        animation_count: 1,
        ..Default::default()
    }]
    .into_boxed_slice();
    compiled.controller_animations = vec![assets::EntityControllerAnimation {
        target: assets::EntityControllerAnimationTarget::Clip(0),
        weight: Some(1),
    }]
    .into_boxed_slice();
    compiled.rig_geometries[0].animation_count = 0;
    compiled.rig_animations = Box::new([]);
    compiled.rig_geometries[0].controller_count = 1;
    compiled.rig_controllers = vec![assets::EntityRigControllerBinding {
        name: 0,
        controller: 0,
        weight: None,
        order: 0,
    }]
    .into_boxed_slice();
    for static_channel in [false, true] {
        for gate in 0..3 {
            let mut variant = compiled.clone();
            if static_channel {
                for keyframe in &mut variant.animation_keyframes {
                    keyframe.expressions = [None; 3];
                    keyframe.value[0] = EntityGeometryScalar::new(10.0).unwrap();
                }
            }
            if gate == 0 {
                variant.rig_geometries[0].animation_count = 1;
                variant.rig_geometries[0].controller_count = 0;
                variant.rig_controllers = Box::new([]);
                variant.rig_animations = vec![assets::EntityRigAnimationBinding {
                    name: 0,
                    clip: 0,
                    weight: Some(1),
                    order: 0,
                }]
                .into_boxed_slice();
            } else if gate == 1 {
                variant.rig_controllers[0].weight = Some(1);
                variant.controller_animations[0].weight = None;
            }
            let mut store = fixture_with_assets(Arc::new(
                RuntimeEntityAssets::from_compiled(variant).unwrap(),
            ));
            store.exclude_remote_state_for(1);
            let completed_tick = store.actor_rig(1).unwrap().completed_tick;
            store.sync_local_swing(
                1,
                crate::LocalSwingProgress {
                    bedrock: [5.0 / 6.0, 0.0],
                    java: [5.0 / 6.0, 0.0],
                    frame_alpha: Some(0.5),
                },
            );
            store.advance_interpolation_frame(0);
            let stats = store.animation_stats();
            let completed_pose = store.actor_rig(1).unwrap().current.to_vec();
            for _ in 0..2 {
                let layers = store.render_frame(0.0).layers(1).unwrap().into_owned();
                let angle = if static_channel { -10.0 } else { -11.0 / 12.0 };
                let expected = pose::quat_from_euler([angle, 0.0, 0.0]);
                assert_rotation(layers[0].pose[0].rotation, expected);
            }
            assert_eq!(store.actor_rig(1).unwrap().completed_tick, completed_tick);
            assert_eq!(store.actor_rig(1).unwrap().current, completed_pose);
            assert_eq!(store.animation_stats(), stats);
        }
    }
}

#[test]
fn local_swing_frame_preserves_the_authored_item_rotation_factor() {
    let mut compiled = camera_compiled();
    compiled.molang_symbols[1].kind = MolangSymbolKind::Variable;
    compiled.molang_symbols[1].identifier = "variable.attack_time".into();
    let mut symbols = compiled.molang_symbols.into_vec();
    for identifier in [
        "variable.first_person_item_rotation_factor",
        "variable.first_person_rotation_factor",
    ] {
        symbols.push(MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: identifier.into(),
        });
    }
    compiled.molang_symbols = symbols.into_boxed_slice();
    compiled.rig_bindings[0].pre_animation = Some(1);
    compiled.molang_ops = vec![
        MolangOp::LoadVariable(2),
        MolangOp::LoadVariable(1),
        MolangOp::StoreVariable(3),
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
    ]
    .into_boxed_slice();
    compiled.molang_expressions = vec![
        CompiledMolangExpression {
            first_op: 0,
            op_count: 1,
            max_stack: 1,
        },
        CompiledMolangExpression {
            first_op: 1,
            op_count: 3,
            max_stack: 1,
        },
    ]
    .into_boxed_slice();
    for keyframe in &mut compiled.animation_keyframes {
        keyframe.expressions = [Some(0), None, None];
    }
    let mut store = fixture_with_assets(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    store.exclude_remote_state_for(1);
    store.sync_local_swing(
        1,
        crate::LocalSwingProgress {
            bedrock: [0.25, 0.5],
            java: [0.25, 0.5],
            frame_alpha: Some(0.75),
        },
    );
    store.advance_interpolation_frame(0);
    let layers = store.render_frame(0.0).layers(1).unwrap().into_owned();
    assert_rotation(
        layers[0].pose[0].rotation,
        pose::quat_from_euler([-0.4375, 0.0, 0.0]),
    );
}
