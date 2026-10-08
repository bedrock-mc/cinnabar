use super::*;
use client_world::ACTOR_TICK_DURATION;

fn clock_stream(program: Vec<MolangOp>, length: f32, mode: EntityAnimationLoop) -> WorldStream {
    configured_clock_stream(program, length, mode, |_| {})
}

fn configured_clock_stream(
    program: Vec<MolangOp>,
    length: f32,
    mode: EntityAnimationLoop,
    configure: impl FnOnce(&mut CompiledEntityAssets),
) -> WorldStream {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "move"),
        (MolangSymbolKind::Name, "moving"),
        (MolangSymbolKind::Query, "query.anim_time"),
        (MolangSymbolKind::Query, "query.delta_time"),
        (MolangSymbolKind::Query, "query.ground_speed"),
        (MolangSymbolKind::Query, "query.modified_distance_moved"),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    // Keep the clock active in the initial state, independently of packet velocity.
    // Individual tests can replace the dormant transition condition below.
    compiled.molang_ops = vec![MolangOp::Push(scalar(0.0))].into();
    compiled.controller_states[0].animation_count = 1;
    compiled.controller_states[1].animation_count = 0;
    compiled.controller_states[1].first_animation = 1;
    let stack = assets::molang_program_stack(&program).unwrap();
    let clock = frame_advance::expression(&mut compiled, program, stack);
    let value = frame_advance::expression(&mut compiled, vec![MolangOp::LoadQuery(2)], 1);
    compiled.animation_clips[0].anim_time_update = Some(clock);
    compiled.animation_clips[0].length_seconds = scalar(length);
    compiled.animation_clips[0].loop_mode = mode;
    compiled.animation_channels[0].keyframe_count = 1;
    compiled.animation_keyframes = vec![compiled.animation_keyframes[0]].into();
    compiled.animation_keyframes[0].expressions[0] = Some(value);
    configure(&mut compiled);
    let mut stream = stream_with_entity_assets(decode_entity_assets(&compiled));
    stream.submit(1, spawn(42, -7, [1.0, 0.0, 0.0])).unwrap();
    stream
}

fn time(stream: &WorldStream) -> f32 {
    -stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0]
}

fn move_to(stream: &mut WorldStream, sequence: u64, x: f32, teleported: bool) {
    stream
        .submit(
            sequence,
            WorldEvent::Actor(ActorEvent::Move(ActorMoveEvent {
                dimension: 0,
                runtime_id: 42,
                position: [Some(x), None, None],
                position_origin: ActorPositionOrigin::Feet,
                pitch: None,
                yaw: None,
                head_yaw: None,
                on_ground: Some(true),
                teleported,
                player_mode: None,
                source_tick: Some(sequence),
                interpolation: Default::default(),
            })),
        )
        .unwrap();
}

#[test]
fn distance_clock_holds_at_rest_and_tracks_interpolated_movement_not_packet_velocity() {
    let mut stream = clock_stream(vec![MolangOp::LoadQuery(5)], 0.0, EntityAnimationLoop::Loop);
    stream.advance_actor_interpolation_ticks(120);
    assert_eq!(
        time(&stream),
        0.0,
        "wall time and stale spawn velocity cannot drive walking"
    );
    move_to(&mut stream, 2, 0.3, false);
    for expected in [0.16, 0.416, 0.7296] {
        stream.advance_actor_interpolation_ticks(1);
        assert!((time(&stream) - expected).abs() < 1e-5);
    }
    stream.advance_actor_interpolation_ticks(80);
    let stopped = time(&stream);
    assert!(
        stopped > 1.0,
        "zero-length procedural clips keep an unbounded distance clock"
    );
    stream.advance_actor_interpolation_ticks(80);
    assert_eq!(
        time(&stream),
        stopped,
        "walking settles once displacement stops"
    );
    move_to(&mut stream, 3, 1000.0, true);
    stream.advance_actor_interpolation_ticks(1);
    assert_eq!(
        time(&stream),
        stopped,
        "teleport gap is excluded by motion-history reset"
    );
}

#[test]
fn scripted_clock_assigns_previous_time_plus_the_full_frame_delta() {
    let delta = ACTOR_TICK_DURATION.as_secs_f32();
    let mut stream = clock_stream(
        vec![
            MolangOp::LoadQuery(2),
            MolangOp::LoadQuery(3),
            MolangOp::Add,
        ],
        0.0,
        EntityAnimationLoop::Loop,
    );
    stream.advance_actor_interpolation_frame(1);
    assert!((time(&stream) - delta).abs() < 1e-6);
    stream.advance_actor_interpolation_frame(20);
    assert!((time(&stream) - 21.0 * delta).abs() < 1e-6);
    move_to(&mut stream, 2, 100.0, true);
    stream.advance_actor_interpolation_frame(1);
    assert!(
        (time(&stream) - delta).abs() < 1e-6,
        "reset discards the previous clip clock"
    );
}

#[test]
fn zero_weight_pauses_a_scripted_clock_and_resumes_its_previous_time() {
    let delta = ACTOR_TICK_DURATION.as_secs_f32();
    let mut stream = configured_clock_stream(
        vec![
            MolangOp::LoadQuery(2),
            MolangOp::LoadQuery(3),
            MolangOp::Add,
        ],
        0.0,
        EntityAnimationLoop::Loop,
        |compiled| {
            let weight = frame_advance::expression(
                compiled,
                vec![
                    MolangOp::LoadQuery(4),
                    MolangOp::Push(scalar(0.0)),
                    MolangOp::Greater,
                ],
                2,
            );
            compiled.controller_animations[0].weight = Some(weight);
        },
    );
    stream.advance_actor_interpolation_frame(1);
    assert!((time(&stream) - delta).abs() < 1e-6);
    move_to(&mut stream, 2, 0.0, false);
    stream.advance_actor_interpolation_frame(20);
    assert_eq!(
        time(&stream),
        0.0,
        "an animation with zero weight contributes no pose"
    );
    move_to(&mut stream, 3, 0.1, false);
    stream.advance_actor_interpolation_frame(1);
    assert!(
        (time(&stream) - 2.0 * delta).abs() < 1e-6,
        "resume must keep the pre-pause clock"
    );
}

#[test]
fn reset_clears_finished_clocks_before_initial_controller_transitions() {
    let mut stream = configured_clock_stream(
        vec![MolangOp::Push(scalar(1.0))],
        1.0,
        EntityAnimationLoop::HoldOnLastFrame,
        |compiled| {
            let mut symbols = compiled.molang_symbols.to_vec();
            // Keep the carrier's query symbols in canonical lexical order.
            symbols.insert(
                2,
                MolangSymbol {
                    kind: MolangSymbolKind::Query,
                    identifier: "query.any_animation_finished".into(),
                },
            );
            symbols.sort_by(|a, b| (a.kind, &a.identifier).cmp(&(b.kind, &b.identifier)));
            let index = symbols
                .iter()
                .position(|symbol| symbol.identifier.as_ref() == "query.any_animation_finished")
                .unwrap() as u32;
            for op in &mut compiled.molang_ops {
                if let MolangOp::LoadQuery(old) = op {
                    *old = symbols
                        .iter()
                        .position(|symbol| symbol == &compiled.molang_symbols[*old as usize])
                        .unwrap() as u32;
                }
            }
            compiled.molang_symbols = symbols.into();
            let finished = frame_advance::expression(compiled, vec![MolangOp::LoadQuery(index)], 1);
            compiled.controller_transitions[0].condition = finished;
            compiled.controller_states[0].animation_count = 1;
            compiled.controller_states[1].animation_count = 0;
            compiled.controller_states[1].first_animation = 1;
        },
    );
    stream.advance_actor_interpolation_frame(1);
    assert_eq!(time(&stream), 1.0);
    move_to(&mut stream, 2, 100.0, true);
    stream.advance_actor_interpolation_frame(1);
    assert_eq!(
        time(&stream),
        1.0,
        "reset starts the controller before its clip is finished"
    );
    stream.advance_actor_interpolation_frame(1);
    assert_eq!(
        time(&stream),
        0.0,
        "next frame sees the freshly completed clip"
    );
}

#[test]
fn loop_clock_preserves_endpoint_then_wraps_before_bone_queries() {
    let mut stream = clock_stream(
        vec![MolangOp::Push(scalar(1.0))],
        1.0,
        EntityAnimationLoop::Loop,
    );
    stream.advance_actor_interpolation_ticks(1);
    assert_eq!(time(&stream), 1.0);
    let mut stream = clock_stream(
        vec![MolangOp::Push(scalar(1.25))],
        1.0,
        EntityAnimationLoop::Loop,
    );
    stream.advance_actor_interpolation_ticks(1);
    assert_eq!(time(&stream), 0.25);
    let mut stream = clock_stream(
        vec![MolangOp::Push(scalar(-0.25))],
        1.0,
        EntityAnimationLoop::Loop,
    );
    stream.advance_actor_interpolation_ticks(1);
    assert_eq!(
        time(&stream),
        -0.25,
        "reversible clocks are not clamped to zero"
    );
}

#[test]
fn one_shot_and_hold_apply_the_authored_clock() {
    let program = || vec![MolangOp::Push(scalar(1.25))];
    let mut once = clock_stream(program(), 1.0, EntityAnimationLoop::Once);
    once.advance_actor_interpolation_ticks(1);
    assert_eq!(time(&once), 0.0, "finished one-shot no longer contributes");
    let mut hold = clock_stream(program(), 1.0, EntityAnimationLoop::HoldOnLastFrame);
    hold.advance_actor_interpolation_ticks(1);
    assert_eq!(
        time(&hold),
        1.0,
        "hold exposes its clamped endpoint to bone queries"
    );
}
