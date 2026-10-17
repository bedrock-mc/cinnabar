use super::*;
use crate::test_support::scalar;
use assets::{CompiledEntityAssets, MolangSymbol, MolangSymbolKind};

/// Inserts one query and remaps every bytecode operand holding a Molang symbol.
fn add_query(compiled: &mut CompiledEntityAssets, name: &str) -> u32 {
    let mut symbols = compiled.molang_symbols.to_vec();
    let index = symbols.partition_point(|symbol| {
        (symbol.kind, symbol.identifier.as_ref()) < (MolangSymbolKind::Query, name)
    }) as u32;
    assert!(!symbols.iter().any(|symbol| {
        symbol.kind == MolangSymbolKind::Query && symbol.identifier.as_ref() == name
    }));
    symbols.insert(
        index as usize,
        MolangSymbol {
            kind: MolangSymbolKind::Query,
            identifier: name.into(),
        },
    );
    compiled.molang_symbols = symbols.into();

    for op in &mut compiled.molang_ops {
        let symbol = match op {
            MolangOp::PushString(symbol)
            | MolangOp::LoadQuery(symbol)
            | MolangOp::LoadVariable(symbol)
            | MolangOp::StoreVariable(symbol) => symbol,
            MolangOp::CallQuery(call) => &mut call.symbol,
            MolangOp::Coalesce(branch)
            | MolangOp::ForEachStart(branch)
            | MolangOp::ForEachNext(branch) => &mut branch.symbol,
            _ => continue,
        };
        if *symbol >= index {
            *symbol += 1;
        }
    }
    index
}

/// Finds a symbol after all insertions have completed.
fn find_symbol(compiled: &CompiledEntityAssets, name: &str) -> u32 {
    compiled
        .molang_symbols
        .iter()
        .position(|symbol| symbol.identifier.as_ref() == name)
        .unwrap() as u32
}

/// Appends an expression with its exact bounded stack requirement.
fn expression(compiled: &mut CompiledEntityAssets, code: &[MolangOp], max_stack: u8) -> u32 {
    let mut expressions = compiled.molang_expressions.to_vec();
    let index = expressions.len() as u32;
    expressions.push(CompiledMolangExpression {
        first_op: compiled.molang_ops.len() as u32,
        op_count: code.len() as u16,
        max_stack,
    });
    let mut ops = compiled.molang_ops.to_vec();
    ops.extend_from_slice(code);
    compiled.molang_expressions = expressions.into();
    compiled.molang_ops = ops.into();
    index
}

/// Extends the proven player fixture with lifetime and optional held assignments.
fn held_fixture(effects: bool) -> (CompiledEntityAssets, u32) {
    let mut compiled = compiled_fixture(180.0);
    let life = add_query(&mut compiled, "query.life_time");
    let size = find_symbol(&compiled, "variable.charge_amount");
    let body = find_symbol(&compiled, "query.body_y_rotation");

    let held = if effects {
        let initialize = expression(
            &mut compiled,
            &[
                MolangOp::Push(scalar(2.0)),
                MolangOp::StoreVariable(size),
                MolangOp::Push(scalar(0.0)),
            ],
            1,
        );
        compiled.rig_bindings[0].initialize = Some(initialize);
        expression(
            &mut compiled,
            &[
                MolangOp::LoadVariable(size),
                MolangOp::Push(scalar(1.0)),
                MolangOp::Add,
                MolangOp::StoreVariable(size),
                MolangOp::LoadVariable(size),
                MolangOp::LoadQuery(life),
                MolangOp::Add,
            ],
            2,
        )
    } else {
        expression(
            &mut compiled,
            &[
                MolangOp::Push(scalar(1.0)),
                MolangOp::LoadQuery(life),
                MolangOp::Add,
            ],
            2,
        )
    };
    compiled.rig_bindings[0].held_item_scale = Some(held);

    if effects {
        // The zero product makes these outputs motion-dependent while preserving
        // their independently specified size and lifetime values.
        let size_read = expression(
            &mut compiled,
            &[
                MolangOp::LoadQuery(body),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Multiply,
                MolangOp::LoadVariable(size),
                MolangOp::Add,
            ],
            2,
        );
        let time_read = expression(
            &mut compiled,
            &[
                MolangOp::LoadQuery(body),
                MolangOp::Push(scalar(0.0)),
                MolangOp::Multiply,
                MolangOp::LoadQuery(life),
                MolangOp::Add,
                MolangOp::Push(scalar(1.0)),
                MolangOp::Add,
            ],
            2,
        );
        let unit = expression(&mut compiled, &[MolangOp::Push(scalar(1.0))], 1);
        compiled.rig_bindings[0].scale_expressions = Some([size_read, time_read, unit, unit]);

        let mut keys = compiled.animation_keyframes.to_vec();
        let mut key = keys[0];
        key.value = [scalar(1.0); 3];
        key.expressions = [Some(size_read), Some(time_read), Some(unit)];
        let first_keyframe = keys.len() as u32;
        keys.push(key);
        compiled.animation_keyframes = keys.into();

        let mut channels = compiled.animation_channels.to_vec();
        let mut channel = channels[0].clone();
        channel.property = EntityAnimationProperty::Scale;
        channel.first_keyframe = first_keyframe;
        channels.push(channel);
        compiled.animation_clips[0].channel_count = channels.len() as u32;
        compiled.animation_channels = channels.into();
    }

    (compiled, life)
}

/// Checks that the retained frame satisfies the actual isolated-path predicate.
fn motion_frame(store: &ActorAnimationStore, life: u32, turning: bool) -> &FrameState {
    let state = store.rigs.values().next().unwrap();
    assert!(state.initialized && !state.reset_pending && !state.culled);
    let frame = state.render_frame.as_ref().unwrap();
    assert!(frame.motion_poses.is_some());
    assert!(frame.previous_motion.is_some());
    assert!(frame.motion.sampling.as_ref().unwrap().uses_rotation());
    assert_eq!(frame.needs_rotation_sampling(), turning);
    assert!(!frame.needs_walk_sampling());
    assert_eq!(frame.motion.context.frame_alpha, 0.0);
    assert!(
        frame
            .motion
            .queries
            .iter()
            .any(|(symbol, _)| *symbol == life)
    );
    frame
}

/// Returns the retained lifetime query without running another evaluator.
fn frozen_lifetime(frame: &MotionEndpointState, life: u32) -> f32 {
    frame
        .queries
        .iter()
        .find(|(symbol, _)| *symbol == life)
        .unwrap()
        .1
        .number()
}

/// Samples through the public frame API and checks cache and authored-state isolation.
fn sampled<'a>(
    store: &'a ActorAnimationStore,
    actors: &'a HashMap<u64, TestActor>,
    alpha: f32,
) -> ActorRenderLayers<'a> {
    let before = authored_state(store);
    let fallbacks = store
        .frame_fallbacks
        .load(std::sync::atomic::Ordering::Relaxed);
    let tick = store.get(1).unwrap();
    let mut frame = ActorRenderFrame::new(store, actors, [0.0; 2], [0.0; 3], alpha);
    if alpha < 0.5 {
        frame.layers(1).unwrap();
    }
    let held = frame.sample_held_item_scale(tick).held_item_scale;
    let layers = frame.layers_with_skin(1).unwrap();
    assert!(matches!(&layers.render, Cow::Owned(_)));
    assert_eq!(layers.held_item_scale, held);
    let remaining = frame.remaining_ops();
    assert!(remaining < MAX_MOLANG_OPS_PER_RENDER_FRAME);
    assert_eq!(frame.sample_held_item_scale(tick).held_item_scale, held);
    frame.layers(1).unwrap();
    assert_eq!(frame.remaining_ops(), remaining);
    assert_eq!(authored_state(store), before);
    assert_eq!(
        store
            .frame_fallbacks
            .load(std::sync::atomic::Ordering::Relaxed),
        fallbacks
    );
    layers
}

/// Advances both controls equally, changing only the rotating control's angles.
fn advance(store: &mut ActorAnimationStore, actors: &mut HashMap<u64, TestActor>, turn: bool) {
    if turn {
        let actor = actors.get_mut(&1).unwrap();
        actor.yaw = 90.0;
        actor.head_yaw = 90.0;
    }
    store.advance_tick(actors, None, None, true, true, |_| Default::default());
}

/// Compares bounded scalar outputs with a tolerance below the tested frame advance.
fn close(actual: f32, expected: f32) {
    assert!((actual - expected).abs() < 1.0e-6, "{actual} vs {expected}");
}

/// Collects both alphas before checking the suspected lifetime discrepancy.
fn lifetime_samples(turn: bool) -> ([f32; 2], [f32; 2], u64) {
    let (compiled, life) = held_fixture(false);
    let (mut store, mut actors) = fixture_with(compiled, 0.0, Default::default());
    advance(&mut store, &mut actors, turn);
    let history = motion_frame(&store, life, turn);
    let life_tick = history.motion.life_tick;
    let tick = store.get(1).unwrap();
    close(
        tick.held_item_scale.unwrap(),
        1.0 + frozen_lifetime(&history.motion, life),
    );
    let expected = [0.25, 0.75]
        .map(|alpha| tick.held_item_scale.unwrap() + alpha * ACTOR_TICK_DURATION.as_secs_f32());
    let actual = [0.25, 0.75].map(|alpha| sampled(&store, &actors, alpha).held_item_scale.unwrap());
    (actual, expected, life_tick)
}

#[test]
fn held_lifetime_matches_stationary_control_with_isolated_rotation() {
    let (stationary, expected, lifetime) = lifetime_samples(false);
    let (rotating, rotating_expected, rotating_lifetime) = lifetime_samples(true);
    assert_eq!(lifetime, rotating_lifetime);
    assert_eq!(expected, rotating_expected);
    for index in 0..2 {
        close(stationary[index], expected[index]);
        close(rotating[index], expected[index]);
    }
    close(
        rotating[1] - rotating[0],
        0.5 * ACTOR_TICK_DURATION.as_secs_f32(),
    );
}

#[test]
fn held_effects_precede_pre_animation_and_body_scale() {
    let (compiled, life) = held_fixture(true);
    let (store, actors) = fixture_with(compiled, 0.0, Default::default());
    let history = store
        .rigs
        .values()
        .next()
        .unwrap()
        .render_frame
        .as_ref()
        .unwrap();
    let tick_life = frozen_lifetime(&history.motion, life);
    let tick = store.get(1).unwrap();
    close(tick.held_item_scale.unwrap(), 3.0 + tick_life);
    close(tick.scale, 4.0);
    for alpha in [0.25, 0.75] {
        close(
            sampled(&store, &actors, alpha).held_item_scale.unwrap(),
            3.0 + tick_life + alpha * ACTOR_TICK_DURATION.as_secs_f32(),
        );
    }
}

#[test]
fn held_effects_replay_once_in_both_motion_endpoints() {
    let (compiled, life) = held_fixture(true);
    let (mut store, mut actors) = fixture_with(compiled, 0.0, Default::default());
    advance(&mut store, &mut actors, true);
    let history = motion_frame(&store, life, true);
    let previous_life = frozen_lifetime(history.previous_motion.as_ref().unwrap(), life);
    let current_life = frozen_lifetime(&history.motion, life);
    assert!(previous_life < current_life);

    for alpha in [0.25, 0.75] {
        let layers = sampled(&store, &actors, alpha);
        close(
            layers.held_item_scale.unwrap(),
            5.0 + current_life + alpha * ACTOR_TICK_DURATION.as_secs_f32(),
        );
        let scale = layers.render[0].sampled_scale.unwrap();
        close(scale[0], 6.0);
        close(scale[1], 1.0 + current_life);

        let previous = crate::test_support::total_scale(&layers.render[0].previous_pose[0]);
        let current = crate::test_support::total_scale(&layers.render[0].pose[0]);
        close(previous[0], 4.0);
        close(current[0], 6.0);
        close(previous[1], 1.0 + previous_life);
        close(current[1], 1.0 + current_life);
    }

    let before = authored_state(&store);
    let tick = store.get(1).unwrap();
    for budget in [0, 1] {
        let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.5);
        frame.set_remaining_ops(budget);
        assert_eq!(
            frame.sample_held_item_scale(tick).held_item_scale,
            tick.held_item_scale
        );
        assert_eq!(authored_state(&store), before);
    }
}
