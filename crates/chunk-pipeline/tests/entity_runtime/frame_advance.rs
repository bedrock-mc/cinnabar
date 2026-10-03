use super::*;

/// Builds a crowded animated frame with a bounded, expensive authored weight expression.
fn crowded_stream() -> WorldStream {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    with_weight_program(
        &mut compiled,
        vec![
            MolangOp::Push(scalar(5_000.0)),
            MolangOp::LoopStart(3),
            MolangOp::LoopNext(2),
            MolangOp::Push(scalar(1.0)),
        ],
        1,
    );
    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    for index in 0..128 {
        stream
            .submit(
                index + 1,
                spawn(index + 100, -(index as i64) - 100, [1.0, 0.0, 0.0]),
            )
            .unwrap();
    }
    stream.advance_actor_interpolation_ticks(1);
    stream
}

/// Measures a one-second gap independently of rendering, asset loading, and event admission.
#[test]
#[ignore = "benchmark"]
fn frame_advance_long_gap_bench() {
    let mut stream = crowded_stream();
    let before = stream.actor_animation_stats().evaluated_molang_ops;
    let start = std::time::Instant::now();
    for _ in 0..50 {
        stream.advance_actor_interpolation_frame(20);
    }
    eprintln!(
        "ACTOR_LONG_GAP mean_us={:.3} molang_ops={}",
        start.elapsed().as_secs_f64() * 1e6 / 50.0,
        (stream.actor_animation_stats().evaluated_molang_ops - before) / 50
    );
}

/// Appends one expression to the synthetic fixture.
fn expression(compiled: &mut CompiledEntityAssets, program: Vec<MolangOp>, max_stack: u8) -> u32 {
    let first_op = compiled.molang_ops.len() as u32;
    let op_count = program.len() as u16;
    let mut ops = std::mem::take(&mut compiled.molang_ops).into_vec();
    ops.extend(program);
    compiled.molang_ops = ops.into_boxed_slice();
    let index = compiled.molang_expressions.len() as u32;
    let mut expressions = std::mem::take(&mut compiled.molang_expressions).into_vec();
    expressions.push(CompiledMolangExpression {
        first_op,
        op_count,
        max_stack,
    });
    compiled.molang_expressions = expressions.into_boxed_slice();
    index
}

/// Builds scripts that expose pose evaluations, transition events, and clocks through bone positions.
fn scripted_stream() -> WorldStream {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    let mut symbols = compiled.molang_symbols.into_vec();
    let ground_symbol = symbols.pop().unwrap();
    let old_ground = symbols.len() as u32;
    let mut symbol = |kind, identifier: &str| {
        let index = symbols.len() as u32;
        symbols.push(MolangSymbol {
            kind,
            identifier: identifier.into(),
        });
        index
    };
    let anim = symbol(MolangSymbolKind::Query, "query.anim_time");
    let delta = symbol(MolangSymbolKind::Query, "query.delta_time");
    let ground = symbol(ground_symbol.kind, &ground_symbol.identifier);
    let life = symbol(MolangSymbolKind::Query, "query.life_time");
    let count = symbol(MolangSymbolKind::Variable, "variable.calls");
    let events = symbol(MolangSymbolKind::Variable, "variable.events");
    compiled.molang_symbols = symbols.into_boxed_slice();
    for op in &mut compiled.molang_ops {
        if let MolangOp::LoadQuery(index) = op
            && *index == old_ground
        {
            *index = ground;
        }
    }
    let increment = |slot| {
        vec![
            MolangOp::LoadVariable(slot),
            MolangOp::Push(scalar(1.0)),
            MolangOp::Add,
            MolangOp::StoreVariable(slot),
            MolangOp::LoadVariable(slot),
        ]
    };
    let calls = expression(&mut compiled, increment(count), 2);
    let delta = expression(&mut compiled, vec![MolangOp::LoadQuery(delta)], 1);
    let elapsed = expression(&mut compiled, vec![MolangOp::LoadQuery(life)], 1);
    let event_script = expression(&mut compiled, increment(events), 2);
    let event_value = expression(&mut compiled, vec![MolangOp::LoadVariable(events)], 1);
    let clip_time = expression(&mut compiled, vec![MolangOp::LoadQuery(anim)], 1);
    let transition = expression(
        &mut compiled,
        vec![
            MolangOp::LoadQuery(life),
            MolangOp::Push(scalar(
                chunk_pipeline::ACTOR_TICK_DURATION.as_secs_f32() * 5.0,
            )),
            MolangOp::Greater,
        ],
        2,
    );
    compiled.controller_transitions[0].condition = transition;
    compiled.controller_states[0].animation_count = 1;
    compiled.controller_states[1].first_animation = 1;
    let mut animations = compiled.controller_animations.into_vec();
    animations.push(animations[0]);
    compiled.controller_animations = animations.into_boxed_slice();
    compiled.controller_states[0].on_exit = Some(event_script);
    compiled.controller_states[1].on_entry = Some(event_script);
    compiled.animation_clips[0].channel_count = 2;
    let mut channels = compiled.animation_channels.into_vec();
    channels[0].keyframe_count = 1;
    channels.push(EntityAnimationChannel {
        bone: 0,
        property: EntityAnimationProperty::Translation,
        first_keyframe: 1,
        keyframe_count: 1,
    });
    compiled.animation_channels = channels.into_boxed_slice();
    compiled.animation_keyframes[0].expressions = [Some(calls), Some(delta), Some(elapsed)];
    compiled.animation_keyframes[1].time_seconds = scalar(0.0);
    compiled.animation_keyframes[1].value = [scalar(0.0); 3];
    compiled.animation_keyframes[1].expressions = [None, Some(clip_time), Some(event_value)];
    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    stream
}

/// One visual update preserves elapsed clocks and transition hooks without replaying keyframe scripts.
#[test]
fn long_frame_advances_time_and_events_with_one_visual_evaluation() {
    let mut stream = scripted_stream();
    stream.advance_actor_interpolation_frame(1);
    let previous = stream.actor_rig(42).unwrap().current.to_vec();
    stream.advance_actor_interpolation_frame(20);
    let rig = stream.actor_rig(42).unwrap();
    assert_eq!(rig.completed_tick, 21);
    assert_eq!(rig.rest_completed_tick, 21);
    assert_eq!(rig.previous, previous);
    let (root, wing) = (
        rig.current[0].translation_scale,
        rig.current[1].translation_scale,
    );
    assert_eq!(root[2], 2.0, "one on_exit and one on_entry");
    assert_eq!(
        root[1], 0.0,
        "the new controller state starts at this frame's time"
    );
    assert_eq!(
        wing[0], -2.0,
        "the keyframe script runs once per published frame"
    );
    let tick = chunk_pipeline::ACTOR_TICK_DURATION.as_secs_f32();
    assert!((wing[1] - root[1] - 2.0 - 20.0 * tick).abs() < 1.0e-5);
    assert!((wing[2] - root[2] - 21.0 * tick).abs() < 1.0e-5);
    let current = rig.current.to_vec();
    let stats = stream.actor_animation_stats();
    stream.advance_actor_interpolation_frame(0);
    assert_eq!(stream.actor_rig(42).unwrap().current, current);
    assert_eq!(stream.actor_animation_stats(), stats);
    stream.advance_actor_interpolation_frame(1);
    let rig = stream.actor_rig(42).unwrap();
    assert_eq!(rig.completed_tick, 22);
    assert!((rig.current[0].translation_scale[1] - tick).abs() < 1.0e-5);
    assert_eq!(rig.current[0].translation_scale[2], 2.0);
}

/// Normal single-tick publication keeps exact poses, clocks, motion, and evaluation counts.
#[test]
fn single_tick_frames_match_explicit_tick_snapshots() {
    let (mut legacy, mut frame) = (scripted_stream(), scripted_stream());
    for ticks in [0, 1].into_iter().cycle().take(80) {
        legacy.advance_actor_interpolation_ticks(ticks);
        frame.advance_actor_interpolation_frame(ticks);
        let (left, right) = (legacy.actor_rig(42).unwrap(), frame.actor_rig(42).unwrap());
        assert_eq!(left.current, right.current);
        assert_eq!(left.previous, right.previous);
        assert_eq!(left.completed_tick, right.completed_tick);
        assert_eq!(left.body_yaw, right.body_yaw);
        assert_eq!(left.hand, right.hand);
        assert_eq!(legacy.actor(42), frame.actor(42));
        assert_eq!(
            legacy.actor_animation_stats(),
            frame.actor_animation_stats()
        );
    }
}

/// Every physical tick still advances, while visual Molang work has one tick's budget.
#[test]
fn gap_preserves_motion_and_bounds_visual_work() {
    let (mut legacy, mut frame) = (crowded_stream(), crowded_stream());
    let mut movement = turn(129, 90.0);
    if let WorldEvent::Actor(ActorEvent::Move(event)) = &mut movement {
        event.runtime_id = 100;
    }
    legacy.submit(129, movement.clone()).unwrap();
    frame.submit(129, movement).unwrap();
    let before = frame.actor_animation_stats().evaluated_molang_ops;
    legacy.advance_actor_interpolation_ticks(20);
    frame.advance_actor_interpolation_frame(20);
    for id in 100..228 {
        assert_eq!(legacy.actor(id), frame.actor(id));
        let (left, right) = (legacy.actor_rig(id).unwrap(), frame.actor_rig(id).unwrap());
        assert_eq!(left.body_yaw, right.body_yaw);
        assert_eq!(left.hand, right.hand);
        assert_eq!(left.completed_tick, right.completed_tick);
    }
    assert!(
        frame.actor_animation_stats().evaluated_molang_ops - before
            <= MAX_MOLANG_OPS_PER_WORLD_TICK as u64
    );
}
