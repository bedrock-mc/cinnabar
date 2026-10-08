use super::*;
use assets::{CompiledMolangExpression, EntityGeometryScalar, MolangSymbol, MolangSymbolKind};

/// A valid local player has separate clock-driven and swing-driven translation channels.
fn local_fixture(authored_clock: bool) -> (HashMap<u64, ActorSnapshot>, ActorAnimationStore) {
    local_fixture_scripts(authored_clock, false)
}

/// Adds retained script and authored-clock counters to the normal local player fixture.
fn local_fixture_scripts(
    authored_clock: bool,
    stateful: bool,
) -> (HashMap<u64, ActorSnapshot>, ActorAnimationStore) {
    local_fixture_with(authored_clock, stateful, |_| {})
}

/// Applies an authored controller or geometry policy before creating the local rig.
fn local_fixture_with(
    authored_clock: bool,
    stateful: bool,
    configure: impl FnOnce(&mut assets::CompiledEntityAssets),
) -> (HashMap<u64, ActorSnapshot>, ActorAnimationStore) {
    let mut compiled = crate::actor_animation::attachable::tests::compiled_fixture();
    compiled.sources[1].path = "entity/player.json".into();
    compiled.symbols[4].kind = assets::EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:player".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    let mut layer_geometry = compiled.geometries[0].clone();
    layer_geometry.identifier = "geometry.layer".into();
    let mut symbols = compiled.symbols.into_vec();
    symbols.insert(
        2,
        assets::EntityAssetSymbol {
            kind: assets::EntityAssetKind::Geometry,
            identifier: layer_geometry.identifier.clone(),
            source_index: layer_geometry.source_index,
            dependencies: Box::new([]),
        },
    );
    compiled.symbols = symbols.into();
    let mut geometries = compiled.geometries.into_vec();
    geometries.push(layer_geometry);
    compiled.geometries = geometries.into();
    compiled.rig_bindings[0].render_controller = 4;
    compiled.rig_bindings[0].pre_animation = None;
    compiled.animation_clips[0].symbol = 3;
    compiled.animation_clips[0].anim_time_update = authored_clock.then_some(2);
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, "query.anim_time"),
        (MolangSymbolKind::Variable, "variable.attack_time"),
    ]
    .into_iter()
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .collect::<Vec<_>>()
    .into();
    let scalar = |value| EntityGeometryScalar::new(value).unwrap();
    compiled.molang_ops = vec![
        MolangOp::LoadQuery(1),
        MolangOp::Push(scalar(80.0)),
        MolangOp::Multiply,
        MolangOp::LoadVariable(2),
        MolangOp::Push(scalar(80.0)),
        MolangOp::Multiply,
        MolangOp::LoadQuery(1),
        MolangOp::Push(scalar(0.05)),
        MolangOp::Add,
    ]
    .into();
    compiled.molang_expressions = [0, 3, 6]
        .into_iter()
        .map(|first_op| CompiledMolangExpression {
            first_op,
            op_count: 3,
            max_stack: 2,
        })
        .collect::<Vec<_>>()
        .into();
    compiled.animation_keyframes[0].expressions = [Some(0), Some(1), None];
    let mut layer_clip = compiled.animation_clips[0];
    layer_clip.geometry = Some(1);
    layer_clip.first_channel = 1;
    compiled.animation_clips = vec![compiled.animation_clips[0], layer_clip].into();
    let mut layer_channel = compiled.animation_channels[0].clone();
    layer_channel.first_keyframe = 1;
    compiled.animation_channels =
        vec![compiled.animation_channels[0].clone(), layer_channel].into();
    compiled.animation_keyframes = compiled.animation_keyframes.repeat(2).into();
    compiled.render.layers[0].geometry_count = 1;
    compiled.render.geometries = vec![assets::EntityRenderGeometry {
        condition: None,
        geometry: 1,
    }]
    .into();
    if stateful {
        let mut symbols = compiled.molang_symbols.into_vec();
        for identifier in ["variable.refresh_clock", "variable.refresh_count"] {
            symbols.push(MolangSymbol {
                kind: MolangSymbolKind::Variable,
                identifier: identifier.into(),
            });
        }
        compiled.molang_symbols = symbols.into();
        let mut expressions = compiled.molang_expressions.into_vec();
        let mut ops = compiled.molang_ops.into_vec();
        let script = expressions.len() as u32;
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 5,
            max_stack: 2,
        });
        ops.extend([
            MolangOp::LoadVariable(4),
            MolangOp::Push(scalar(1.0)),
            MolangOp::Add,
            MolangOp::StoreVariable(4),
            MolangOp::Push(scalar(0.0)),
        ]);
        let clock = expressions.len() as u32;
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 7,
            max_stack: 2,
        });
        ops.extend([
            MolangOp::LoadVariable(3),
            MolangOp::Push(scalar(1.0)),
            MolangOp::Add,
            MolangOp::StoreVariable(3),
            MolangOp::LoadQuery(1),
            MolangOp::Push(scalar(0.05)),
            MolangOp::Add,
        ]);
        let counter = expressions.len() as u32;
        expressions.push(CompiledMolangExpression {
            first_op: ops.len() as u32,
            op_count: 1,
            max_stack: 1,
        });
        ops.push(MolangOp::LoadVariable(4));
        compiled.molang_expressions = expressions.into();
        compiled.molang_ops = ops.into();
        compiled.rig_bindings[0].pre_animation = Some(script);
        for clip in &mut compiled.animation_clips {
            clip.anim_time_update = Some(clock);
        }
        for keyframe in &mut compiled.animation_keyframes {
            keyframe.expressions[2] = Some(counter);
        }
    }
    configure(&mut compiled);
    let assets = Arc::new(RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut actor = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    actor.kind = ActorKind::Player {
        uuid: [1; 16],
        username: "Player".into(),
    };
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    let actors = HashMap::from([(actor.runtime_id, actor)]);
    for _ in 0..3 {
        store.advance_tick(&actors, None, Some(LOCAL), true, true, |_| {
            ActorTickContext {
                is_local: true,
                ..Default::default()
            }
        });
    }
    assert!(store.get(LOCAL).is_some_and(|rig| rig.current.len() == 1));
    (actors, store)
}

/// Changes only committed swing inputs before reevaluating this completed tick's local view.
fn refresh_swing(actors: &HashMap<u64, ActorSnapshot>, store: &mut ActorAnimationStore) {
    assert!(store.sync_local_swing(
        LOCAL,
        crate::LocalSwingProgress {
            bedrock: [0.25, 0.5],
            java: [0.25, 0.5],
            frame_alpha: Some(0.5),
        }
    ));
    store.refresh_local_view(actors, LOCAL, |_| ActorTickContext {
        is_local: true,
        animation_elapsed_ticks: Some(0),
        ..Default::default()
    });
}

#[test]
fn local_swing_refresh_preserves_authored_clip_clocks() {
    let (actors, mut store) = local_fixture(true);
    let state = store.rigs.values().next().unwrap();
    let clock = state.clip_clocks.values().next().unwrap().time;
    assert!(
        (clock - 0.15).abs() < 1e-6,
        "one authored increment per completed evaluation"
    );
    let tick = state.completed_tick;
    let current = state.current[0].translation_scale;
    let ui_x = state.ui_pose.as_ref().unwrap()[0].translation_scale[0];
    refresh_swing(&actors, &mut store);
    let state = store.rigs.values().next().unwrap();
    assert_eq!(state.clip_clocks.values().next().unwrap().time, clock);
    assert_eq!(state.completed_tick, tick);
    assert_eq!(state.current[0].translation_scale[0], current[0]);
    assert_ne!(
        state.current[0].translation_scale[1], current[1],
        "new swing inputs still affect the current pose"
    );
    assert_eq!(
        state.ui_pose.as_ref().unwrap()[0].translation_scale[0],
        ui_x
    );
    store.refresh_local_view(&actors, LOCAL, |_| ActorTickContext {
        is_local: true,
        animation_elapsed_ticks: Some(0),
        ..Default::default()
    });
    assert_eq!(
        store
            .rigs
            .values()
            .next()
            .unwrap()
            .clip_clocks
            .values()
            .next()
            .unwrap()
            .time,
        clock
    );
}

#[test]
fn local_swing_refresh_preserves_previous_pose_endpoints() {
    let (actors, mut store) = local_fixture(false);
    let state = store.rigs.values().next().unwrap();
    let tick = state.completed_tick;
    let previous = state.previous.clone();
    let current = state.current.clone();
    let layer_previous = Arc::clone(&state.render[0].previous_pose);
    let layer_current = Arc::clone(&state.render[0].pose);
    assert_ne!(
        previous[0].translation_scale[0],
        current[0].translation_scale[0]
    );
    assert_ne!(
        layer_previous[0].translation_scale[0],
        layer_current[0].translation_scale[0]
    );
    refresh_swing(&actors, &mut store);
    let state = store.rigs.values().next().unwrap();
    assert_eq!(state.completed_tick, tick);
    assert_eq!(state.previous, previous);
    assert_eq!(
        state.current[0].translation_scale[0],
        current[0].translation_scale[0]
    );
    assert_ne!(
        state.current[0].translation_scale[1],
        current[0].translation_scale[1]
    );
    assert_eq!(state.render[0].previous_pose, layer_previous);
    assert_eq!(
        state.render[0].pose[0].translation_scale[0],
        layer_current[0].translation_scale[0]
    );
}

#[test]
fn a_starved_local_refresh_keeps_both_interpolation_endpoints() {
    let (actors, mut store) = local_fixture(false);
    let rig = store.get(LOCAL).unwrap();
    let previous = rig.previous.to_vec();
    let current = rig.current.to_vec();
    let tick = rig.completed_tick;
    store.schedule.world_budget = 0;
    refresh_swing(&actors, &mut store);
    let rig = store.get(LOCAL).unwrap();
    assert_eq!(rig.completed_tick, tick);
    assert_eq!(rig.previous, previous);
    assert_eq!(rig.current, current);
}

#[test]
fn local_swing_refresh_replaces_retained_script_and_clock_assignments_once() {
    let (actors, mut store) = local_fixture_scripts(true, true);
    let count = store
        .layout
        .named_slot(store.assets.as_ref().unwrap(), "variable.refresh_count");
    let clock = store
        .layout
        .named_slot(store.assets.as_ref().unwrap(), "variable.refresh_clock");
    refresh_swing(&actors, &mut store);
    for _ in 0..2 {
        refresh_again(&actors, &mut store);
        let state = store.rigs.values().next().unwrap();
        assert_eq!(state.variables.get(count), Some(3.0));
        assert_eq!(state.variables.get(clock), Some(3.0));
        assert_eq!(state.current[0].translation_scale[2], 3.0);
        assert_eq!(state.ui_pose.as_ref().unwrap()[0].translation_scale[2], 3.0);
    }
}

/// Replaces an already refreshed tick without changing its swing observation again.
fn refresh_again(actors: &HashMap<u64, ActorSnapshot>, store: &mut ActorAnimationStore) {
    store.refresh_local_view(actors, LOCAL, |_| ActorTickContext {
        is_local: true,
        animation_elapsed_ticks: Some(0),
        ..Default::default()
    });
}

mod retained;
