use super::tests::{compiled_fixture, owner_rig};
use super::*;
use assets::*;

/// Compiles incremental scripts and clocks, optionally selecting another geometry or failing its clock.
fn fixture(multiple_geometry: bool, fail_selected: bool) -> Arc<RuntimeEntityAssets> {
    let mut compiled = compiled_fixture();
    compiled.molang_symbols[1].identifier = "query.anim_time".into();
    let mut ops = compiled.molang_ops.into_vec();
    let mut expressions = compiled.molang_expressions.into_vec();
    let script = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 5,
        max_stack: 2,
    });
    ops.extend([
        MolangOp::LoadVariable(7),
        MolangOp::Push(EntityGeometryScalar::new(1.0).unwrap()),
        MolangOp::Add,
        MolangOp::StoreVariable(7),
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
    ]);
    compiled.rig_bindings[0].pre_animation = Some(script);
    let clock = expressions.len() as u32;
    let clock_ops = if fail_selected {
        let mut ops = vec![MolangOp::LoadQuery(2)];
        for _ in 0..7 {
            ops.extend([
                MolangOp::Push(EntityGeometryScalar::new(1_000_000.0).unwrap()),
                MolangOp::Multiply,
            ]);
        }
        ops
    } else {
        vec![
            MolangOp::LoadQuery(1),
            MolangOp::Push(EntityGeometryScalar::new(0.25).unwrap()),
            MolangOp::Add,
        ]
    };
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: clock_ops.len() as u16,
        max_stack: 2,
    });
    ops.extend(clock_ops);
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    compiled.animation_clips[0].anim_time_update = Some(clock);
    compiled.animation_clips[0].length_seconds = EntityGeometryScalar::new(100.0).unwrap();
    if multiple_geometry {
        let mut ops = compiled.molang_ops.into_vec();
        let mut expressions = compiled.molang_expressions.into_vec();
        let condition = expressions.len() as u32;
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 2,
            max_stack: 1,
        });
        ops.extend([MolangOp::LoadQuery(2), MolangOp::Truthy]);
        compiled.molang_ops = ops.into();
        compiled.molang_expressions = expressions.into();
        let mut geometries = compiled.geometries.into_vec();
        let mut selected = geometries[0].clone();
        selected.identifier = "geometry.preview".into();
        geometries.push(selected);
        compiled.geometries = geometries.into();
        let mut symbols = compiled.symbols.into_vec();
        symbols.insert(
            1,
            EntityAssetSymbol {
                kind: EntityAssetKind::Geometry,
                identifier: "geometry.preview".into(),
                source_index: 2,
                dependencies: Box::new([]),
            },
        );
        compiled.symbols = symbols.into();
        compiled.rig_bindings[0].entity_symbol += 1;
        compiled.rig_bindings[0].render_controller += 1;
        compiled.animation_clips[0].symbol += 1;
        let mut animations = compiled.rig_animations.into_vec();
        animations.push(animations[0]);
        compiled.rig_animations = animations.into();
        let mut bindings = compiled.rig_geometries.into_vec();
        bindings.push(EntityRigGeometryBinding {
            geometry: 1,
            condition: Some(condition),
            first_animation: 1,
            ..bindings[0]
        });
        compiled.rig_geometries = bindings.into();
        compiled.rig_bindings[0].geometry_count = 2;
        compiled.animation_clips[0].geometry = None;
    }
    Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap())
}

/// Reads the retained script counter and clip clock directly, before any further evaluation.
fn dynamic_state(runtime: &AttachablesRuntime) -> (f32, f32) {
    let rig = &runtime.states.values().next().unwrap().rig;
    (
        rig.variables
            .get(
                runtime
                    .layout
                    .named_slot(&runtime.assets, "variable.charge_amount"),
            )
            .unwrap_or(0.0),
        rig.clip_clocks
            .values()
            .next()
            .map_or(0.0, |clock| clock.time),
    )
}

#[test]
fn attachable_preview_commits_reused_scripts_and_clocks_once() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut runtime = AttachablesRuntime::new(fixture(false, false));
    runtime.begin_preview();
    let result = runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput::default(),
        )
        .unwrap();
    assert_eq!(result.pose[0].translation_scale[1], 1.0);
    assert_eq!(dynamic_state(&runtime), (0.0, 0.0));
    assert_eq!(runtime.commits, 0);
    runtime.end_preview();
    runtime.finish_preview(true);
    assert_eq!(dynamic_state(&runtime), (1.0, 0.25));
    assert_eq!(runtime.commits, 1);
    runtime.finish_preview(true);
    assert_eq!(runtime.commits, 1);
}

#[test]
fn attachable_preview_changed_source_evaluates_original_committed_scripts_and_clocks() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut runtime = AttachablesRuntime::new(fixture(false, false));
    runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput::default(),
        )
        .unwrap();
    runtime.begin_preview();
    runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput::default(),
        )
        .unwrap();
    assert_eq!(dynamic_state(&runtime), (1.0, 0.25));
    runtime.end_preview();
    runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput {
                frame_alpha: 0.5,
                ..Default::default()
            },
        )
        .unwrap();
    runtime.finish_preview(false);
    assert_eq!(dynamic_state(&runtime), (2.0, 0.5));
    assert_eq!(runtime.commits, 2);
}

/// Captures geometry-owned buffers and every selector flag to detect copying or incomplete rollback.
fn geometry_state(
    runtime: &AttachablesRuntime,
) -> (usize, [usize; 7], [bool; 5], EntityRigId, String) {
    let rig = &runtime.states.values().next().unwrap().rig;
    (
        rig.geometry_binding,
        [
            rig.bones.as_ptr() as usize,
            rig.bone_names.as_ptr() as usize,
            rig.controllers.as_ptr() as usize,
            rig.previous.as_ptr() as usize,
            rig.rest.as_ptr() as usize,
            rig.current.as_ptr() as usize,
            rig.ui_pose
                .as_ref()
                .map_or(0, |pose| pose.as_ptr() as usize),
        ],
        [
            rig.samples_camera_poses,
            rig.samples_swing_poses,
            rig.samples_render_frames,
            rig.reset_pending,
            rig.rest_reset_pending,
        ],
        rig.rig,
        format!("{:?}", rig.ui_animation),
    )
}

/// Seeds independent UI state so geometry rollback also proves that component is preserved.
fn seed_ui(runtime: &mut AttachablesRuntime, owner: &ActorSnapshot) {
    let state = &mut runtime.states.values_mut().next().unwrap().rig;
    let mut world_left = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    let mut budget = EvalBudget {
        actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
        world_left: &mut world_left,
        work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
        transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
        used: 0,
        stack: Vec::new(),
        static_draw: None,
    };
    let tick = state.completed_tick;
    hud::evaluate(
        &runtime.assets,
        &runtime.layout,
        state,
        owner,
        &ActorTickContext::default(),
        tick,
        &mut budget,
        false,
    );
}

#[test]
fn attachable_preview_geometry_discard_restores_owned_buffers_and_reuse_commits_selected_geometry()
{
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut runtime = AttachablesRuntime::new(fixture(true, false));
    runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput::default(),
        )
        .unwrap();
    seed_ui(&mut runtime, &owner);
    let before = geometry_state(&runtime);
    runtime.begin_preview();
    let selected = runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput {
                animation_frame: 1,
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(selected.geometry, 1);
    assert_eq!(dynamic_state(&runtime), (1.0, 0.25));
    runtime.end_preview();
    runtime.finish_preview(false);
    assert_eq!(
        geometry_state(&runtime),
        before,
        "rollback restores buffers without cloning them"
    );
    assert_eq!(dynamic_state(&runtime), (1.0, 0.25));
    runtime.begin_preview();
    runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput {
                animation_frame: 1,
                ..Default::default()
            },
        )
        .unwrap();
    runtime.end_preview();
    runtime.finish_preview(true);
    assert_eq!(
        runtime.states.values().next().unwrap().rig.geometry_binding,
        1
    );
    assert_eq!(runtime.commits, 2);
    assert!(runtime.states.values().next().unwrap().preview.is_none());
}

#[test]
fn attachable_preview_rejected_evaluation_restores_geometry_and_committed_dynamic_state() {
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    let mut runtime = AttachablesRuntime::new(fixture(true, true));
    runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput::default(),
        )
        .unwrap();
    seed_ui(&mut runtime, &owner);
    let before = geometry_state(&runtime);
    let dynamic = dynamic_state(&runtime);
    runtime.begin_preview();
    assert!(
        runtime
            .evaluate(
                "minecraft:test_item",
                &owner,
                &owner_rig(),
                AttachableAnimationInput {
                    animation_frame: 1,
                    ..Default::default()
                }
            )
            .is_none()
    );
    runtime.end_preview();
    runtime.finish_preview(true);
    assert_eq!(geometry_state(&runtime), before);
    assert_eq!(dynamic_state(&runtime), dynamic);
    assert_eq!(runtime.commits, 1);
}

#[test]
fn attachable_preview_missing_render_returns_no_draw_and_commits_authored_state_once() {
    let mut compiled = compiled_fixture();
    let mut expressions = compiled.molang_expressions.into_vec();
    let mut ops = compiled.molang_ops.into_vec();
    let condition = expressions.len() as u32;
    let condition_ops = vec![
        MolangOp::LoadQuery(2),
        MolangOp::JumpIfFalse(9),
        MolangOp::Push(EntityGeometryScalar::new(MAX_MOLANG_LOOP_ITERATIONS as f32).unwrap()),
        MolangOp::LoopStart(9),
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
        MolangOp::Pop,
        MolangOp::Push(EntityGeometryScalar::new(0.0).unwrap()),
        MolangOp::Pop,
        MolangOp::LoopNext(4),
        MolangOp::Push(EntityGeometryScalar::new(1.0).unwrap()),
    ];
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: condition_ops.len().try_into().unwrap(),
        max_stack: 1,
    });
    ops.extend(condition_ops);
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    compiled.render.layers[0].condition = Some(condition);
    let mut runtime = AttachablesRuntime::new(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    let owner = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    runtime
        .evaluate(
            "minecraft:test_item",
            &owner,
            &owner_rig(),
            AttachableAnimationInput::default(),
        )
        .unwrap();
    assert!(
        !runtime
            .states
            .values()
            .next()
            .unwrap()
            .rig
            .render
            .is_empty()
    );
    let input = AttachableAnimationInput {
        animation_frame: 1,
        ..Default::default()
    };
    runtime.begin_preview();
    assert!(
        runtime
            .evaluate("minecraft:test_item", &owner, &owner_rig(), input)
            .is_none()
    );
    runtime.end_preview();
    runtime.finish_preview(false);
    assert_eq!(runtime.commits, 1, "rejected readiness remains uncommitted");
    assert!(
        runtime
            .evaluate("minecraft:test_item", &owner, &owner_rig(), input)
            .is_none(),
        "render-budget exhaustion must not return an earlier layer as the current draw"
    );
    assert_eq!(
        runtime.commits, 2,
        "the normal authored evaluation still commits once"
    );
}
