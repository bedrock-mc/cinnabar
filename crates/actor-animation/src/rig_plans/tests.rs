use std::sync::atomic::Ordering;

use assets::*;

use super::*;

/// Builds two candidates for the same mesh whose animation bindings differ.
fn candidates() -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::counting_random_compiled("minecraft:test");
    let mut ops = compiled.molang_ops.into_vec();
    let mut expressions = compiled.molang_expressions.into_vec();
    let condition = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op: ops.len() as u32,
        op_count: 2,
        max_stack: 1,
    });
    ops.extend([MolangOp::LoadVariable(7), MolangOp::Truthy]);
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    compiled.rig_geometries = vec![
        compiled.rig_geometries[0],
        EntityRigGeometryBinding {
            condition: Some(condition),
            first_animation: compiled.rig_animations.len() as u32,
            animation_count: 0,
            ..compiled.rig_geometries[0]
        },
    ]
    .into();
    compiled.rig_bindings[0].geometry_count = 2;
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// Adds one real authored controller so mutable clocks can differ under identical topology.
fn controller_catalog() -> Arc<RuntimeEntityAssets> {
    let mut compiled = crate::test_support::compiled_fixture();
    let mut sources = compiled.sources.into_vec();
    sources.insert(
        0,
        EntityAssetSource {
            path: "animation_controllers/test.json".into(),
            source_bytes: 1,
            source_sha256: [1; 32],
        },
    );
    compiled.sources = sources.into();
    let mut symbols = compiled.symbols.into_vec();
    for symbol in &mut symbols {
        symbol.source_index += 1;
    }
    symbols.insert(
        2,
        EntityAssetSymbol {
            mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
            kind: EntityAssetKind::AnimationController,
            identifier: "controller.animation.test".into(),
            source_index: 0,
            dependencies: Box::new([]),
        },
    );
    compiled.symbols = symbols.into();
    compiled.geometries[0].source_index += 1;
    compiled.animation_clips[0].source += 1;
    compiled.rig_bindings[0].entity_symbol += 1;
    *compiled.rig_bindings[0].render_controller.as_mut().unwrap() += 1;
    for candidate in &mut compiled.render.candidates {
        candidate.source += 1;
    }
    compiled.controllers = vec![EntityAnimationController {
        symbol: 2,
        first_state: 0,
        state_count: 1,
        initial_state: 0,
    }]
    .into();
    compiled.controller_states = vec![EntityControllerState {
        name: 0,
        first_animation: 0,
        animation_count: 1,
        ..Default::default()
    }]
    .into();
    compiled.controller_animations = vec![EntityControllerAnimation {
        target: EntityControllerAnimationTarget::Clip(0),
        weight: None,
    }]
    .into();
    compiled.rig_controllers = vec![EntityRigControllerBinding {
        name: 0,
        controller: 0,
        weight: None,
        order: 0,
    }]
    .into();
    compiled.rig_geometries[0].controller_count = 1;
    Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap())
}

#[test]
fn repeated_inserts_build_once_and_keep_actor_variables_roots_and_poses_private() {
    for count in [1, 50, 100] {
        let assets = crate::test_support::counting_random_assets();
        let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
        for id in 1..=count {
            store.insert(
                1,
                0,
                &TestActor {
                    runtime_id: id,
                    ..Default::default()
                },
            );
        }
        assert_eq!(store.rigs.len(), count as usize);
        assert_eq!(store.layout.plans.rig_builds.load(Ordering::Relaxed), 1);
        let template = store.layout.rig_plan(&assets, 0, 0).unwrap();
        let lifetime = store.runtime_to_lifetime[&1];
        let slot = store.layout.named_slot(&assets, "variable.charge_amount");
        let first = store.rigs.get_mut(&lifetime).unwrap();
        first.variables.set(slot, 99.0);
        first.current[0].translation_scale[0] = 99.0;
        crate::attachable::bind_roots(&mut first.bones, &first.bone_names, &["rightitem".into()]);
        store.insert(
            1,
            0,
            &TestActor {
                runtime_id: count + 1,
                ..Default::default()
            },
        );
        let fresh = store.rigs.values().last().unwrap();
        assert_eq!(fresh.current.as_slice(), template.rest.as_ref());
        assert_ne!(fresh.variables.get(slot), Some(99.0));
        assert!(matches!(
            fresh.bones[0].attachable_root,
            AttachableRootFrame::Actor
        ));
        assert!(matches!(
            template.bones[0].attachable_root,
            AttachableRootFrame::Actor
        ));
        assert_eq!(store.layout.plans.rig_builds.load(Ordering::Relaxed), 1);
        let mut uncached = VariableLayout::new(&assets);
        uncached.plans = RigPlans::default();
        let reference = resolve_binding(&assets, &uncached, &TestActor::default(), 0, 0).unwrap();
        assert_eq!(reference.current, fresh.current);
        assert_eq!(reference.bone_names, fresh.bone_names);
        assert_eq!(
            reference.presentation_expressions,
            fresh.presentation_expressions
        );
    }
}

#[test]
fn selected_candidate_and_preview_restore_do_not_mutate_shared_templates() {
    let assets = candidates();
    let layout = VariableLayout::new(&assets);
    let actor = TestActor::default();
    let mut state = resolve_binding(&assets, &layout, &actor, 0, 0).unwrap();
    let original = layout.rig_plan(&assets, 0, 0).unwrap();
    let selected = layout.rig_plan(&assets, 0, 1).unwrap();
    assert!(
        !Arc::ptr_eq(&original, &selected),
        "candidate topology matters even for the same mesh"
    );
    state
        .variables
        .set(layout.named_slot(&assets, "variable.charge_amount"), 1.0);
    state.history.push_back(ActorTickInput::default());
    let mut world_left = MAX_MOLANG_OPS_PER_ACTOR_TICK;
    let mut budget = EvalBudget {
        actor_left: MAX_MOLANG_OPS_PER_ACTOR_TICK,
        world_left: &mut world_left,
        work_left: MAX_RUNTIME_POSE_WORK_PER_ACTOR_TICK,
        transitions_left: MAX_CONTROLLER_TRANSITIONS_PER_TICK,
        used: 0,
        stack: OperandStack::lease(),
        static_draw: None,
    };
    let checkpoint = geometry::reselect_geometry_preview(
        &assets,
        &layout,
        &mut state,
        &actor,
        &ActorTickContext::default(),
        &mut budget,
    )
    .expect("changed candidate has a checkpoint");
    assert_eq!(state.geometry_binding, 1);
    assert_eq!(
        budget.used, 2,
        "condition evaluation still runs on a cache hit"
    );
    crate::attachable::bind_roots(&mut state.bones, &state.bone_names, &["rightitem".into()]);
    assert!(matches!(
        state.bones[0].attachable_root,
        AttachableRootFrame::MatchingOwnerName
    ));
    checkpoint.restore(&mut state);
    assert_eq!(state.geometry_binding, 0);
    assert_eq!(state.current.as_slice(), original.rest.as_ref());
    assert!(matches!(
        state.bones[0].attachable_root,
        AttachableRootFrame::Actor
    ));
    assert!(matches!(
        selected.bones[0].attachable_root,
        AttachableRootFrame::Actor
    ));
    assert_eq!(layout.plans.rig_builds.load(Ordering::Relaxed), 2);
}

#[test]
fn motion_keys_include_all_controller_and_remembered_clip_ids_but_not_runtime_state() {
    let assets = controller_catalog();
    let layout = VariableLayout::new(&assets);
    let rig = layout.rig_plan(&assets, 0, 0).unwrap();
    let first = layout.motion_plan(&assets, 0, 0, &rig.controllers, &[]);
    let mut runtime = rig.controllers.to_vec();
    runtime[0].active = true;
    runtime[0].entered_tick = 99;
    runtime[0].blend_from = Some((0, 10, 0.5));
    let again = layout.motion_plan(&assets, 0, 0, &runtime, &[]);
    assert!(Arc::ptr_eq(&first, &again));
    let extra = layout.motion_plan(&assets, 0, 0, &runtime, &[0]);
    assert!(!Arc::ptr_eq(&first, &extra));
    assert_eq!(
        extra,
        MotionSampling::new(&assets, &layout, 0, 0, &runtime, &[0])
    );
    let duplicate = layout.motion_plan(&assets, 0, 0, &runtime, &[0, 0]);
    assert!(!Arc::ptr_eq(&extra, &duplicate));
    let no_controller = layout.motion_plan(&assets, 0, 0, &[], &[0]);
    assert!(!Arc::ptr_eq(&extra, &no_controller));
    assert_eq!(layout.plans.motion_builds.load(Ordering::Relaxed), 4);
    assert_eq!(runtime[0].entered_tick, 99);
    assert!(!rig.controllers[0].active);
    assert_eq!(rig.controllers[0].entered_tick, 0);
}

#[test]
fn exact_catalog_layout_and_replacement_guards_reject_stale_reuse() {
    let mut assets = crate::test_support::counting_random_assets();
    let layout = VariableLayout::new(&assets);
    let original = layout.rig_plan(&assets, 0, 0).unwrap();
    let foreign = Arc::new(assets.as_ref().clone());
    let first = layout.rig_plan(&foreign, 0, 0).unwrap();
    let second = layout.rig_plan(&foreign, 0, 0).unwrap();
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(first.rest, original.rest);
    let other_layout = VariableLayout::new(&assets);
    assert!(!Arc::ptr_eq(
        &original,
        &other_layout.rig_plan(&assets, 0, 0).unwrap()
    ));
    let _ = Arc::make_mut(&mut assets);
    assert!(
        !layout.plans.matches(&assets),
        "Weak ownership detects detached catalog identity"
    );
    let detached = layout.rig_plan(&assets, 0, 0).unwrap();
    assert!(!Arc::ptr_eq(&original, &detached));
    let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
    store.insert(1, 0, &TestActor::default());
    let mut replacement = crate::test_support::counting_random_compiled("minecraft:test");
    replacement.geometries[0].bones[0].pivot = Some([crate::test_support::scalar(12.0); 3]);
    let pack = Arc::new(
        RuntimeEntityAssets::from_compiled(replacement.with_unresolved_admissions()).unwrap(),
    );
    store.set_pack(Some((Arc::clone(&pack), vec![0])));
    store.insert(1, 0, &TestActor::default());
    assert!(store.rigs.values().next().unwrap().pack);
    assert_eq!(
        store.rigs.values().next().unwrap().bones[0].pivot,
        geometry::resolve_bones(&pack, 0).unwrap().0[0].pivot
    );
    store.clear();
    assert!(store.layout.plans.cache.lock().unwrap().entries.is_empty());
    assert!(
        store
            .pack
            .as_ref()
            .unwrap()
            .layout
            .plans
            .cache
            .lock()
            .unwrap()
            .entries
            .is_empty()
    );
}

#[test]
fn aggregate_slot_and_payload_limits_cover_keys_and_use_lossless_fallback() {
    let assets = crate::test_support::counting_random_assets();
    let layout = VariableLayout::new(&assets);
    let _ = layout.rig_plan(&assets, 0, 0).unwrap();
    let first = layout.motion_plan(&assets, 0, 0, &[], &[]);
    assert_eq!(layout.plans.cache.lock().unwrap().entries.len(), 2);
    for count in 1..MAX_ENTRIES + 2 {
        let _ = layout.motion_plan(&assets, 0, 0, &[], &vec![0; count]);
        let cache = layout.plans.cache.lock().unwrap();
        assert!(cache.entries.len() <= MAX_ENTRIES);
        assert!(cache.bytes <= MAX_BYTES);
    }
    assert_eq!(
        Arc::strong_count(&first),
        1,
        "eviction releases cache ownership"
    );
    layout.plans.clear();
    let value_bytes = arc_bytes::<MotionSampling>(first.retained_bytes().unwrap()).unwrap();
    layout.plans.cache.lock().unwrap().limit = FIXED_BYTES + value_bytes;
    let cached = layout.motion_plan(&assets, 0, 0, &[], &[]);
    let fallback = layout.motion_plan(&assets, 0, 0, &[], &[0]);
    let fallback_again = layout.motion_plan(&assets, 0, 0, &[], &[0]);
    assert_eq!(cached, fallback, "only the retained key differs");
    assert!(
        !Arc::ptr_eq(&fallback, &fallback_again),
        "clip keys count toward the cap"
    );
    assert_eq!(
        fallback,
        MotionSampling::new(&assets, &layout, 0, 0, &[], &[0])
    );
    assert!(Arc::ptr_eq(
        &cached,
        &layout.motion_plan(&assets, 0, 0, &[], &[])
    ));
    let mut cache = layout.plans.cache.lock().unwrap();
    let retained = cache.bytes;
    cache.insert(arc_bytes::<RigPlan>(usize::MAX), || {
        panic!("overflow never allocates an entry")
    });
    assert_eq!(cache.bytes, retained);
    drop(cache);
    layout.plans.clear();
    layout.plans.cache.lock().unwrap().limit = FIXED_BYTES;
    let one = layout.rig_plan(&assets, 0, 0).unwrap();
    let two = layout.rig_plan(&assets, 0, 0).unwrap();
    assert!(!Arc::ptr_eq(&one, &two));
    assert_eq!(one.rest, two.rest);
    assert!(layout.plans.cache.lock().unwrap().entries.is_empty());
}

#[test]
fn simultaneous_same_key_requests_publish_one_shared_constructor_plan() {
    let assets = crate::test_support::counting_random_assets();
    let layout = VariableLayout::new(&assets);
    let plans = std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..4)
            .map(|_| scope.spawn(|| layout.rig_plan(&assets, 0, 0).unwrap()))
            .collect();
        jobs.into_iter()
            .map(|job| job.join().unwrap())
            .collect::<Vec<_>>()
    });
    assert!(plans.iter().all(|plan| Arc::ptr_eq(plan, &plans[0])));
    assert_eq!(layout.plans.rig_builds.load(Ordering::Relaxed), 1);
}

mod bindings;
mod sparse;
