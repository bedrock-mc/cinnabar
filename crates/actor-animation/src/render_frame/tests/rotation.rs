use super::*;
use assets::{CompiledMolangExpression, EntityAnimationProperty, MolangCall};

mod held_scale;
mod review;

/// Encodes head yaw, pitch and body yaw as translation so their exact query values are visible.
fn compiled_fixture(limit: f32) -> assets::CompiledEntityAssets {
    let mut compiled = crate::test_support::initialized_pose_compiled();
    compiled.symbols[0].identifier = "minecraft:player".into();
    compiled.molang_symbols[1].identifier = "query.body_y_rotation".into();
    compiled.molang_symbols[2].identifier = "query.frame_alpha".into();
    compiled.molang_symbols[3].identifier = "query.head_x_rotation".into();
    compiled.molang_symbols[4].identifier = "query.head_y_rotation".into();
    compiled.render.visibility = Box::new([]);
    compiled.render.layers[0].visibility_count = 0;
    let mut ops = compiled.molang_ops.into_vec();
    for op in &mut ops {
        if let MolangOp::LoadQuery(1) = op {
            *op = MolangOp::LoadQuery(2);
        }
    }
    let mut expressions = compiled.molang_expressions.into_vec();
    let mut reads = Vec::new();
    for (symbol, argument) in [(4, Some(limit)), (3, Some(0.0)), (1, None)] {
        let first_op = ops.len() as u32;
        if let Some(argument) = argument {
            ops.push(MolangOp::Push(crate::test_support::scalar(argument)));
            ops.push(MolangOp::CallQuery(MolangCall {
                symbol,
                arguments: 1,
            }));
        } else {
            ops.push(MolangOp::LoadQuery(symbol));
        }
        if symbol == 4 {
            // Composition mirrors authored X offsets; expose the query's original value.
            ops.push(MolangOp::Negate);
        }
        reads.push(expressions.len() as u32);
        expressions.push(CompiledMolangExpression {
            first_op,
            op_count: (ops.len() as u32 - first_op) as u16,
            max_stack: 1,
        });
    }
    let mut keys = compiled.animation_keyframes.into_vec();
    let mut ordinary = keys[0];
    ordinary.expressions = [None, None, Some(1)];
    keys[0].expressions = [Some(reads[0]), Some(reads[1]), Some(reads[2])];
    keys.push(ordinary);
    let mut channels = compiled.animation_channels.into_vec();
    let mut ordinary = channels[0].clone();
    ordinary.property = EntityAnimationProperty::Rotation;
    ordinary.first_keyframe = 1;
    channels.push(ordinary);
    compiled.animation_clips[0].channel_count = channels.len() as u32;
    compiled.animation_channels = channels.into();
    compiled.animation_keyframes = keys.into();
    compiled.molang_ops = ops.into();
    compiled.molang_expressions = expressions.into();
    compiled
}

/// Starts the rotation fixture with the ordinary third-person context.
fn fixture(limit: f32, yaw: f32) -> (ActorAnimationStore, HashMap<u64, TestActor>) {
    fixture_with(compiled_fixture(limit), yaw, Default::default())
}

/// Seeds a player rig with the supplied authored assets and prepared appearance context.
fn fixture_with(
    compiled: assets::CompiledEntityAssets,
    yaw: f32,
    context: ActorTickContext,
) -> (ActorAnimationStore, HashMap<u64, TestActor>) {
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let actor = TestActor {
        kind: protocol::ActorKind::Player {
            uuid: [1; 16],
            username: "Player".into(),
        },
        yaw,
        head_yaw: yaw,
        body_yaw: yaw,
        ..Default::default()
    };
    let mut store = ActorAnimationStore::with_assets(assets);
    store.insert(1, 0, &actor);
    if let Some(source) = &context.skin_geometry {
        store.request_skin_preparation(source);
        store.submit_skin_preparation();
        store.finish_skin_fixture_batch();
    }
    let actors = HashMap::from([(actor.runtime_id, actor)]);
    store.advance_tick(
        &actors,
        None,
        context.is_local.then_some(1),
        true,
        true,
        |_| context.clone(),
    );
    (store, actors)
}

/// Resolves frame-sampled layers or the tick-owned pose the renderer uses as its fallback.
fn sample(store: &ActorAnimationStore, actor: &TestActor, alpha: f32) -> RenderTextureLayer {
    let mut budget = MAX_MOLANG_OPS_PER_RENDER_FRAME;
    let mut layer = store
        .render_layers(actor, alpha, [0.0; 2], [0.0; 3], &mut budget, false)
        .unwrap()
        .render[0]
        .clone();
    if layer.pose.is_empty() {
        let rig = store.get(actor.runtime_id).unwrap();
        layer.previous_pose = Arc::from(rig.previous);
        layer.pose = Arc::from(rig.current);
    }
    layer
}

/// Compares query values with enough tolerance for one float interpolation step.
fn assert_angles(actual: [f32; 4], expected: [f32; 3]) {
    for (actual, expected) in actual.into_iter().zip(expected) {
        assert!((actual - expected).abs() < 1.0e-4, "{actual} vs {expected}");
    }
}

#[test]
fn local_and_remote_player_rotations_sample_the_same_partial_tick() {
    for local in [false, true] {
        let (mut store, mut actors) = fixture(180.0, 0.0);
        let actor = actors.get_mut(&1).unwrap();
        actor.yaw = 90.0;
        actor.head_yaw = 100.0;
        actor.pitch = 40.0;
        store.advance_tick(&actors, None, local.then_some(1), true, true, |_| {
            ActorTickContext {
                is_local: local,
                is_local_player: local,
                ..Default::default()
            }
        });
        let rig = store.get(1).unwrap();
        assert_eq!((rig.previous_body_yaw, rig.body_yaw), (0.0, 33.75));
        let authored = authored_state(&store);
        for alpha in [0.0, 0.5, 1.0] {
            let layer = sample(&store, &actors[&1], alpha);
            let expected = [66.25 * alpha, 40.0 * alpha, 33.75 * alpha];
            assert_angles(layer.pose[0].translation_scale, expected);
            assert_angles(layer.previous_pose[0].translation_scale, expected);
            assert_eq!(authored_state(&store), authored);
        }
        // New input between ticks must not bypass the tick pair used to pose the model.
        actors.get_mut(&1).unwrap().head_yaw = -100.0;
        assert_angles(
            sample(&store, &actors[&1], 0.5).pose[0].translation_scale,
            [33.125, 20.0, 16.875],
        );
    }
}

#[test]
fn head_yaw_crosses_180_along_the_short_arc() {
    for (from, to, direction) in [(179.0, -179.0, 1.0), (-179.0, 179.0, -1.0)] {
        let (mut store, mut actors) = fixture(180.0, from);
        let actor = actors.get_mut(&1).unwrap();
        actor.yaw = to;
        actor.head_yaw = to;
        store.advance_tick(&actors, None, None, true, true, |_| Default::default());
        for alpha in [0.0, 0.5, 1.0] {
            let head = sample(&store, &actors[&1], alpha).pose[0].translation_scale[0];
            assert!((head - direction * 2.0 * alpha).abs() < 1.0e-4);
        }
    }
}

#[test]
fn rotations_use_adjacent_simulation_ticks_when_a_visual_tick_is_skipped() {
    let (mut store, mut actors) = fixture(180.0, 0.0);
    actors.get_mut(&1).unwrap().head_yaw = 60.0;
    store.advance_tick(&actors, None, None, false, true, |_| Default::default());
    actors.get_mut(&1).unwrap().head_yaw = 90.0;
    store.advance_tick(&actors, None, None, true, false, |_| Default::default());
    for (alpha, expected) in [(0.0, 60.0), (0.5, 75.0), (1.0, 90.0)] {
        assert_angles(
            sample(&store, &actors[&1], alpha).pose[0].translation_scale,
            [expected, 0.0, 0.0],
        );
    }
}

#[test]
fn relative_head_clamp_runs_after_angle_sampling_and_body_catch_up() {
    for direction in [-1.0, 1.0] {
        let (mut store, mut actors) = fixture(20.0, 0.0);
        let actor = actors.get_mut(&1).unwrap();
        actor.yaw = 90.0 * direction;
        actor.head_yaw = actor.yaw;
        store.advance_tick(&actors, None, None, true, true, |_| Default::default());
        let body = store.get(1).unwrap().body_yaw;
        assert_eq!(body, 33.75 * direction);
        for (alpha, expected) in [(0.0, 0.0), (0.5, 20.0), (1.0, 20.0)] {
            let head = sample(&store, &actors[&1], alpha).pose[0].translation_scale[0];
            assert!((head - expected * direction).abs() < 1.0e-4);
        }
        store.advance_tick(&actors, None, None, true, true, |_| Default::default());
        assert_eq!(store.get(1).unwrap().body_yaw, 38.4375 * direction);
    }
}

#[test]
fn rotation_sampling_keeps_unrelated_animation_endpoints() {
    let (mut store, mut actors) = fixture(180.0, 0.0);
    let actor = actors.get_mut(&1).unwrap();
    actor.yaw = 90.0;
    actor.head_yaw = actor.yaw;
    store.advance_tick(&actors, None, None, true, true, |_| Default::default());
    let rig = store.get(1).unwrap();
    let previous = rig.previous[0].rotation;
    let current = rig.current[0].rotation;
    assert_ne!(previous, current, "ordinary channel must change each tick");
    let layer = sample(&store, &actors[&1], 0.5);
    assert_eq!(layer.previous_pose[0].rotation, previous);
    assert_eq!(layer.pose[0].rotation, current);
}

#[test]
fn first_person_and_perspective_changes_do_not_blend_old_world_rotations() {
    let (mut store, mut actors) = fixture(180.0, 0.0);
    let actor = actors.get_mut(&1).unwrap();
    actor.yaw = 90.0;
    actor.head_yaw = actor.yaw;
    actor.pitch = 40.0;
    let first_person = |_: &TestActor| ActorTickContext {
        is_local: true,
        is_local_player: true,
        is_local_first_person: true,
        ..Default::default()
    };
    store.advance_tick(&actors, None, Some(1), true, true, first_person);
    for alpha in [0.0, 0.5, 1.0] {
        assert_angles(
            sample(&store, &actors[&1], alpha).pose[0].translation_scale,
            [0.0; 3],
        );
    }
    let tick = store.get(1).unwrap().completed_tick;
    store.prepare_frame(&actors, None, Some(1), true, |_| ActorTickContext {
        is_local: true,
        is_local_player: true,
        animation_elapsed_ticks: Some(0),
        ..Default::default()
    });
    assert_eq!(store.get(1).unwrap().completed_tick, tick);
    for alpha in [0.0, 0.5, 1.0] {
        assert_angles(
            sample(&store, &actors[&1], alpha).pose[0].translation_scale,
            [56.25, 40.0, 33.75],
        );
    }
}

#[test]
fn pitch_and_body_yaw_each_wrap_without_changing_other_endpoint_inputs() {
    let previous = ActorTickInput {
        yaw: 179.0,
        head_yaw: 179.0,
        body_yaw: 179.0,
        pitch: 179.0,
        ..Default::default()
    };
    let current = ActorTickInput {
        yaw: -179.0,
        head_yaw: -179.0,
        body_yaw: -179.0,
        pitch: -179.0,
        walk_distance: 3.0,
        ..Default::default()
    };
    for (alpha, expected) in [(0.0, 179.0), (0.5, 180.0), (1.0, 181.0)] {
        let sampled = current.sample_rotation(previous, current, alpha);
        assert_eq!(
            [
                sampled.yaw,
                sampled.head_yaw,
                sampled.body_yaw,
                sampled.pitch
            ],
            [expected; 4]
        );
        assert_eq!(sampled.walk_distance, current.walk_distance);
    }
}
