use super::*;
use assets::{CompiledMolangExpression, MolangSymbol, MolangSymbolKind};

/// Builds a player carrier exposing a motion query alongside a frame-sampled spear variable.
fn compiled(query: &str) -> assets::CompiledEntityAssets {
    let mut compiled = super::super::attachable::tests::compiled_fixture();
    compiled.sources[1].path = "entity/player.json".into();
    compiled.symbols[4].kind = assets::EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:player".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = 3;
    compiled.rig_bindings[0].pre_animation = None;
    compiled.animation_clips[0].symbol = 2;
    compiled.molang_symbols = vec![
        MolangSymbol {
            kind: MolangSymbolKind::Name,
            identifier: "wield".into(),
        },
        MolangSymbol {
            kind: if query.starts_with("variable.") {
                MolangSymbolKind::Variable
            } else {
                MolangSymbolKind::Query
            },
            identifier: query.into(),
        },
        MolangSymbol {
            kind: MolangSymbolKind::Variable,
            identifier: "variable.tp_melee_spear_use_arm_rotation_x".into(),
        },
    ]
    .into_boxed_slice();
    compiled.molang_ops = vec![
        if query.starts_with("variable.") {
            MolangOp::LoadVariable(1)
        } else {
            MolangOp::LoadQuery(1)
        },
        MolangOp::LoadVariable(2),
        MolangOp::Add,
    ]
    .into_boxed_slice();
    compiled.molang_expressions = vec![CompiledMolangExpression {
        first_op: 0,
        op_count: 3,
        max_stack: 2,
    }]
    .into_boxed_slice();
    compiled.animation_keyframes[0].expressions = [Some(0), None, None];
    compiled
}

/// Spawns a synthetic local player without requiring installed carriers.
fn fixture(query: &str) -> (HashMap<u64, ActorSnapshot>, ActorAnimationStore) {
    let compiled = compiled(query);
    let mut actor = super::super::tests::actor_with_metadata(HashMap::new());
    actor.kind = ActorKind::Player {
        uuid: [1; 16],
        username: "Player".into(),
    };
    actor.position = [0.0; 3];
    let mut store = ActorAnimationStore::with_assets(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    store.insert(1, 0, &actor);
    (HashMap::from([(actor.runtime_id, actor)]), store)
}

/// Advances only completed actor ticks, leaving the render samples read-only.
fn tick(actors: &HashMap<u64, ActorSnapshot>, store: &mut ActorAnimationStore) {
    store.advance_tick(actors, None, Some(1), true, true, |_| ActorTickContext {
        is_local: true,
        is_local_player: true,
        ..Default::default()
    });
}

/// Resolves the frame's root translation just as the renderer blends its published endpoints.
fn sample(actors: &HashMap<u64, ActorSnapshot>, store: &ActorAnimationStore, alpha: f32) -> f32 {
    let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
    let layers = store
        .render_layers(&actors[&1], alpha, [0.0; 2], [0.0; 3], &mut budget, true)
        .unwrap();
    let layer = &layers.render[0];
    let rig = store.get(1).unwrap();
    let (previous, current) = if layer.pose.is_empty() {
        (rig.previous, rig.current)
    } else {
        (layer.previous_pose.as_ref(), layer.pose.as_ref())
    };
    let previous = previous[0].translation_scale[0];
    let current = current[0].translation_scale[0];
    -(previous + (current - previous) * alpha)
}

#[test]
fn local_walk_pose_changes_between_ticks_at_the_interpolated_distance() {
    let (mut actors, mut store) = fixture("query.modified_distance_moved");
    tick(&actors, &mut store);
    actors.get_mut(&1).unwrap().position[0] = 0.2;
    tick(&actors, &mut store);
    let committed = store.get(1).unwrap();
    let before = (
        committed.completed_tick,
        committed.previous.to_vec(),
        committed.current.to_vec(),
    );
    let stats = store.stats;
    let early = sample(&actors, &store, 0.25);
    let late = sample(&actors, &store, 0.75);
    assert!((early - 0.08).abs() < 1e-6, "early frame: {early}");
    assert!((late - 0.24).abs() < 1e-6, "late frame: {late}");
    assert!(late > early);
    let unchanged = store.get(1).unwrap();
    assert_eq!(
        (
            unchanged.completed_tick,
            unchanged.previous.to_vec(),
            unchanged.current.to_vec()
        ),
        before
    );
    assert_eq!(store.stats, stats);
}

#[test]
fn local_frame_motion_samples_speed_stride_and_wrapped_headings() {
    for query in [
        "query.modified_move_speed",
        "query.walk_distance",
        "query.body_y_rotation",
        "query.target_y_rotation",
        "query.target_x_rotation",
        "variable.player_x_rotation",
    ] {
        let (mut actors, mut store) = fixture(query);
        {
            let actor = actors.get_mut(&1).unwrap();
            actor.yaw = 170.0;
            actor.head_yaw = 170.0;
            actor.pitch = 10.0;
        }
        tick(&actors, &mut store);
        {
            let actor = actors.get_mut(&1).unwrap();
            actor.position[0] = 0.2;
            actor.yaw = -170.0;
            actor.head_yaw = -170.0;
            actor.pitch = 30.0;
        }
        tick(&actors, &mut store);
        let state = &store.rigs[store.runtime_to_lifetime.get(&1).unwrap()];
        let previous = state.history[0];
        let current = state.history[1];
        for alpha in [0.0, 0.25, 0.75, 1.0] {
            let angle = |from: f32, to: f32| from + query::wrap_degrees(to - from) * alpha;
            let expected = match query {
                "query.modified_move_speed" => {
                    previous.move_speed + (current.move_speed - previous.move_speed) * alpha
                }
                "query.walk_distance" => {
                    current.walk_distance + (current.walk_distance - previous.walk_distance) * alpha
                }
                "query.body_y_rotation" => angle(previous.body_yaw, current.body_yaw),
                "query.target_y_rotation" => query::wrap_degrees(
                    angle(previous.head_yaw, current.head_yaw)
                        - angle(previous.body_yaw, current.body_yaw),
                ),
                "query.target_x_rotation" | "variable.player_x_rotation" => {
                    angle(previous.pitch, current.pitch)
                }
                _ => unreachable!(),
            };
            let actual = sample(&actors, &store, alpha);
            assert!(
                (actual - expected).abs() < 1e-4,
                "{query} at {alpha}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn local_motion_driven_clip_clock_samples_frames_without_advancing_ticks() {
    let (mut actors, _) = fixture("query.modified_distance_moved");
    let mut compiled = compiled("query.modified_distance_moved");
    compiled.animation_clips[0].anim_time_update = Some(0);
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.insert(
        1,
        MolangSymbol {
            kind: MolangSymbolKind::Query,
            identifier: "query.anim_time".into(),
        },
    );
    compiled.molang_symbols = symbols.into_boxed_slice();
    let mut ops = compiled.molang_ops.into_vec();
    for op in &mut ops {
        match op {
            MolangOp::LoadQuery(symbol) | MolangOp::LoadVariable(symbol) => *symbol += 1,
            _ => {}
        }
    }
    ops.extend([
        MolangOp::LoadQuery(1),
        MolangOp::LoadVariable(3),
        MolangOp::Add,
    ]);
    compiled.molang_ops = ops.into_boxed_slice();
    let mut expressions = compiled.molang_expressions.into_vec();
    expressions.push(CompiledMolangExpression {
        first_op: 3,
        op_count: 3,
        max_stack: 2,
    });
    compiled.molang_expressions = expressions.into_boxed_slice();
    compiled.animation_keyframes[0].expressions = [Some(1), None, None];
    let actor = &actors[&1];
    let mut store = ActorAnimationStore::with_assets(Arc::new(
        RuntimeEntityAssets::from_compiled(compiled).unwrap(),
    ));
    store.insert(1, 0, actor);
    tick(&actors, &mut store);
    actors.get_mut(&1).unwrap().position[0] = 0.2;
    tick(&actors, &mut store);
    for (alpha, expected) in [(0.25, 0.08), (0.75, 0.24)] {
        let actual = sample(&actors, &store, alpha);
        assert!(
            (actual - expected).abs() < 1e-6,
            "clip at {alpha}: {actual}"
        );
    }
}

#[test]
fn frame_motion_keeps_raw_position_and_tick_actions_for_local_and_remote_players() {
    for local in [false, true] {
        let (mut actors, mut store) = fixture("query.modified_distance_moved");
        for position in [0.0, 0.2] {
            actors.get_mut(&1).unwrap().position[0] = position;
            store.advance_tick(&actors, None, local.then_some(1), true, true, |_| {
                ActorTickContext {
                    is_local: local,
                    is_local_player: local,
                    ..Default::default()
                }
            });
        }
        let state = &store.rigs[store.runtime_to_lifetime.get(&1).unwrap()];
        let current = state.render_frame.as_ref().unwrap().motion.input;
        for (alpha, expected) in [(0.25, 0.08), (0.75, 0.24)] {
            let input = motion::input(state, &state.render_frame.as_ref().unwrap().motion, alpha);
            assert_eq!(input.position, current.position);
            assert_eq!(input.position_delta, current.position_delta);
            assert_eq!(input.attack_time, current.attack_time);
            assert_eq!(input.item_use_ticks, current.item_use_ticks);
            assert!((sample(&actors, &store, alpha) - expected).abs() < 1e-6);
        }
    }
}

#[test]
fn first_person_pitch_driver_samples_observed_pitch() {
    for pitches in [[30.0, 30.0], [10.0, 30.0]] {
        let (mut actors, mut store) = fixture("variable.player_x_rotation");
        for pitch in pitches {
            actors.get_mut(&1).unwrap().pitch = pitch;
            store.advance_tick(&actors, None, Some(1), true, true, |_| ActorTickContext {
                is_local: true,
                is_local_player: true,
                is_local_first_person: true,
                ..Default::default()
            });
        }
        for alpha in [0.0, 0.25, 0.75, 1.0] {
            let expected = pitches[0] + (pitches[1] - pitches[0]) * alpha;
            let actual = sample(&actors, &store, alpha);
            assert!(
                (actual - expected).abs() < 1e-6,
                "pitch driver at {alpha}: {actual} != {expected}"
            );
        }
    }
}

#[test]
fn first_person_rotation_queries_stay_zero_between_ticks() {
    for query in [
        "query.target_x_rotation",
        "query.target_y_rotation",
        "query.body_y_rotation",
    ] {
        let (mut actors, mut store) = fixture(query);
        let actor = actors.get_mut(&1).unwrap();
        actor.pitch = 30.0;
        actor.yaw = 60.0;
        actor.head_yaw = 90.0;
        for _ in 0..2 {
            store.advance_tick(&actors, None, Some(1), true, true, |_| ActorTickContext {
                is_local: true,
                is_local_player: true,
                is_local_first_person: true,
                ..Default::default()
            });
        }
        for alpha in [0.0, 0.25, 0.75, 1.0] {
            let actual = sample(&actors, &store, alpha);
            assert!(actual.abs() < 1e-6, "{query} at {alpha}: {actual}");
        }
    }
}

#[test]
fn starved_frame_holds_completed_pose_until_motion_can_be_evaluated() {
    let (mut actors, mut store) = fixture("query.modified_distance_moved");
    tick(&actors, &mut store);
    actors.get_mut(&1).unwrap().position[0] = 0.2;
    tick(&actors, &mut store);
    store.schedule.world_budget = 0;
    actors.get_mut(&1).unwrap().position[0] = 0.4;
    tick(&actors, &mut store);
    assert_eq!(store.stats.world_budget_exhaustions, 1);
    for alpha in [0.25, 0.75, 0.0, 1.0] {
        let actual = sample(&actors, &store, alpha);
        assert!(
            (actual - 0.32).abs() < 1e-6,
            "held frame at {alpha}: {actual}"
        );
    }
    store.schedule.world_budget = MAX_MOLANG_OPS_PER_WORLD_TICK;
    actors.get_mut(&1).unwrap().position[0] = 0.6;
    tick(&actors, &mut store);
    let actual = sample(&actors, &store, 0.25);
    assert!((actual - 0.9888).abs() < 1e-6, "resumed motion: {actual}");
}
