use super::*;
use protocol_ids::ActorFlag;

mod cache;
mod selected_geometry;
use assets::{
    CompiledEntityAssets, EntityAssetKind, EntityAssetSymbol, MolangSymbol, MolangSymbolKind,
};

/// Adds a query or variable to this fixture's sorted symbol table and shifts existing reads.
fn symbol(compiled: &mut CompiledEntityAssets, kind: MolangSymbolKind, name: &str) -> u32 {
    let mut symbols = compiled.molang_symbols.to_vec();
    let index = symbols
        .partition_point(|symbol| (symbol.kind, symbol.identifier.as_ref()) < (kind, name))
        as u32;
    symbols.insert(
        index as usize,
        MolangSymbol {
            kind,
            identifier: name.into(),
        },
    );
    compiled.molang_symbols = symbols.into();
    for op in &mut compiled.molang_ops {
        let slot = match op {
            MolangOp::LoadQuery(slot)
            | MolangOp::LoadVariable(slot)
            | MolangOp::StoreVariable(slot) => slot,
            MolangOp::CallQuery(call) => &mut call.symbol,
            _ => continue,
        };
        if *slot >= index {
            *slot += 1;
        }
    }
    index
}

/// Appends an authored expression without changing earlier bytecode ranges.
fn expression(compiled: &mut CompiledEntityAssets, ops: &[MolangOp], max_stack: u8) -> u32 {
    let mut expressions = compiled.molang_expressions.to_vec();
    let index = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op: compiled.molang_ops.len() as u32,
        op_count: ops.len() as u16,
        max_stack,
    });
    compiled.molang_expressions = expressions.into();
    let mut code = compiled.molang_ops.to_vec();
    code.extend_from_slice(ops);
    compiled.molang_ops = code.into();
    index
}

/// Appends a scalar binary-operation tail, which needs two stack slots.
fn extend(compiled: &mut CompiledEntityAssets, index: u32, tail: &[MolangOp]) -> u32 {
    let original = compiled.molang_expressions[index as usize];
    let first = original.first_op as usize;
    let mut ops = compiled.molang_ops[first..first + usize::from(original.op_count)].to_vec();
    ops.extend_from_slice(tail);
    expression(compiled, &ops, original.max_stack.max(2))
}

/// Adds a reachable camera clip with an authored constant activation weight.
fn camera_clip(compiled: &mut CompiledEntityAssets, active: bool) {
    let query = symbol(compiled, MolangSymbolKind::Query, "query.camera_rotation");
    let camera = expression(
        compiled,
        &[
            MolangOp::Push(crate::test_support::scalar(0.0)),
            MolangOp::CallQuery(MolangCall {
                symbol: query,
                arguments: 1,
            }),
        ],
        1,
    );
    let weight = expression(
        compiled,
        &[MolangOp::Push(crate::test_support::scalar(f32::from(
            active,
        )))],
        1,
    );
    let mut symbols = compiled.symbols.to_vec();
    let index = symbols.partition_point(|symbol| symbol.kind <= EntityAssetKind::Animation) as u32;
    symbols.insert(
        index as usize,
        EntityAssetSymbol {
            kind: EntityAssetKind::Animation,
            identifier: "animation.zz_camera".into(),
            source_index: 0,
            dependencies: Box::new([]),
            mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
        },
    );
    compiled.symbols = symbols.into();
    for binding in &mut compiled.rig_bindings {
        if let Some(controller) = binding.render_controller.as_mut()
            && *controller >= index
        {
            *controller += 1;
        }
    }
    let mut keys = compiled.animation_keyframes.to_vec();
    let mut key = keys[0];
    key.expressions = [Some(camera), None, None];
    let first_keyframe = keys.len() as u32;
    keys.push(key);
    compiled.animation_keyframes = keys.into();
    let mut channels = compiled.animation_channels.to_vec();
    let mut channel = channels[0].clone();
    channel.first_keyframe = first_keyframe;
    let first_channel = channels.len() as u32;
    channels.push(channel);
    compiled.animation_channels = channels.into();
    let mut clips = compiled.animation_clips.to_vec();
    let clip = clips.len() as u32;
    clips.push(assets::EntityAnimationClip {
        symbol: index,
        first_channel,
        channel_count: 1,
        ..clips[0]
    });
    compiled.animation_clips = clips.into();
    let mut bindings = compiled.rig_animations.to_vec();
    bindings.push(assets::EntityRigAnimationBinding {
        name: 0,
        clip,
        weight: Some(weight),
        order: 1,
    });
    compiled.rig_animations = bindings.into();
    compiled.rig_geometries[0].animation_count += 1;
}

#[test]
fn a_dormant_camera_clip_does_not_collapse_turning_players_walk_endpoints() {
    let mut compiled = compiled_fixture(180.0);
    camera_clip(&mut compiled, false);
    let (mut store, mut actors) = fixture_with(compiled, 0.0, Default::default());
    actors.get_mut(&1).unwrap().yaw = 90.0;
    actors.get_mut(&1).unwrap().head_yaw = 90.0;
    store.advance_tick(&actors, None, None, true, true, |_| Default::default());
    let rig = store.get(1).unwrap();
    let previous = rig.previous[0].rotation;
    let current = rig.current[0].rotation;
    assert_ne!(previous, current);
    let authored = authored_state(&store);
    for alpha in [0.25, 0.75] {
        let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let sampled = store
            .render_layers(
                &actors[&1],
                alpha,
                [10.0, 20.0],
                [4.0, 3.0, 0.0],
                &mut budget,
                false,
            )
            .unwrap();
        assert_eq!(sampled.render[0].previous_pose[0].rotation, previous);
        assert_eq!(sampled.render[0].pose[0].rotation, current);
        assert_angles(
            sampled.render[0].pose[0].translation_scale,
            [56.25 * alpha, 0.0, 33.75 * alpha],
        );
        assert_eq!(authored_state(&store), authored);
    }
}

/// Supplies a persona atlas on a named layer with its own pivot beside the base skin.
fn persona_source() -> Arc<render_api::SkinGeometrySource> {
    Arc::new(render_api::SkinGeometrySource {
        resource_patch: r#"{"geometry":{"default":"geometry.rotation_body","animated_32x32":"geometry.rotation_layer"}}"#.into(),
        geometry_data: r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.rotation_body","texture_width":32,"texture_height":32},"bones":[{"name":"rightitem","pivot":[0,0,0],"cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}]}]},{"description":{"identifier":"geometry.rotation_layer","texture_width":32,"texture_height":32},"bones":[{"name":"rightitem","pivot":[0,2,0],"cubes":[{"origin":[0,2,0],"size":[1,1,1],"uv":[0,0]}]}]}]}"#.into(),
        animations: Arc::from([render_api::SkinAnimation {
            kind: render_api::SkinAnimationKind::Body32,
            width: 32,
            height: 64,
            rgba8: vec![255; 32 * 64 * 4].into(),
            frames: 2,
            blinking: false,
        }]),
    })
}

#[test]
fn persona_layers_keep_both_walk_endpoints_while_the_player_turns() {
    let context = ActorTickContext {
        is_local: true,
        is_local_player: true,
        skin_geometry: Some(persona_source()),
        ..Default::default()
    };
    let (mut store, mut actors) = fixture_with(compiled_fixture(180.0), 0.0, context.clone());
    actors.get_mut(&1).unwrap().yaw = 90.0;
    actors.get_mut(&1).unwrap().head_yaw = 90.0;
    store.advance_tick(&actors, None, Some(1), true, true, |_| context.clone());
    let rig = store.get(1).unwrap();
    assert_eq!(rig.skin_layers.len(), 1);
    let previous = Arc::clone(&rig.skin_layers[0].previous);
    let current = Arc::clone(&rig.skin_layers[0].current);
    assert_ne!(previous[0].rotation, current[0].rotation);
    let authored = authored_state(&store);
    for alpha in [0.0, 0.5, 1.0] {
        let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
        let sampled = store
            .render_layers(&actors[&1], alpha, [0.0; 2], [0.0; 3], &mut budget, true)
            .unwrap();
        assert_eq!(sampled.skin[0].previous[0].rotation, previous[0].rotation);
        assert_eq!(sampled.skin[0].current[0].rotation, current[0].rotation);
        assert_eq!(
            sampled.skin[0].previous[0].rotation,
            sampled.render[0].previous_pose[0].rotation
        );
        assert_eq!(
            sampled.skin[0].current[0].rotation,
            sampled.render[0].pose[0].rotation
        );
        assert_eq!(authored_state(&store), authored);
    }
    let retained = store.get(1).unwrap();
    assert!(Arc::ptr_eq(&retained.skin_layers[0].previous, &previous));
    assert!(Arc::ptr_eq(&retained.skin_layers[0].current, &current));
}

#[test]
fn draw_only_refreshes_retain_rotation_history_after_perspective_changes_and_resets() {
    for perspective_change in [false, true] {
        let (mut store, mut actors) = fixture(180.0, 0.0);
        let actor = actors.get_mut(&1).unwrap();
        actor.yaw = 90.0;
        actor.head_yaw = 90.0;
        actor.pitch = 40.0;
        actor.metadata.insert(
            0,
            protocol::ActorMetadataValue::Flags(1 << ActorFlag::UsingItem.wire_value()),
        );
        let mut context = ActorTickContext {
            is_local: true,
            is_local_player: true,
            is_local_first_person: perspective_change,
            ..Default::default()
        };
        store.advance_tick(&actors, None, Some(1), true, true, |_| context.clone());
        if perspective_change {
            context.is_local_first_person = false;
        } else {
            store.mark_reset(1);
        }
        context.animation_elapsed_ticks = Some(0);
        store.prepare_frame(&actors, None, Some(1), true, |_| context.clone());
        let tick = store.get(1).unwrap().completed_tick;
        for use_ticks in [1, 2] {
            context.local_item_use_ticks = Some(use_ticks);
            store.prepare_frame(&actors, None, Some(1), true, |_| context.clone());
            for alpha in [0.0, 0.5, 1.0] {
                assert_angles(
                    sample(&store, &actors[&1], alpha).pose[0].translation_scale,
                    [56.25, 40.0, 33.75],
                );
            }
            assert_eq!(store.get(1).unwrap().completed_tick, tick);
        }
    }
}

#[test]
fn isolated_motion_completes_spear_inputs_before_channel_and_weight_sampling() {
    for weight in [false, true] {
        let mut compiled = compiled_fixture(180.0);
        for (axis, name) in [
            "variable.melee_spear_equipped",
            "variable.tp_melee_spear_use_arm_rotation_x",
            "variable.tp_melee_spear_attack_arm_rotation_x",
        ]
        .into_iter()
        .enumerate()
        {
            let slot = symbol(&mut compiled, MolangSymbolKind::Variable, name);
            let original = compiled.animation_keyframes[0].expressions[axis].unwrap();
            let combined = extend(
                &mut compiled,
                original,
                &[
                    MolangOp::LoadVariable(slot),
                    // Authored X offsets are mirrored when the pose is composed.
                    if axis == 0 {
                        MolangOp::Subtract
                    } else {
                        MolangOp::Add
                    },
                ],
            );
            compiled.animation_keyframes[0].expressions[axis] = Some(combined);
        }
        if weight {
            let equipped = compiled
                .molang_symbols
                .iter()
                .position(|symbol| symbol.identifier.as_ref() == "variable.melee_spear_equipped")
                .unwrap() as u32;
            let query = compiled.animation_keyframes[0].expressions[2].unwrap();
            let gate = extend(
                &mut compiled,
                query,
                &[
                    MolangOp::Push(crate::test_support::scalar(0.0)),
                    MolangOp::Multiply,
                    MolangOp::LoadVariable(equipped),
                    MolangOp::Add,
                ],
            );
            compiled.rig_animations[0].weight = Some(gate);
        }
        let (mut store, mut actors) = fixture_with(compiled, 0.0, Default::default());
        actors.get_mut(&1).unwrap().yaw = 90.0;
        actors.get_mut(&1).unwrap().head_yaw = 90.0;
        actors.get_mut(&1).unwrap().pitch = 40.0;
        actors.get_mut(&1).unwrap().metadata.insert(
            0,
            protocol::ActorMetadataValue::Flags(1 << ActorFlag::UsingItem.wire_value()),
        );
        store.sync_local_swing(
            1,
            LocalSwingProgress {
                bedrock: [0.0, 0.15],
                java: [0.0, 0.15],
                frame_alpha: None,
            },
        );
        let context = ActorTickContext {
            is_local: true,
            is_local_player: true,
            main_hand_is_spear: true,
            local_item_use_ticks: Some(5),
            main_hand_kinetic: Some(protocol::KineticWeaponTiming {
                delay_ticks: 12,
                dismount_ticks: 50,
                knockback_ticks: 135,
                damage_ticks: 225,
            }),
            ..Default::default()
        };
        store.advance_tick(&actors, None, Some(1), true, true, |_| context.clone());
        let rig = store.get(1).unwrap();
        let expected = rig.current[0].translation_scale;
        assert!(
            (expected[0] - (90.0 - rig.body_yaw + 1.0)).abs() < 1.0e-4,
            "equipped spear adds one model unit"
        );
        assert_ne!(expected[1], 40.0, "use pose must be active");
        assert_ne!(expected[2], rig.body_yaw, "attack pose must be active");
        let authored = authored_state(&store);
        let sampled = sample(&store, &actors[&1], 1.0);
        assert_angles(
            sampled.pose[0].translation_scale,
            [expected[0], expected[1], expected[2]],
        );
        assert_eq!(authored_state(&store), authored);
    }
}

#[test]
fn camera_sampling_completes_spear_posture_with_the_interpolated_pitch() {
    let mut compiled = compiled_fixture(180.0);
    camera_clip(&mut compiled, true);
    let slot = symbol(
        &mut compiled,
        MolangSymbolKind::Variable,
        "variable.tp_melee_spear_base_arm_rotation_x",
    );
    let pitch = compiled.animation_keyframes[0].expressions[1].unwrap();
    let combined = extend(
        &mut compiled,
        pitch,
        &[MolangOp::LoadVariable(slot), MolangOp::Add],
    );
    compiled.animation_keyframes[0].expressions[1] = Some(combined);
    let (mut store, mut actors) = fixture_with(compiled, 0.0, Default::default());
    let actor = actors.get_mut(&1).unwrap();
    actor.yaw = 90.0;
    actor.head_yaw = 90.0;
    actor.pitch = 40.0;
    actor.metadata.insert(
        0,
        protocol::ActorMetadataValue::Flags(1 << ActorFlag::Swimming.wire_value()),
    );
    store.advance_tick(&actors, None, None, true, true, |_| Default::default());
    let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
    let sampled = store
        .render_layers(
            &actors[&1],
            0.5,
            [0.0; 2],
            [4.0, 0.0, 0.0],
            &mut budget,
            false,
        )
        .unwrap();
    assert!((sampled.render[0].pose[0].translation_scale[1] + 115.0).abs() < 1.0e-4);
}

#[test]
fn inactive_swing_consumers_preserve_turning_players_walk_endpoints() {
    for visibility in [false, true] {
        let mut compiled = compiled_fixture(180.0);
        camera_clip(&mut compiled, false);
        let attack = symbol(
            &mut compiled,
            MolangSymbolKind::Variable,
            "variable.attack_time",
        );
        let attack = expression(&mut compiled, &[MolangOp::LoadVariable(attack)], 1);
        if visibility {
            compiled.render.visibility = vec![assets::EntityRenderVisibility {
                pattern: "rightitem".into(),
                condition: attack,
            }]
            .into();
            compiled.render.layers[0].visibility_count = 1;
        } else {
            let key = compiled.animation_keyframes.last_mut().unwrap();
            key.expressions = [Some(attack), None, None];
        }
        let context = ActorTickContext {
            is_local: true,
            is_local_player: true,
            ..Default::default()
        };
        let (mut store, mut actors) = fixture_with(compiled, 0.0, context.clone());
        actors.get_mut(&1).unwrap().yaw = 90.0;
        actors.get_mut(&1).unwrap().head_yaw = 90.0;
        store.advance_tick(&actors, None, Some(1), true, true, |_| context.clone());
        let rig = store.get(1).unwrap();
        let previous = rig.previous[0].rotation;
        let current = rig.current[0].rotation;
        assert_ne!(previous, current);
        store.sync_local_swing(
            1,
            LocalSwingProgress {
                bedrock: [0.0, 0.5],
                java: [0.0, 0.5],
                frame_alpha: None,
            },
        );
        for alpha in [0.0, 0.25, 0.75] {
            let layer = sample(&store, &actors[&1], alpha);
            assert_eq!(layer.previous_pose[0].rotation, previous);
            assert_eq!(layer.pose[0].rotation, current);
            if visibility {
                assert_eq!(layer.hidden_bones.is_empty(), alpha > 0.0);
            }
        }
    }
}

#[test]
fn body_sampling_retains_render_layers_under_the_same_frame_budget() {
    let (mut store, mut actors) = fixture(10.0, 0.0);
    actors.get_mut(&1).unwrap().head_yaw = 90.0;
    store.advance_tick(&actors, None, None, true, true, |_| Default::default());
    let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.05);
    let body = frame.layers_with_skin(1).unwrap();
    frame.set_remaining_ops(0);
    let render = frame.layers(1).unwrap();
    assert_eq!(render, body.render);
    assert_eq!(frame.remaining_ops(), 0);
}
