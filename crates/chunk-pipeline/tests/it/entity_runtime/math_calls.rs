use super::*;
use assets::MolangFunction;
use sha2::{Digest, Sha256};

/// Makes every sampled clip expose its weight directly in the wing's X translation.
fn math_assets(program: Vec<MolangOp>, stack: u8) -> CompiledEntityAssets {
    let mut compiled = compiled_entity_assets(EntityRigFallback::Skip);
    compiled.controllers[0].initial_state = 1;
    for keyframe in &mut compiled.animation_keyframes {
        keyframe.value[0] = scalar(1.0);
    }
    with_weight_program(&mut compiled, program, stack);
    compiled
}

/// Spawns one rig with a repeatable lifetime and random seed.
fn math_stream(compiled: &CompiledEntityAssets, speed: f32) -> WorldStream {
    let mut stream = stream_with_entity_assets(decode_entity_assets(compiled));
    stream.submit(1, spawn(42, -7, [speed, 0.0, 0.0])).unwrap();
    stream
}

/// Hashes published float bits without relying on debug formatting.
fn pose_digest(stream: &WorldStream, hash: &mut Sha256) {
    let rig = stream.authority().actor_rig(42).unwrap();
    for bone in rig.current {
        for value in bone
            .rotation
            .iter()
            .chain(&bone.translation_scale)
            .chain(&bone.axis_scale)
        {
            hash.update(value.to_bits().to_le_bytes());
        }
    }
}

/// One-, two-, and three-argument calls preserve order and values below their arguments.
#[test]
fn argument_arities_preserve_prefix_and_numeric_string_conversion() {
    for (program, stack, expected) in [
        (
            vec![
                MolangOp::Push(scalar(-2.0)),
                MolangOp::Call(MolangFunction::Abs),
            ],
            1,
            2.0,
        ),
        (
            vec![
                MolangOp::Push(scalar(2.0)),
                MolangOp::Push(scalar(3.0)),
                MolangOp::Call(MolangFunction::Pow),
            ],
            2,
            8.0,
        ),
        (
            vec![
                MolangOp::Push(scalar(5.0)),
                MolangOp::Push(scalar(10.0)),
                MolangOp::Push(scalar(20.0)),
                MolangOp::Push(scalar(0.25)),
                MolangOp::Call(MolangFunction::Lerp),
                MolangOp::Add,
            ],
            4,
            17.5,
        ),
    ] {
        let mut stream = math_stream(&math_assets(program, stack), 1.0);
        stream.advance_actor_interpolation_ticks(1);
        assert_eq!(
            stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0],
            -expected
        );
    }
    let compiled = compiled_entity_assets(EntityRigFallback::Skip);
    let string = compiled.molang_symbols.len() as u32;
    let mut symbols = compiled.molang_symbols.into_vec();
    symbols.push(MolangSymbol {
        kind: MolangSymbolKind::String,
        identifier: "text".into(),
    });
    let mut compiled = math_assets(
        vec![
            MolangOp::PushString(string),
            MolangOp::Push(scalar(2.0)),
            MolangOp::Push(scalar(0.25)),
            MolangOp::Call(MolangFunction::Lerp),
        ],
        3,
    );
    compiled.molang_symbols = symbols.into_boxed_slice();
    let mut stream = math_stream(&compiled, 1.0);
    stream.advance_actor_interpolation_ticks(1);
    assert_eq!(
        stream.authority().actor_rig(42).unwrap().current[1].translation_scale[0],
        -0.5
    );
}

/// Non-finite results still freeze the pose rather than committing partial animation state.
#[test]
fn nan_math_retains_the_previous_pose() {
    let mut stream = math_stream(
        &math_assets(
            vec![
                MolangOp::Push(scalar(-1.0)),
                MolangOp::Call(MolangFunction::Sqrt),
            ],
            1,
        ),
        1.0,
    );
    let previous = stream.authority().actor_rig(42).unwrap().current.to_vec();
    stream.advance_actor_interpolation_ticks(2);
    let rig = stream.authority().actor_rig(42).unwrap();
    assert_eq!(rig.current, previous);
    assert_eq!(rig.completed_tick, 0);
    assert_eq!(stream.authority().actor_animation_stats().frozen_actors, 2);
}

/// A failed budget discards random draws from the transaction before the next successful call.
#[test]
fn budget_failure_preserves_the_random_sequence() {
    let compiled = math_assets(
        vec![
            MolangOp::LoadQuery(2),
            MolangOp::JumpIfFalse(9),
            MolangOp::Push(scalar(5_000.0)),
            MolangOp::LoopStart(9),
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(1.0)),
            MolangOp::Call(MolangFunction::Random),
            MolangOp::Pop,
            MolangOp::LoopNext(4),
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(1.0)),
            MolangOp::Call(MolangFunction::Random),
        ],
        2,
    );
    let (mut failed, mut clean) = (math_stream(&compiled, 1.0), math_stream(&compiled, 0.0));
    failed.advance_actor_interpolation_ticks(1);
    assert_eq!(
        failed
            .authority()
            .actor_animation_stats()
            .actor_budget_exhaustions,
        1
    );
    assert_eq!(failed.authority().actor_rig(42).unwrap().completed_tick, 0);
    let mut stop = turn(2, 0.0);
    if let WorldEvent::Actor(ActorEvent::Move(movement)) = &mut stop {
        movement.position = [Some(0.0), Some(64.0), Some(0.0)];
    }
    failed.submit(2, stop).unwrap();
    failed.advance_actor_interpolation_ticks(1);
    clean.advance_actor_interpolation_ticks(1);
    assert_eq!(
        failed.authority().actor_rig(42).unwrap().current,
        clean.authority().actor_rig(42).unwrap().current
    );
}

/// The digest records cross-build RNG order for one-, two-, and three-argument math calls.
#[test]
fn random_math_pose_digest() {
    let compiled = math_assets(
        vec![
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(1.0)),
            MolangOp::Call(MolangFunction::Random),
            MolangOp::Push(scalar(1.0)),
            MolangOp::Push(scalar(4.0)),
            MolangOp::Call(MolangFunction::RandomInteger),
            MolangOp::Add,
            MolangOp::Push(scalar(2.0)),
            MolangOp::Push(scalar(1.0)),
            MolangOp::Push(scalar(6.0)),
            MolangOp::Call(MolangFunction::DieRoll),
            MolangOp::Add,
            MolangOp::Call(MolangFunction::Abs),
        ],
        4,
    );
    let mut stream = math_stream(&compiled, 1.0);
    let mut digest = Sha256::new();
    for _ in 0..64 {
        stream.advance_actor_interpolation_ticks(1);
        pose_digest(&stream, &mut digest);
    }
    assert_eq!(
        format!("{:x}", digest.finalize()),
        "f59ab272b28fbde25ab28ea43efb9bcb5a96a75f708917d84f06af209f2b138a"
    );
}

/// Measures evaluator calls after construction and warm-up, independently of frame publication.
#[test]
#[ignore = "benchmark"]
fn math_call_arguments_bench() {
    let mut program = Vec::new();
    for _ in 0..64 {
        program.extend([
            MolangOp::Push(scalar(-0.5)),
            MolangOp::Call(MolangFunction::Abs),
            MolangOp::Pop,
            MolangOp::Push(scalar(2.0)),
            MolangOp::Push(scalar(3.0)),
            MolangOp::Call(MolangFunction::Pow),
            MolangOp::Pop,
            MolangOp::Push(scalar(0.25)),
            MolangOp::Push(scalar(0.0)),
            MolangOp::Push(scalar(1.0)),
            MolangOp::Call(MolangFunction::Clamp),
            MolangOp::Pop,
        ]);
    }
    program.push(MolangOp::Push(scalar(1.0)));
    let compiled = math_assets(program, 3);
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
    let before = stream
        .authority()
        .actor_animation_stats()
        .evaluated_molang_ops;
    let start = std::time::Instant::now();
    for _ in 0..100 {
        stream.advance_actor_interpolation_ticks(1);
    }
    eprintln!(
        "MATH_CALLS mean_us={:.3} calls_per_tick={} molang_ops={}",
        start.elapsed().as_secs_f64() * 1e6 / 100.0,
        128 * 64 * 3,
        (stream
            .authority()
            .actor_animation_stats()
            .evaluated_molang_ops
            - before)
            / 100
    );
}
