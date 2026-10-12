use super::*;
use assets::{EntityAssetKind, MolangSymbol, MolangSymbolKind};

/// The initialized pose and part-visibility fixture.
fn compiled_assets() -> assets::CompiledEntityAssets {
    crate::test_support::initialized_pose_compiled()
}

/// Loads the original initialized pose and visibility fixture.
fn assets() -> Arc<RuntimeEntityAssets> {
    Arc::new(
        RuntimeEntityAssets::from_compiled(compiled_assets().with_unresolved_admissions()).unwrap(),
    )
}

/// Selects a second geometry with its own matching animation channel.
fn alternate_geometry_assets() -> Arc<RuntimeEntityAssets> {
    let mut compiled = compiled_assets();
    let mut geometry = compiled.geometries[0].clone();
    geometry.identifier = "geometry.layer".into();
    let mut symbols = compiled.symbols.into_vec();
    symbols.insert(
        2,
        assets::EntityAssetSymbol {
            kind: EntityAssetKind::Geometry,
            identifier: geometry.identifier.clone(),
            source_index: geometry.source_index,
            dependencies: Box::new([]),
            mount_equipment: assets::entity::mount::MountEquipment::Unresolved,
        },
    );
    compiled.symbols = symbols.into();
    compiled.geometries = vec![compiled.geometries[0].clone(), geometry].into();
    compiled.rig_bindings[0].render_controller += 1;
    compiled.animation_clips[0].symbol += 1;
    let mut clip = compiled.animation_clips[0];
    clip.geometry = Some(1);
    clip.first_channel = 1;
    compiled.animation_clips = vec![compiled.animation_clips[0], clip].into();
    let mut channel = compiled.animation_channels[0].clone();
    channel.first_keyframe = 1;
    compiled.animation_channels = vec![compiled.animation_channels[0].clone(), channel].into();
    compiled.animation_keyframes = compiled.animation_keyframes.repeat(2).into();
    compiled.render.layers[0].geometry_count = 1;
    compiled.render.geometries = vec![assets::EntityRenderGeometry {
        condition: None,
        geometry: 1,
    }]
    .into();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// A fresh rig with the complete zero-time presentation context.
fn animation_fixture() -> (ActorAnimationStore, HashMap<u64, TestActor>) {
    animation_fixture_with(assets())
}

/// Admits the original counting actor with the supplied geometry policy.
fn animation_fixture_with(
    assets: Arc<RuntimeEntityAssets>,
) -> (ActorAnimationStore, HashMap<u64, TestActor>) {
    let mut store = ActorAnimationStore::with_assets(assets);
    let actor = super::super::super::tests::actor_with_metadata(HashMap::new());
    store.insert(1, 0, &actor);
    (store, HashMap::from([(actor.runtime_id, actor)]))
}

/// Supplies a real frame context with no elapsed simulation interval.
fn zero_time_context(_: &TestActor) -> ActorTickContext {
    ActorTickContext {
        animation_elapsed_ticks: Some(0),
        is_local: true,
        ..Default::default()
    }
}

#[test]
fn first_presentation_preserves_motion_and_clip_time_and_replays_local_view() {
    let (mut store, actors) = animation_fixture();
    let motion = format!("{:?}", store.rigs.values().next().unwrap().motion);
    store.prepare_frame(&actors, None, Some(1), true, zero_time_context);
    let state = store.rigs.values().next().unwrap();
    assert_eq!(format!("{:?}", state.motion), motion);
    assert_eq!(state.history.len(), 1);
    assert!(state.clip_clocks.values().all(|clock| clock.time == 0.0));
    assert_eq!(state.render[0].color[0], 6.0);
    let authored = authored_state(&store);
    store.prepare_frame(&actors, None, Some(1), true, zero_time_context);
    assert_eq!(
        authored_state(&store),
        authored,
        "local refresh replays the first evaluation"
    );
    store.advance_tick(&actors, None, Some(1), true, true, |_| ActorTickContext {
        is_local: true,
        ..Default::default()
    });
    assert_eq!(store.get(1).unwrap().render[0].color[0], 7.0);
    assert!(
        store
            .rigs
            .values()
            .next()
            .unwrap()
            .clip_clocks
            .values()
            .all(|clock| clock.time == simulation_clock::TICK_DURATION.as_secs_f32())
    );
    store.clear();
    store.insert(2, 0, actors.get(&1).unwrap());
    store.prepare_frame(&actors, None, Some(1), false, zero_time_context);
    assert_eq!(store.get(1).unwrap().render[0].color[0], 6.0);
    assert_eq!(store.get(1).unwrap().completed_tick, 0);
}

#[test]
fn initial_presentation_respects_the_world_budget_and_retries_without_partial_effects() {
    let (mut store, actors) = animation_fixture();
    let authored = authored_state(&store);
    store.set_world_budget(1);
    store.prepare_frame(&actors, None, None, false, zero_time_context);
    assert_eq!(store.completed_tick, 0);
    assert_eq!(store.stats.evaluated_molang_ops, 1);
    assert_eq!(authored_state(&store), authored);
    assert!(!store.get(1).unwrap().pose_evaluated);
    assert!(store.needs_initial_presentation(&actors, None, None, |_| true));
    store.set_world_budget(MAX_MOLANG_OPS_PER_WORLD_TICK);
    store.prepare_frame(&actors, None, None, false, zero_time_context);
    assert_eq!(store.get(1).unwrap().render[0].color[0], 6.0);
    assert!(store.get(1).unwrap().pose_evaluated);
    assert!(!store.needs_initial_presentation(&actors, None, None, |_| true));
}

#[test]
fn initial_render_selection_retries_without_publishing_partial_effects() {
    let (mut reference, actors) = animation_fixture();
    reference.prepare_frame(&actors, None, None, false, zero_time_context);
    let complete = authored_state(&reference);
    let complete_cost = reference.stats.evaluated_molang_ops as usize;
    assert!(complete_cost > 1);
    for budget in 1..complete_cost {
        let (mut store, actors) = animation_fixture();
        let pending = authored_state(&store);
        store.set_world_budget(budget);
        store.prepare_frame(&actors, None, None, false, zero_time_context);
        assert!(
            !store.get(1).unwrap().pose_evaluated,
            "incomplete initial evaluation with budget {budget} must remain pending"
        );
        assert!(store.needs_initial_presentation(&actors, None, None, |_| true));
        assert_eq!(authored_state(&store), pending, "budget {budget}");
        store.set_world_budget(MAX_MOLANG_OPS_PER_WORLD_TICK);
        store.prepare_frame(&actors, None, None, false, zero_time_context);
        assert_eq!(authored_state(&store), complete, "retry budget {budget}");
        assert_eq!(store.get(1).unwrap().completed_tick, 0);
    }
}

#[test]
fn initial_render_alternate_pose_retries_after_layer_budget_exhaustion() {
    let (mut reference, actors) = animation_fixture_with(alternate_geometry_assets());
    reference.prepare_frame(&actors, None, None, false, zero_time_context);
    let complete = authored_state(&reference);
    let rig = reference.get(1).unwrap();
    assert_eq!(rig.render[0].geometry, Some(1));
    assert_eq!(rig.render[0].pose.as_ref(), rig.current);
    let cost = reference.stats.evaluated_molang_ops as usize;
    let (mut store, actors) = animation_fixture_with(alternate_geometry_assets());
    let pending = authored_state(&store);
    store.set_world_budget(cost - 1);
    store.prepare_frame(&actors, None, None, false, zero_time_context);
    assert!(!store.get(1).unwrap().pose_evaluated);
    assert_eq!(authored_state(&store), pending);
    store.set_world_budget(MAX_MOLANG_OPS_PER_WORLD_TICK);
    store.prepare_frame(&actors, None, None, false, zero_time_context);
    assert_eq!(authored_state(&store), complete);
    assert_eq!(
        store.get(1).unwrap().render,
        reference.get(1).unwrap().render
    );
}

#[test]
fn first_presentation_seeds_java_posture_without_advancing_motion() {
    for (flag, sleeping, riding, spear) in [
        (None, false, false, false),
        (Some(query::FLAG_SWIMMING), false, false, false),
        (Some(query::FLAG_GLIDING), false, false, false),
        (Some(query::FLAG_EMOTING), false, false, false),
        (Some(query::FLAG_CRAWLING), false, false, false),
        (None, true, false, false),
        (None, false, true, false),
        (None, false, false, true),
    ] {
        let mut compiled = compiled_assets();
        compiled.symbols[0].identifier = "minecraft:player".into();
        let assets = Arc::new(
            RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
        );
        let mut actor = super::super::super::tests::actor_with_metadata(HashMap::new());
        actor.kind = protocol::ActorKind::Player {
            uuid: [1; 16],
            username: "Player".into(),
        };
        if let Some(flag) = flag {
            let (key, bit) = if flag < 64 {
                (0, flag)
            } else {
                (crate::test_actor::EXTENDED_FLAGS_METADATA_KEY, flag - 64)
            };
            actor
                .metadata
                .insert(key, ActorMetadataValue::Flags(1 << bit));
        }
        actor.status.sleeping = sleeping;
        let context = ActorTickContext {
            is_riding: riding,
            main_hand_is_spear: spear,
            ..zero_time_context(&actor)
        };
        let mut store = ActorAnimationStore::with_assets(assets);
        store.insert(1, 0, &actor);
        let mut expected = store.get(1).unwrap().java;
        expected.riding = riding;
        expected.vanilla_posture = flag.is_some() || sleeping || spear;
        let actors = HashMap::from([(1, actor)]);
        store.prepare_frame(&actors, None, None, false, |_| context.clone());
        let rig = store.get(1).unwrap();
        assert!(rig.pose_evaluated);
        assert_eq!(rig.completed_tick, 0);
        assert_eq!(
            rig.java, expected,
            "posture {flag:?}, sleeping {sleeping}, riding {riding}, spear {spear}"
        );
        store.prepare_frame(&actors, None, None, false, |_| context.clone());
        assert_eq!(store.get(1).unwrap().java, expected);
    }
}

#[test]
fn first_presentation_refreshes_current_queries_after_pending_simulation_history() {
    let mut compiled = compiled_assets();
    compiled.molang_symbols[3].identifier = "query.is_riding".into();
    let expression = compiled.animation_keyframes[0].expressions[1].unwrap() as usize;
    let op = compiled.molang_expressions[expression].first_op as usize;
    compiled.molang_ops[op] = MolangOp::LoadQuery(3);
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let (mut store, mut actors) = animation_fixture_with(assets);
    store.advance_tick(&actors, None, None, true, true, |_| ActorTickContext {
        appearance_pending: true,
        ..Default::default()
    });
    let state = store.rigs.values().next().unwrap();
    let motion = format!("{:?}", state.motion);
    let observed = *state.history.back().unwrap();
    let history_len = state.history.len();
    assert!(!store.get(1).unwrap().pose_evaluated);
    let actor = actors.get_mut(&1).unwrap();
    actor.position = [4.0, 5.0, 6.0];
    actor.yaw = 75.0;
    actor.pitch = 30.0;
    actor
        .metadata
        .insert(0, ActorMetadataValue::Flags(1 << query::FLAG_USING_ITEM));
    store.prepare_frame(&actors, None, None, false, |_| ActorTickContext {
        is_riding: true,
        local_item_use_ticks: Some(3),
        animation_elapsed_ticks: Some(0),
        ..Default::default()
    });
    let rig = store.get(1).unwrap();
    assert!(rig.pose_evaluated);
    assert_eq!(rig.completed_tick, 1);
    assert_eq!(
        rig.current[0].translation_scale[1], 1.0,
        "current riding query drives first pose"
    );
    let input = rig.animation_variables.input().unwrap();
    assert_eq!(input.position, actors[&1].position);
    assert_eq!(input.yaw, actors[&1].yaw);
    assert_eq!(input.pitch, actors[&1].pitch);
    assert!(input.is_riding);
    assert_eq!(input.item_use_ticks, 4);
    assert_eq!(input.position_delta, observed.position_delta);
    assert_eq!(input.walk_distance, observed.walk_distance);
    let state = store.rigs.values().next().unwrap();
    assert_eq!(state.history.len(), history_len);
    assert_eq!(format!("{:?}", state.motion), motion);
}

#[test]
fn first_presentation_uses_the_rendered_body_heading_after_pending_motion() {
    let mut compiled = compiled_assets();
    compiled.molang_symbols[5].identifier = "query.target_y_rotation".into();
    let expression = compiled.animation_keyframes[0].expressions[1].unwrap() as usize;
    let op = compiled.molang_expressions[expression].first_op as usize;
    compiled.molang_ops[op] = MolangOp::LoadQuery(5);
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let (mut store, mut actors) = animation_fixture_with(assets);
    let actor = actors.get_mut(&1).unwrap();
    actor.yaw = 90.0;
    actor.head_yaw = 100.0;
    store.advance_tick(&actors, None, None, true, true, |_| ActorTickContext {
        appearance_pending: true,
        ..Default::default()
    });
    let state = store.rigs.values().next().unwrap();
    let body_yaw = state.motion.body_yaw;
    let motion = format!("{:?}", state.motion);
    let history_len = state.history.len();
    assert_ne!(body_yaw, actors[&1].body_yaw);
    actors.get_mut(&1).unwrap().head_yaw = body_yaw + 20.0;
    store.prepare_frame(&actors, None, None, false, zero_time_context);
    let rig = store.get(1).unwrap();
    assert!(rig.pose_evaluated);
    assert_eq!(rig.body_yaw, body_yaw);
    assert_eq!(
        rig.current[0].translation_scale[1], 20.0,
        "initial heading query must be relative to the rendered body"
    );
    let state = store.rigs.values().next().unwrap();
    assert_eq!(state.history.len(), history_len);
    assert_eq!(format!("{:?}", state.motion), motion);
}

const LOCAL_VIEW_RUNTIME_ID: u64 = 3;

#[test]
fn first_presentation_does_not_observe_rest_after_spawn_or_reset() {
    for reset in [false, true] {
        let (mut store, mut actors) = animation_fixture();
        store.advance_tick(&actors, None, None, true, true, |_| Default::default());
        assert_eq!(store.get(1).unwrap().rest_completed_tick, 1);
        let runtime_id = if reset {
            store.mark_reset(1);
            1
        } else {
            let mut actor = actors[&1].clone();
            actor.runtime_id = 2;
            store.insert(1, 0, &actor);
            actors.insert(2, actor);
            2
        };
        assert_eq!(store.get(runtime_id).unwrap().rest_completed_tick, 0);
        store.prepare_frame(&actors, None, Some(runtime_id), true, zero_time_context);
        let rig = store.get(runtime_id).unwrap();
        assert!(rig.pose_evaluated);
        assert_eq!(
            rig.rest_completed_tick, 0,
            "a presentation frame cannot replace this lifetime's fixed-tick observation"
        );
        store.advance_tick(&actors, None, None, false, true, |_| Default::default());
        assert_eq!(store.get(runtime_id).unwrap().rest_completed_tick, 2);
    }
}

/// Gives the local rig a visible pose change when its view mode changes.
fn local_view_fixture() -> (ActorAnimationStore, HashMap<u64, TestActor>) {
    let mut compiled = compiled_assets();
    let mut symbols = compiled.molang_symbols.into_vec();
    let slot = symbols.len() as u32;
    symbols.push(MolangSymbol {
        kind: MolangSymbolKind::Variable,
        identifier: "variable.is_first_person".into(),
    });
    compiled.molang_symbols = symbols.into();
    let expression = compiled.animation_keyframes[0].expressions[1].unwrap() as usize;
    let op = compiled.molang_expressions[expression].first_op as usize;
    compiled.molang_ops[op] = MolangOp::LoadVariable(slot);
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let mut store = ActorAnimationStore::with_assets(assets);
    let mut actor = super::super::super::tests::actor_with_metadata(HashMap::new());
    actor.runtime_id = LOCAL_VIEW_RUNTIME_ID;
    store.insert(1, 0, &actor);
    let actors = HashMap::from([(LOCAL_VIEW_RUNTIME_ID, actor)]);
    store.prepare_frame(
        &actors,
        None,
        Some(LOCAL_VIEW_RUNTIME_ID),
        false,
        zero_time_context,
    );
    (store, actors)
}

#[test]
fn local_view_refresh_keeps_its_budget_when_new_rigs_are_pending() {
    let first_person = |actor: &TestActor| ActorTickContext {
        is_local_first_person: actor.runtime_id == LOCAL_VIEW_RUNTIME_ID,
        ..zero_time_context(actor)
    };
    let (mut reference, actors) = local_view_fixture();
    let before = reference.stats.evaluated_molang_ops;
    reference.prepare_frame(
        &actors,
        None,
        Some(LOCAL_VIEW_RUNTIME_ID),
        true,
        first_person,
    );
    let refresh_cost = (reference.stats.evaluated_molang_ops - before) as usize;
    let expected = reference
        .get(LOCAL_VIEW_RUNTIME_ID)
        .unwrap()
        .current
        .to_vec();
    assert_eq!(expected[0].translation_scale[1], 1.0);
    let (mut store, mut actors) = local_view_fixture();
    for runtime_id in 1..LOCAL_VIEW_RUNTIME_ID {
        let mut remote = super::super::super::tests::actor_with_metadata(HashMap::new());
        remote.runtime_id = runtime_id;
        store.insert(1, 0, &remote);
        actors.insert(runtime_id, remote);
    }
    store.set_world_budget(refresh_cost);
    store.prepare_frame(
        &actors,
        None,
        Some(LOCAL_VIEW_RUNTIME_ID),
        true,
        first_person,
    );
    assert_eq!(
        store.get(LOCAL_VIEW_RUNTIME_ID).unwrap().current,
        expected,
        "new actors must not consume a requested local refresh budget"
    );
    for runtime_id in 1..LOCAL_VIEW_RUNTIME_ID {
        assert!(!store.get(runtime_id).unwrap().pose_evaluated);
    }
    store.set_world_budget(MAX_MOLANG_OPS_PER_WORLD_TICK);
    store.prepare_frame(
        &actors,
        None,
        Some(LOCAL_VIEW_RUNTIME_ID),
        false,
        first_person,
    );
    assert_eq!(store.get(LOCAL_VIEW_RUNTIME_ID).unwrap().current, expected);
    for runtime_id in 1..LOCAL_VIEW_RUNTIME_ID {
        assert!(store.get(runtime_id).unwrap().pose_evaluated);
    }
}

#[test]
fn first_presentation_retry_refreshes_snapshot_inputs_without_a_motion_tick() {
    let (mut store, mut actors) = animation_fixture();
    store.set_world_budget(1);
    store.prepare_frame(&actors, None, None, false, zero_time_context);
    let actor = actors.get_mut(&1).unwrap();
    actor.position = [4.0, 5.0, 6.0];
    actor.yaw = 75.0;
    actor.pitch = 30.0;
    store.set_world_budget(MAX_MOLANG_OPS_PER_WORLD_TICK);
    store.prepare_frame(&actors, None, None, false, zero_time_context);
    let state = store.rigs.values().next().unwrap();
    let input = state.history.back().unwrap();
    assert_eq!(input.position, actors[&1].position);
    assert_eq!(input.yaw, actors[&1].yaw);
    assert_eq!(input.pitch, actors[&1].pitch);
    assert_eq!(state.history.len(), 1);
    assert_eq!(store.completed_tick, 0);
}

#[test]
fn off_screen_initial_rigs_do_not_build_frame_contexts() {
    let (mut store, mut actors) = animation_fixture();
    let actor = actors.get_mut(&1).unwrap();
    actor.position[0] = 20.0;
    actor.previous_position = actor.position;
    let view = ActorAnimationView {
        planes: [[-1.0, 0.0, 0.0, 10.0]; 6],
        camera: [0.0; 3],
        player_distance: 100.0,
        entity_radius: 72.0,
    };
    let contexts = std::cell::Cell::new(0);
    for _ in 0..2 {
        assert!(!store.needs_initial_presentation(&actors, Some(&view), None, |_| true));
        store.prepare_frame(&actors, Some(&view), None, false, |actor| {
            contexts.set(contexts.get() + 1);
            zero_time_context(actor)
        });
    }
    assert_eq!(contexts.get(), 0);
    let actor = actors.get_mut(&1).unwrap();
    actor.position = [0.0; 3];
    actor.previous_position = actor.position;
    assert!(store.needs_initial_presentation(&actors, Some(&view), None, |_| true));
    store.prepare_frame(&actors, Some(&view), None, false, zero_time_context);
    assert_eq!(store.get(1).unwrap().render[0].color[0], 6.0);
}
