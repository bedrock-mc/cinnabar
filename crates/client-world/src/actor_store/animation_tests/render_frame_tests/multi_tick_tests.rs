//! Frames that complete several ticks still blend across exactly the last tick.
use std::{collections::HashMap, sync::Arc};

use assets::{
    CompiledMolangExpression, MolangOp, MolangSymbol, MolangSymbolKind, RuntimeEntityAssets,
};

use crate::actor_store::ActorStore;
use actor_animation::test_support;

const ACTORS: u64 = 9;

/// A rig whose first bone translates by one unit per completed tick of actor age.
fn clock_assets() -> Arc<RuntimeEntityAssets> {
    let scalar = |value| assets::EntityGeometryScalar::new(value).unwrap();
    let mut compiled = test_support::counting_random_compiled("minecraft:test");
    compiled.rig_bindings[0].initialize = None;
    compiled.render.layers[0].color = None;
    compiled.molang_symbols = [
        (MolangSymbolKind::Name, "wield"),
        (MolangSymbolKind::Query, "query.life_time"),
    ]
    .map(|(kind, identifier)| MolangSymbol {
        kind,
        identifier: identifier.into(),
    })
    .into();
    compiled.molang_ops = vec![
        MolangOp::Push(scalar(0.0)),
        MolangOp::LoadQuery(1),
        MolangOp::Push(scalar(20.0)),
        MolangOp::Multiply,
        MolangOp::Push(scalar(0.0)),
        MolangOp::Push(scalar(0.0)),
    ]
    .into();
    compiled.molang_expressions = [(0, 1, 1), (1, 3, 2), (4, 1, 1), (5, 1, 1)]
        .map(|(first_op, op_count, max_stack)| CompiledMolangExpression {
            first_op,
            op_count,
            max_stack,
        })
        .into();
    Arc::new(RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap())
}

/// Each frame advances two or three ticks; the drawn age must follow frame time exactly.
#[test]
fn multi_tick_frames_blend_over_one_tick_for_every_actor() {
    let mut store = ActorStore::new_with_entity_assets(1, 0, clock_assets());
    for runtime_id in 1..=ACTORS {
        let mut actor = super::super::actor_with_metadata(HashMap::new());
        actor.runtime_id = runtime_id;
        actor.unique_id = runtime_id as i64;
        store.apply(
            1,
            runtime_id,
            protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
                dimension: 0,
                unique_id: actor.unique_id,
                runtime_id,
                kind: actor.kind,
                position: actor.position,
                velocity: actor.velocity,
                pitch: actor.pitch,
                yaw: actor.yaw,
                head_yaw: actor.head_yaw,
                body_yaw: actor.body_yaw,
                held_item: Default::default(),
                metadata: Arc::from([]),
                attributes: Arc::from([]),
                properties: Arc::from([]),
                links: Arc::from([]),
            }),
        );
    }
    store.advance_interpolation_frame(0);
    store.advance_interpolation_frame(4);
    // Frame time advances 2.3 ticks per frame, so frames complete two or three ticks.
    let mut time = 0.0_f32;
    let mut drawn: Vec<Vec<f32>> = vec![Vec::new(); ACTORS as usize];
    for _ in 0..12 {
        let before = time.floor();
        time += 2.3;
        let alpha = time.fract();
        store.advance_interpolation_frame((time.floor() - before) as u32);
        for runtime_id in 1..=ACTORS {
            let rig = store.actor_rig(runtime_id).unwrap();
            let [previous, current] =
                [rig.previous[0], rig.current[0]].map(|bone| bone.translation_scale[0]);
            drawn[runtime_id as usize - 1].push(previous + (current - previous) * alpha);
        }
    }
    for (actor, drawn) in drawn.iter().enumerate() {
        let steps: Vec<f32> = drawn.windows(2).map(|pair| pair[1] - pair[0]).collect();
        let unit = steps.iter().sum::<f32>() / (2.3 * steps.len() as f32);
        assert!(unit.abs() > 1.0e-3, "actor {actor} must animate: {drawn:?}");
        for step in &steps {
            assert!(
                (step / unit - 2.3).abs() < 1.0e-3,
                "actor {actor} jumps across a multi-tick frame: {steps:?}"
            );
        }
    }
}
