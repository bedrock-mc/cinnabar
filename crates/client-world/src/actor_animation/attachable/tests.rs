use super::*;
use crate::actor_animation::pose::quat_from_euler;
use assets::*;

fn scalar(value: f32) -> EntityGeometryScalar {
    EntityGeometryScalar::new(value).unwrap()
}

pub(in crate::actor_animation) fn compiled_fixture() -> CompiledEntityAssets {
    let sources = [
        "animations/item.json",
        "attachables/item.json",
        "models/entity/item.json",
        "render_controllers/item.json",
        "textures/items/item.png",
    ]
    .into_iter()
    .map(|path| EntityAssetSource {
        path: path.into(),
        source_bytes: 1,
        source_sha256: [1; 32],
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let symbols = [
        (EntityAssetKind::Geometry, "geometry.item", 2),
        (EntityAssetKind::Animation, "animation.item", 0),
        (
            EntityAssetKind::RenderController,
            "controller.render.item",
            3,
        ),
        (EntityAssetKind::Texture, "textures/items/item", 4),
        (EntityAssetKind::Attachable, "minecraft:test_item", 1),
    ]
    .into_iter()
    .map(|(kind, identifier, source_index)| EntityAssetSymbol {
        kind,
        identifier: identifier.into(),
        source_index,
        dependencies: Box::new([]),
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    let molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, "query.frame_alpha"),
        (MolangSymbolKind::Query, "query.get_animation_frame"),
        (MolangSymbolKind::Query, "query.main_hand_item_max_duration"),
        (MolangSymbolKind::Query, "query.main_hand_item_use_duration"),
        (MolangSymbolKind::Variable, "context.is_first_person"),
        (MolangSymbolKind::Variable, "context.is_paperdoll"),
        (MolangSymbolKind::Variable, "variable.charge_amount"),
    ]
    .into_iter()
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .collect::<Vec<_>>()
    .into_boxed_slice();
    CompiledEntityAssets {
        source_manifest_sha256: [1; 32],
        block_visual_count: 1,
        sources,
        symbols,
        geometries: vec![EntityGeometry {
            visible_bounds: None,
            identifier: "geometry.item".into(),
            inherits: None,
            source_index: 2,
            texture_width: 16,
            texture_height: 16,
            bones: vec![EntityGeometryBone {
                name: "rightitem".into(),
                binding: None,
                parent: None,
                pivot: Some([scalar(0.0); 3]),
                rotation: None,
                bind_pose_rotation: None,
                mirror: None,
                inflate: None,
                never_render: None,
                reset: None,
                cubes: Box::new([]),
                texture_meshes: Box::new([]),
            }]
            .into_boxed_slice(),
        }]
        .into_boxed_slice(),
        animation_clips: vec![EntityAnimationClip {
            symbol: 1,
            length_seconds: scalar(0.0),
            loop_mode: EntityAnimationLoop::Loop,
            first_channel: 0,
            channel_count: 1,
            source: 0,
            override_previous: false,
            anim_time_update: None,
            geometry: Some(0),
        }]
        .into_boxed_slice(),
        animation_channels: vec![EntityAnimationChannel {
            bone_name: None,
            bone: 0,
            property: EntityAnimationProperty::Translation,
            first_keyframe: 0,
            keyframe_count: 1,
            rotation_relative_to_entity: false,
        }]
        .into_boxed_slice(),
        animation_keyframes: vec![EntityAnimationKeyframe {
            time_seconds: scalar(0.0),
            value: [scalar(0.0); 3],
            interpolation: EntityAnimationInterpolation::Linear,
            expressions: [Some(1), Some(2), None],
        }]
        .into_boxed_slice(),
        molang_symbols,
        molang_expressions: [(0, 11, 3), (11, 1, 1), (12, 1, 1), (13, 1, 1)]
            .into_iter()
            .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
                first_op,
                op_count,
                max_stack,
            })
            .collect::<Vec<_>>()
            .into_boxed_slice(),
        // Native attachable charge formula, expressed through the shared compiled VM.
        molang_ops: vec![
            MolangOp::LoadQuery(3),
            MolangOp::LoadQuery(4),
            MolangOp::LoadQuery(1),
            MolangOp::Subtract,
            MolangOp::Push(scalar(1.0)),
            MolangOp::Add,
            MolangOp::Subtract,
            MolangOp::Push(scalar(10.0)),
            MolangOp::Divide,
            MolangOp::StoreVariable(7),
            MolangOp::Push(scalar(0.0)),
            MolangOp::LoadVariable(5),
            MolangOp::LoadVariable(7),
            MolangOp::LoadQuery(2),
        ]
        .into_boxed_slice(),
        molang_collections: Box::new([]),
        molang_collection_items: Box::new([]),
        controllers: Box::new([]),
        controller_states: Box::new([]),
        controller_animations: Box::new([]),
        controller_transitions: Box::new([]),
        rig_bindings: vec![EntityRigBinding {
            entity_symbol: 4,
            render_controller: 2,
            first_geometry: 0,
            geometry_count: 1,
            fallback: EntityRigFallback::Skip,
            initialize: None,
            pre_animation: Some(0),
            scale: scalar(1.0),
            scale_expressions: None,
        }]
        .into_boxed_slice(),
        rig_geometries: vec![EntityRigGeometryBinding {
            geometry: 0,
            condition: None,
            first_animation: 0,
            animation_count: 1,
            first_controller: 0,
            controller_count: 0,
        }]
        .into_boxed_slice(),
        rig_animations: vec![EntityRigAnimationBinding {
            name: 0,
            clip: 0,
            weight: None,
            order: 0,
        }]
        .into_boxed_slice(),
        rig_controllers: Box::new([]),
        item_visuals: Box::new([]),
        item_visual_aliases: Box::new([]),
        render: EntityRenderData {
            layers: vec![EntityRenderLayer {
                material: Default::default(),
                material_state: None,
                hurt_color: None,
                rig: 0,
                condition: None,
                first_slot: 0,
                slot_count: 1,
                first_visibility: 0,
                visibility_count: 0,
                color: None,
                overlay_color: None,
                on_fire_color: None,
                uv_anim: None,
                first_geometry: 0,
                geometry_count: 0,
                ignore_lighting: false,
                light_color_multiplier: None,
            }]
            .into_boxed_slice(),
            slots: vec![EntityRenderSlot {
                first_candidate: 0,
                candidate_count: 1,
            }]
            .into_boxed_slice(),
            candidates: vec![EntityRenderCandidate {
                condition: None,
                source: 4,
            }]
            .into_boxed_slice(),
            ..EntityRenderData::default()
        },
    }
}

fn fixture() -> Arc<RuntimeEntityAssets> {
    Arc::new(RuntimeEntityAssets::from_compiled(compiled_fixture()).unwrap())
}

pub(super) fn owner_rig() -> ActorRigSnapshot<'static> {
    ActorRigSnapshot {
        actor: ActorLifetimeId {
            session_id: 1,
            dimension: 0,
            runtime_id: 1,
            spawn_revision: 1,
        },
        rig: EntityRigId(0),
        previous: &[],
        current: &[],
        rest: &[],
        rest_completed_tick: 20,
        rest_reset_generation: 0,
        completed_tick: 20,
        reset_generation: 0,
        fallback: EntityRigFallback::Skip,
        scale: 1.0,
        axis_scale: [1.0; 3],
        previous_body_yaw: 0.0,
        body_yaw: 0.0,
        render: &[],
        bone_names: &[],
        skin_geometry: None,
        skin_mesh: None,
        skin_layers: &[],
        hand: [HandPhase::default(); 2],
        item_animation: [ItemAnimationState::default(); 2],
        off_hand_animation: [ItemAnimationState::default(); 2],
        animation_variables: ActorAnimationVariables::default(),
        java: Default::default(),
        java_equipped: None,
    }
}

#[test]
fn authored_attachable_runs_pre_animation_and_context_pose_at_render_alpha() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut runtime = AttachablesRuntime::new(fixture());
    let input = AttachableAnimationInput {
        first_person: true,
        frame_alpha: 0.5,
        use_elapsed_ticks: Some(5),
        max_use_ticks: 100,
        ..AttachableAnimationInput::default()
    };
    let snapshot = runtime
        .evaluate("minecraft:test_item", &owner, &owner_rig(), input)
        .unwrap();
    assert_eq!(snapshot.geometry, 0);
    assert_eq!(snapshot.render[0].source, 4);
    assert!((snapshot.pose[0].translation_scale[0] + 1.0).abs() < 1e-6);
    assert!((snapshot.pose[0].translation_scale[1] - 0.45).abs() < 1e-6);
    let snapshot = runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput {
                first_person: false,
                frame_alpha: 0.0,
                ..input
            },
        )
        .unwrap();
    assert_eq!(snapshot.pose[0].translation_scale[0], 0.0);
    assert!((snapshot.pose[0].translation_scale[1] - 0.4).abs() < 1e-6);
    assert!(
        runtime
            .evaluate("minecraft:unknown", &owner, &owner_rig(), input)
            .is_none()
    );
}

/// Every admitted owner fits; ended sessions release their retained controller state.
#[test]
fn attachable_states_retain_admitted_owners_and_clear_ended_sessions() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut runtime = AttachablesRuntime::new(fixture());
    let input = AttachableAnimationInput {
        first_person: true,
        ..AttachableAnimationInput::default()
    };
    let owners = crate::actor_store::MAX_TRACKED_ACTORS as u64;
    // Distinct actors of one session, then one owner per reconnected session.
    for (session_id, runtime_id) in (0..owners)
        .map(|index| (1, 2 + index))
        .chain((0..owners).map(|index| (2 + index, 2 + owners + index)))
    {
        let mut rig = owner_rig();
        rig.actor.session_id = session_id;
        rig.actor.runtime_id = runtime_id;
        assert!(
            runtime
                .evaluate("minecraft:test_item", &owner, &rig, input)
                .is_some(),
            "session {session_id} owner {runtime_id} lost its attachable"
        );
        assert!(runtime.states.len() <= MAX_ATTACHABLE_STATES);
        if session_id == 1 {
            assert_eq!(runtime.states.len() as u64, runtime_id - 1);
        }
    }
    assert_eq!(runtime.states.len(), 1, "ended sessions keep no state");
}

#[test]
fn offhand_equip_clock_starts_low_and_swaps_independently_of_the_main_hand() {
    let assets = fixture();
    let layout = VariableLayout::new(&assets);
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut state = resolve_binding(&assets, &layout, &owner, 0, 0).unwrap();
    assert_eq!(
        state.off_hand_animation.map(|phase| phase.arm_height),
        [0.0; 2]
    );
    let mut context = ActorTickContext {
        main_hand: Some(Arc::from("minecraft:stone")),
        off_hand: Some(Arc::from("minecraft:shield")),
        ..ActorTickContext::default()
    };
    for expected in [0.0, 0.4, 0.8, 1.0] {
        advance_motion(&mut state, &owner, &context, false);
        assert!((state.off_hand_animation[1].arm_height - expected).abs() < 1e-6);
        assert_eq!(state.history.back().unwrap().arm_height, 1.0);
        assert_eq!(state.equipped_off, context.off_hand);
    }
    context.main_hand = Some(Arc::from("minecraft:dirt"));
    advance_motion(&mut state, &owner, &context, false);
    assert!((state.history.back().unwrap().arm_height - 0.6).abs() < 1e-6);
    assert_eq!(state.off_hand_animation[1].arm_height, 1.0);
    context.off_hand = Some(Arc::from("minecraft:arrow"));
    for expected in [0.6, 0.2] {
        advance_motion(&mut state, &owner, &context, false);
        assert!((state.off_hand_animation[1].arm_height - expected).abs() < 1e-6);
        assert_eq!(state.equipped_off.as_deref(), Some("minecraft:shield"));
    }
    advance_motion(&mut state, &owner, &context, false);
    assert_eq!(state.off_hand_animation[1].arm_height, 0.0);
    assert_eq!(state.equipped_off, context.off_hand);
    for expected in [0.4, 0.8, 1.0] {
        advance_motion(&mut state, &owner, &context, false);
        assert!((state.off_hand_animation[1].arm_height - expected).abs() < 1e-6);
    }
    // A pose/geometry reset does not reconstruct the independent item renderer clock.
    state.reset_pending = true;
    advance_motion(&mut state, &owner, &context, true);
    assert_eq!(state.history.len(), 1);
    assert_eq!(
        state.off_hand_animation.map(|phase| phase.arm_height),
        [1.0; 2]
    );
    context.off_hand = None;
    for expected in [0.6, 0.2, 0.0, 0.4] {
        advance_motion(&mut state, &owner, &context, false);
        assert!((state.off_hand_animation[1].arm_height - expected).abs() < 1e-6);
    }
    assert!(state.equipped_off.is_none());
    assert!(
        state
            .off_hand_animation
            .iter()
            .all(|phase| phase.attack_time == 0.0)
    );
}

#[test]
fn offhand_arm_context_interpolates_its_own_clock_through_the_shared_vm() {
    let mut compiled = compiled_fixture();
    compiled.molang_symbols[7].identifier = "context.player_offhand_arm_height".into();
    let mut symbols = compiled.molang_symbols.to_vec();
    symbols.push(MolangSymbol {
        kind: MolangSymbolKind::Variable,
        identifier: "variable.player_arm_height".into(),
    });
    compiled.molang_symbols = symbols.into_boxed_slice();
    compiled.rig_bindings[0].pre_animation = None;
    let mut ops = compiled.molang_ops.to_vec();
    ops.push(MolangOp::LoadVariable(8));
    compiled.molang_ops = ops.into_boxed_slice();
    let mut expressions = compiled.molang_expressions.to_vec();
    expressions.push(CompiledMolangExpression {
        first_op: 14,
        op_count: 1,
        max_stack: 1,
    });
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.animation_keyframes[0].expressions[0] = Some(4);
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let rig = ActorRigSnapshot {
        item_animation: [0.1, 0.3].map(|arm_height| ItemAnimationState {
            arm_height,
            attack_time: 0.0,
        }),
        off_hand_animation: [0.2, 0.8].map(|arm_height| ItemAnimationState {
            arm_height,
            attack_time: 0.0,
        }),
        ..owner_rig()
    };
    let mut runtime = AttachablesRuntime::new(assets);
    for alpha in [0.0, 0.5, 1.0] {
        let snapshot = runtime
            .evaluate(
                "minecraft:test_item",
                &owner,
                &rig,
                AttachableAnimationInput {
                    first_person: true,
                    off_hand: true,
                    frame_alpha: alpha,
                    ..AttachableAnimationInput::default()
                },
            )
            .unwrap();
        assert!((snapshot.pose[0].translation_scale[0] + 0.1 + 0.2 * alpha).abs() < 1e-6);
        assert!((snapshot.pose[0].translation_scale[1] - 0.2 - 0.6 * alpha).abs() < 1e-6);
    }
}

#[test]
fn owner_name_binding_clears_defaults_but_explicit_binding_keeps_native_origin() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let names = [Box::<str>::from("rightItem")];
    let owner_rig = ActorRigSnapshot {
        bone_names: &names,
        ..owner_rig()
    };
    for explicit in [false, true] {
        let mut compiled = compiled_fixture();
        let bone = &mut compiled.geometries[0].bones[0];
        bone.pivot = Some([scalar(2.0), scalar(12.0), scalar(4.0)]);
        bone.rotation = Some([scalar(0.0), scalar(0.0), scalar(45.0)]);
        if explicit {
            bone.name = "shield".into();
            bone.binding = Some("q.item_slot_to_bone_name(c.item_slot)".into());
        }
        compiled.animation_keyframes[0].value = [scalar(5.0), scalar(6.0), scalar(7.0)];
        compiled.animation_keyframes[0].expressions = [None; 3];
        compiled.rig_bindings[0].pre_animation = None;
        let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
        let mut runtime = AttachablesRuntime::new(Arc::clone(&assets));
        let snapshot = runtime
            .evaluate(
                "minecraft:test_item",
                &owner,
                &owner_rig,
                AttachableAnimationInput {
                    first_person: true,
                    ..AttachableAnimationInput::default()
                },
            )
            .unwrap();
        if explicit {
            assert_eq!(snapshot.pose[0].translation_scale[..3], [-7.0, -6.0, 11.0]);
            assert_eq!(snapshot.pose[0].rotation, quat_from_euler([0.0, 0.0, 45.0]));
        } else {
            assert_eq!(snapshot.pose[0].translation_scale[..3], [-5.0, 6.0, 7.0]);
            assert_eq!(snapshot.pose[0].rotation, quat_from_euler([0.0; 3]));
        }
        // Pivots remain authored in the catalog: shaders and children still need the bind frame.
        assert_eq!(
            assets.geometries()[0].bones[0]
                .pivot
                .unwrap()
                .map(|v| v.get()),
            [2.0, 12.0, 4.0]
        );
    }
}

#[test]
fn attachable_queries_are_remaining_ticks_without_changing_entity_units() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let input = ActorTickInput {
        item_use_ticks: 5,
        ..ActorTickInput::default()
    };
    let mut context = ActorTickContext {
        main_hand_max_use_ticks: 100,
        ..ActorTickContext::default()
    };
    let read_with_args =
        |context: &ActorTickContext, name: &str, arguments: &[evaluation::MolangValue]| {
            query::query(
                &query::QueryInputs {
                    actor: &owner,
                    input: &input,
                    context,
                    anim_tick: 2,
                    anim_time: None,
                    life_tick: 10,
                    finished: (false, false),
                    bones: &[],
                    bone_names: &[],
                },
                name,
                arguments,
            )
        };
    let read = |context: &ActorTickContext, name: &str| read_with_args(context, name, &[]);
    assert_eq!(
        read(&context, "query.main_hand_item_use_duration").number(),
        0.25
    );
    context.attachable = Some(AttachableQueryContext {
        worn: false,
        first_person: true,
        off_hand: false,
        is_paperdoll: false,
        frame_alpha: 0.5,
        animation_frame: 3,
        delta_seconds: None,
        use_elapsed_ticks: Some(5),
        max_use_ticks: 100,
        owner_life_tick: 10,
    });
    assert_eq!(
        read(&context, "query.main_hand_item_use_duration").number(),
        95.0
    );
    assert_eq!(
        read(&context, "query.main_hand_item_max_duration").number(),
        100.0
    );
    assert_eq!(read(&context, "query.get_animation_frame").number(), 3.0);
    assert_eq!(read(&context, "query.frame_alpha").number(), 0.5);
    assert!((read(&context, "query.life_time").number() - 0.525).abs() < 1e-6);
    assert_eq!(
        read(&context, "query.owner_identifier"),
        evaluation::MolangValue::String("minecraft:test".into())
    );
    assert_eq!(
        read_with_args(
            &context,
            "query.item_slot_to_bone_name",
            &[evaluation::MolangValue::String("main_hand".into())]
        ),
        evaluation::MolangValue::String("rightitem".into())
    );
    context.attachable.as_mut().unwrap().off_hand = true;
    assert_eq!(
        read_with_args(
            &context,
            "query.item_slot_to_bone_name",
            &[evaluation::MolangValue::String("off_hand".into())]
        ),
        evaluation::MolangValue::String("leftitem".into())
    );
    context.attachable.as_mut().unwrap().use_elapsed_ticks = None;
    assert_eq!(
        read(&context, "query.main_hand_item_use_duration").number(),
        0.0
    );
    assert_eq!(
        read(&context, "query.item_slot_to_bone_name"),
        evaluation::MolangValue::String("".into())
    );
    assert_eq!(
        read_with_args(
            &context,
            "query.item_slot_to_bone_name",
            &[evaluation::MolangValue::String("head".into())]
        ),
        evaluation::MolangValue::String("head".into())
    );
}

#[test]
fn owner_variables_copy_by_name_between_catalogs_with_different_slots() {
    let target = fixture();
    let mut compiled = compiled_fixture();
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.insert(
        5,
        MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "context.a_unused".into(),
        },
    );
    compiled.molang_symbols = symbols.into_boxed_slice();
    for op in compiled.molang_ops.iter_mut() {
        if let MolangOp::LoadVariable(index) | MolangOp::StoreVariable(index) = op {
            *index += 1;
        }
    }
    let owner = RuntimeEntityAssets::from_compiled(compiled).unwrap();
    let owner_layout = VariableLayout::new(&owner);
    let target_layout = VariableLayout::new(&target);
    let mut values = owner_layout.fresh(1);
    values.set(
        owner_layout.named_slot(&owner, "variable.charge_amount"),
        0.75,
    );
    let inherited = ActorAnimationVariables::new(Some(&owner), &values, 77);
    let mut copied = target_layout.fresh(1);
    inherited.copy_to(&target, &target_layout, &mut copied);
    assert_eq!(
        copied.get(target_layout.named_slot(&target, "variable.charge_amount")),
        Some(0.75)
    );
    assert_eq!(inherited.life_tick(), 77);
}

#[test]
fn shield_predicate_reads_blocking_metadata_and_both_owner_hands() {
    let mut compiled = compiled_fixture();
    let mut symbols = compiled.molang_symbols.into_vec();
    let blocking = symbols.len() as u32;
    symbols.extend(
        [
            (MolangSymbolKind::Query, "query.blocking"),
            (MolangSymbolKind::Query, "query.is_item_name_any"),
            (MolangSymbolKind::String, "slot.weapon.mainhand"),
            (MolangSymbolKind::String, "slot.weapon.offhand"),
            (MolangSymbolKind::String, "minecraft:shield"),
        ]
        .map(|(kind, identifier)| MolangSymbol {
            kind,
            identifier: identifier.into(),
        }),
    );
    let mut ops = compiled.molang_ops.into_vec();
    let mut expressions = compiled.molang_expressions.into_vec();
    let predicate = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 10,
        max_stack: 3,
    });
    // Shield's exact main-hand predicate; the same production VM evaluates its scripts.
    ops.extend([
        MolangOp::LoadQuery(blocking),
        MolangOp::PushString(blocking + 3),
        MolangOp::PushString(blocking + 4),
        MolangOp::CallQuery(MolangCall {
            symbol: blocking + 1,
            arguments: 2,
        }),
        MolangOp::Not,
        MolangOp::Multiply,
        MolangOp::PushString(blocking + 2),
        MolangOp::PushString(blocking + 4),
        MolangOp::CallQuery(MolangCall {
            symbol: blocking + 1,
            arguments: 2,
        }),
        MolangOp::Multiply,
    ]);
    let mut order = symbols.into_iter().enumerate().collect::<Vec<_>>();
    order.sort_by(|(_, a), (_, b)| (a.kind, &a.identifier).cmp(&(b.kind, &b.identifier)));
    let mut remap = vec![0; order.len()];
    for (new, (old, _)) in order.iter().enumerate() {
        remap[*old] = new as u32;
    }
    for op in &mut ops {
        match op {
            MolangOp::LoadQuery(index)
            | MolangOp::LoadVariable(index)
            | MolangOp::StoreVariable(index)
            | MolangOp::PushString(index) => {
                *index = remap[*index as usize];
            }
            MolangOp::CallQuery(call) => call.symbol = remap[call.symbol as usize],
            _ => {}
        }
    }
    compiled.molang_symbols = order
        .into_iter()
        .map(|(_, symbol)| symbol)
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compiled.molang_ops = ops.into_boxed_slice();
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.animation_keyframes[0].expressions[0] = Some(predicate);
    let mut runtime = AttachablesRuntime::new(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    let key = crate::actor_store::EXTENDED_FLAGS_METADATA_KEY;
    let shield = Some("minecraft:shield");
    for (is_blocking, off_hand, expected) in [
        (false, None, 0.0),
        (true, None, -1.0),
        (true, Some("minecraft:bow"), -1.0),
        (true, shield, 0.0),
    ] {
        let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::from([
            // Sneak alone never overrides the authoritative q.blocking callback.
            (0, ActorMetadataValue::Flags(1 << query::FLAG_SNEAKING)),
            (
                key,
                ActorMetadataValue::FlagsExtended(
                    u64::from(is_blocking) << (query::FLAG_BLOCKING - 64),
                ),
            ),
        ]));
        let snapshot = runtime
            .evaluate(
                "minecraft:test_item",
                &owner,
                &owner_rig(),
                AttachableAnimationInput {
                    first_person: true,
                    owner_main_hand: shield,
                    owner_off_hand: off_hand,
                    ..AttachableAnimationInput::default()
                },
            )
            .unwrap();
        assert_eq!(snapshot.pose[0].translation_scale[0], expected);
    }
}

#[test]
fn hand_context_is_a_string_for_shield_controller_selection() {
    let mut compiled = compiled_fixture();
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.insert(
        7,
        MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "context.item_slot".into(),
        },
    );
    let main_hand = symbols.len() as u32;
    symbols.push(MolangSymbol {
        kind: MolangSymbolKind::String,
        identifier: "main_hand".into(),
    });
    compiled.molang_symbols = symbols.into_boxed_slice();
    let mut ops = compiled.molang_ops.into_vec();
    for op in &mut ops {
        if let MolangOp::LoadVariable(index) | MolangOp::StoreVariable(index) = op
            && *index >= 7
        {
            *index += 1;
        }
    }
    let mut expressions = compiled.molang_expressions.into_vec();
    let selection = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 3,
        max_stack: 2,
    });
    // The exact typed condition used by the vanilla shield wield controller.
    ops.extend([
        MolangOp::LoadVariable(7),
        MolangOp::PushString(main_hand),
        MolangOp::Equal,
    ]);
    compiled.molang_ops = ops.into_boxed_slice();
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.animation_keyframes[0].expressions[0] = Some(selection);
    let mut runtime = AttachablesRuntime::new(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let main = runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput {
                first_person: true,
                ..AttachableAnimationInput::default()
            },
        )
        .unwrap();
    assert!((main.pose[0].translation_scale[0] + 1.0).abs() < 1e-6);
    let off = runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput {
                first_person: true,
                off_hand: true,
                ..AttachableAnimationInput::default()
            },
        )
        .unwrap();
    assert_eq!(off.pose[0].translation_scale[0], 0.0);
}

#[test]
fn standby_and_three_pulling_frames_choose_matching_geometry_texture_and_pose() {
    let mut compiled = compiled_fixture();
    compiled.geometries[0].bones[0].binding = Some("q.item_slot_to_bone_name(c.item_slot)".into());
    let mut symbols = compiled.symbols.into_vec();
    let mut geometries = compiled.geometries.into_vec();
    let mut sources = compiled.sources.into_vec();
    for frame in 1..4 {
        let mut geometry = geometries[0].clone();
        geometry.identifier = format!("geometry.item_frame_{frame}").into();
        geometry.bones[0].pivot = Some([scalar(0.0), scalar(frame as f32 * 3.0), scalar(0.0)]);
        symbols.insert(
            frame,
            EntityAssetSymbol {
                kind: EntityAssetKind::Geometry,
                identifier: geometry.identifier.clone(),
                source_index: 2,
                dependencies: Box::new([]),
            },
        );
        geometries.push(geometry);
        sources.push(EntityAssetSource {
            path: format!("textures/items/item_frame_{frame}.png").into(),
            source_bytes: 1,
            source_sha256: [1; 32],
        });
    }
    for frame in 1..4 {
        symbols.insert(
            6 + frame,
            EntityAssetSymbol {
                kind: EntityAssetKind::Texture,
                identifier: format!("textures/items/item_frame_{frame}").into(),
                source_index: 4 + frame as u32,
                dependencies: Box::new([]),
            },
        );
    }
    compiled.symbols = symbols.into_boxed_slice();
    compiled.sources = sources.into_boxed_slice();
    compiled.geometries = geometries.into_boxed_slice();
    compiled.rig_bindings[0].entity_symbol = 10;
    compiled.rig_bindings[0].render_controller = 5;
    let clip = compiled.animation_clips[0];
    let channel = compiled.animation_channels[0].clone();
    let keyframe = compiled.animation_keyframes[0];
    compiled.animation_clips = (0..4)
        .map(|frame| EntityAnimationClip {
            symbol: 4,
            first_channel: frame,
            geometry: Some(frame),
            ..clip
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compiled.animation_channels = (0..4)
        .map(|frame| EntityAnimationChannel {
            bone_name: None,
            first_keyframe: frame,
            ..channel.clone()
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compiled.animation_keyframes = vec![keyframe; 4].into_boxed_slice();
    let mut expressions = compiled.molang_expressions.into_vec();
    let mut ops = compiled.molang_ops.into_vec();
    for frame in 0..4 {
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 3,
            max_stack: 2,
        });
        ops.extend([
            MolangOp::LoadQuery(2),
            MolangOp::Push(scalar(frame as f32)),
            MolangOp::Equal,
        ]);
    }
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.molang_ops = ops.into_boxed_slice();
    compiled.render.layers[0].geometry_count = 4;
    compiled.render.slots[0].candidate_count = 4;
    compiled.render.geometries = (0..4)
        .map(|frame| EntityRenderGeometry {
            condition: Some(4 + frame),
            geometry: frame,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    compiled.render.candidates = (0..4)
        .map(|frame| EntityRenderCandidate {
            condition: Some(4 + frame),
            source: 4 + frame,
        })
        .collect::<Vec<_>>()
        .into_boxed_slice();
    let mut runtime = AttachablesRuntime::new(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let names = [Box::<str>::from("rightitem")];
    let owner_rig = ActorRigSnapshot {
        bone_names: &names,
        ..owner_rig()
    };
    for frame in 0..4 {
        let snapshot = runtime
            .evaluate(
                "minecraft:test_item",
                &owner,
                &owner_rig,
                AttachableAnimationInput {
                    first_person: true,
                    frame_alpha: 0.5,
                    animation_frame: frame,
                    use_elapsed_ticks: Some(5),
                    max_use_ticks: 100,
                    ..AttachableAnimationInput::default()
                },
            )
            .unwrap();
        let layer = &snapshot.render[0];
        assert_eq!(layer.source, 4 + frame);
        assert_eq!(layer.geometry.unwrap_or(snapshot.geometry), frame);
        let pose = if layer.geometry.is_some() {
            layer.pose.as_ref()
        } else {
            snapshot.pose
        };
        assert_eq!(pose.len(), 1);
        assert!((pose[0].translation_scale[0] + 1.0).abs() < 1e-6);
        let expected_y = frame as f32 * 3.0 - pose::MODEL_PART_ORIGIN_Y + 0.45;
        assert!((pose[0].translation_scale[1] - expected_y).abs() < 1e-6);
    }
}

/// Explicit local diagnostic: native packs and carriers never enter the test fixtures or git.
#[test]
#[ignore = "requires CINNABAR_ATTACHABLE_DIAGNOSTIC_CARRIER pointing to a local entity carrier"]
fn downloaded_vanilla_attachables_evaluate_at_idle() {
    let path = std::env::var("CINNABAR_ATTACHABLE_DIAGNOSTIC_CARRIER").unwrap();
    let assets = Arc::new(RuntimeEntityAssets::decode(&std::fs::read(path).unwrap()).unwrap());
    let mut runtime = AttachablesRuntime::new(Arc::clone(&assets));
    let mut owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    owner.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "diagnostic".into(),
    };
    for identifier in ["minecraft:bow", "minecraft:crossbow", "minecraft:shield"] {
        let snapshot = runtime
            .evaluate(
                identifier,
                &owner,
                &owner_rig(),
                AttachableAnimationInput {
                    first_person: true,
                    frame_alpha: 0.5,
                    ..AttachableAnimationInput::default()
                },
            )
            .expect("installed attachable must evaluate");
        assert!(!snapshot.pose.is_empty());
        assert!(!snapshot.render.is_empty());
        let geometry = &assets.geometries()[snapshot.geometry as usize];
        eprintln!(
            "{identifier}: geometry={} bones={:?} pose={:?} layers={:?}",
            geometry.identifier,
            snapshot.bone_names,
            snapshot.pose,
            snapshot
                .render
                .iter()
                .map(|layer| (
                    layer.geometry,
                    &assets.sources()[layer.source as usize].path
                ))
                .collect::<Vec<_>>()
        );
    }
}

/// Runs the installed pack's actual Shield controller and keyframes, without modifying a server.
#[test]
#[ignore = "requires CINNABAR_ATTACHABLE_DIAGNOSTIC_CARRIER pointing to a local entity carrier"]
fn downloaded_shield_blocking_uses_authoritative_metadata_and_hand_priority() {
    let path = std::env::var("CINNABAR_ATTACHABLE_DIAGNOSTIC_CARRIER").unwrap();
    let assets = Arc::new(RuntimeEntityAssets::decode(&std::fs::read(path).unwrap()).unwrap());
    let mut runtime = AttachablesRuntime::new(assets);
    let mut owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    owner.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "diagnostic".into(),
    };
    let mut sample = |blocking: bool, off_hand: bool, other_shield: bool| {
        runtime.clear();
        owner.metadata.insert(
            crate::actor_store::EXTENDED_FLAGS_METADATA_KEY,
            ActorMetadataValue::FlagsExtended(u64::from(blocking) << (query::FLAG_BLOCKING - 64)),
        );
        let mut pose = Vec::new();
        for completed_tick in 1..=4 {
            let rig = ActorRigSnapshot {
                completed_tick,
                ..owner_rig()
            };
            pose = runtime
                .evaluate(
                    "minecraft:shield",
                    &owner,
                    &rig,
                    AttachableAnimationInput {
                        first_person: true,
                        off_hand,
                        owner_main_hand: Some("minecraft:shield"),
                        owner_off_hand: (off_hand || other_shield).then_some("minecraft:shield"),
                        ..AttachableAnimationInput::default()
                    },
                )
                .unwrap()
                .pose
                .to_vec();
        }
        pose
    };
    let main_idle = sample(false, false, false);
    let main_block = sample(true, false, false);
    assert_ne!(
        main_idle, main_block,
        "pack main-hand blocking keyframes must be selected"
    );
    assert_eq!(
        main_idle,
        sample(true, false, true),
        "offhand Shield takes blocking priority"
    );
    let off_idle = sample(false, true, false);
    assert_ne!(
        off_idle,
        sample(true, true, false),
        "pack offhand blocking keyframes must be selected"
    );
}

#[test]
fn offhand_keeps_owner_bow_use_timing_without_main_hand_charge_frame() {
    let owner = AttachableAnimationInput {
        first_person: true,
        use_elapsed_ticks: Some(10),
        max_use_ticks: 100,
        animation_frame: 3,
        hand_charged: true,
        frame_alpha: 0.5,
        owner_main_hand: Some("minecraft:bow"),
        owner_off_hand: Some("minecraft:shield"),
        ..Default::default()
    };
    let off = owner.for_hand(true);
    assert_eq!(off.use_elapsed_ticks, owner.use_elapsed_ticks);
    assert_eq!(off.max_use_ticks, owner.max_use_ticks);
    assert_eq!(off.owner_main_hand, owner.owner_main_hand);
    assert!(off.off_hand);
    assert!(!off.hand_charged);
    assert_eq!(off.animation_frame, 0);
}

/// Runs the pinned shield's bow-retraction script and authored keyframes offline.
#[test]
#[ignore = "requires CINNABAR_ATTACHABLE_DIAGNOSTIC_CARRIER pointing to a local entity carrier"]
fn downloaded_offhand_shield_retracts_while_owner_draws_bow() {
    let path = std::env::var("CINNABAR_ATTACHABLE_DIAGNOSTIC_CARRIER").unwrap();
    let assets = Arc::new(RuntimeEntityAssets::decode(&std::fs::read(path).unwrap()).unwrap());
    let mut runtime = AttachablesRuntime::new(assets);
    let mut owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    owner.kind = ActorKind::Player {
        uuid: [0; 16],
        username: "diagnostic".into(),
    };
    let mut sample = |using: bool| {
        runtime.clear();
        let input = AttachableAnimationInput {
            first_person: true,
            use_elapsed_ticks: using.then_some(5),
            max_use_ticks: 100,
            owner_main_hand: Some("minecraft:bow"),
            owner_off_hand: Some("minecraft:shield"),
            ..Default::default()
        }
        .for_hand(true);
        let snapshot = runtime
            .evaluate("minecraft:shield", &owner, &owner_rig(), input)
            .unwrap();
        let bone = snapshot
            .bone_names
            .iter()
            .position(|name| name.as_ref() == "shield")
            .unwrap();
        snapshot.pose[bone].translation_scale[2]
    };
    // resource_pack/attachables/shield.entity.json:36-45; animations/shield.animation.json:17.
    assert!((sample(true) - sample(false) + 30.1).abs() < 0.001);
}

#[path = "worn_tests.rs"]
mod worn_tests;

#[test]
fn local_attachable_swing_samples_the_physics_fraction() {
    let mut compiled = compiled_fixture();
    compiled.rig_bindings[0].pre_animation = None;
    compiled.molang_symbols[7].identifier = "variable.attack_time".into();
    compiled.molang_ops[11] = MolangOp::LoadVariable(7);
    compiled.animation_keyframes[0].expressions = [Some(1), None, None];
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut runtime = AttachablesRuntime::new(assets);
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut rig = owner_rig();
    rig.item_animation[0].attack_time = 0.25;
    rig.item_animation[1].attack_time = 0.5;
    rig.java.local_swing_alpha = Some(0.75);
    for actor_alpha in [0.0, 0.25, 1.0] {
        let snapshot = runtime
            .evaluate(
                "minecraft:test_item",
                &owner,
                &rig,
                AttachableAnimationInput {
                    first_person: true,
                    frame_alpha: actor_alpha,
                    ..Default::default()
                },
            )
            .unwrap();
        assert!((snapshot.pose[0].translation_scale[0] + 0.4375).abs() < 1e-6);
    }
}
