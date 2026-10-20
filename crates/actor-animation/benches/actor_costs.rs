use actor_animation::{test_support::CompositionFixture, *};
use assets::*;
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::{collections::HashMap, hint::black_box, sync::Arc, time::Duration};

#[path = "../src/test_actor.rs"]
mod test_actor;

/// Builds original player fixtures with independent idle, stride and attack inputs.
fn crowd(
    mode: &str,
    extra_variables: usize,
) -> (ActorAnimationStore, HashMap<u64, test_actor::TestActor>) {
    let mut compiled =
        test_support::counting_random_compiled(assets::facts::VanillaEntity::Player.identifier());
    compiled.rig_bindings[0].pre_animation = None;
    compiled.render.layers[0].color = None;
    compiled.animation_keyframes[0].expressions = if mode == "idle" {
        [None; 3]
    } else {
        [Some(3), None, None]
    };
    if mode == "swing" {
        compiled.molang_symbols[7].identifier = "variable.attack_time".into();
        compiled.molang_ops[11] = MolangOp::LoadVariable(7);
    } else if mode == "walk" {
        compiled.molang_symbols[4].identifier = "query.modified_distance_moved".into();
        compiled.molang_ops[11] = MolangOp::LoadQuery(4);
    }
    let mut symbols = compiled.molang_symbols.to_vec();
    symbols.extend((0..extra_variables).map(|slot| MolangSymbol {
        kind: MolangSymbolKind::Variable,
        identifier: format!("variable.zz_layout_{slot:04}").into(),
    }));
    compiled.molang_symbols = symbols.into();
    let seed = compiled.geometries[0].bones[0].clone();
    compiled.geometries[0].bones = (0..6)
        .map(|index| {
            let mut bone = seed.clone();
            if index > 0 {
                bone.name = format!("limb{index}").into();
                bone.parent = Some(seed.name.clone());
            }
            bone
        })
        .collect();
    let assets = Arc::new(
        RuntimeEntityAssets::from_compiled(compiled.with_unresolved_admissions()).unwrap(),
    );
    let mut store = ActorAnimationStore::with_assets(assets);
    let mut actors: HashMap<_, _> = (1..=128u64)
        .map(|id| {
            let actor = test_actor::TestActor {
                unique_id: id as i64,
                runtime_id: id,
                kind: protocol::ActorKind::Player {
                    uuid: (id as u128).to_le_bytes(),
                    username: "fixture".into(),
                },
                ..Default::default()
            };
            (actor.unique_id as u64, actor)
        })
        .collect();
    for actor in actors.values() {
        store.insert(1, 0, actor);
    }
    store.advance_tick(&actors, None, None, true, false, |_| {
        ActorTickContext::default()
    });
    for actor in actors.values_mut() {
        if mode == "walk" {
            actor.position[0] = 0.2;
            actor.velocity[0] = 4.0;
        }
        if mode == "swing" {
            store.start_swing(actor.runtime_id, 6, 6);
        }
    }
    store.advance_tick(&actors, None, None, true, false, |_| {
        ActorTickContext::default()
    });
    if mode == "swing" {
        store.advance_tick(&actors, None, None, true, false, |_| {
            ActorTickContext::default()
        });
    }
    if mode != "idle" {
        let mut frame = ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.5);
        assert_eq!(frame.layers_with_skin(1).unwrap().render[0].pose.len(), 6);
        assert_eq!(store.frame_fallbacks(), 0);
    }
    (store, actors)
}

/// Measures repeated and changing frame inputs separately for a 128-player crowd.
fn actor_costs(c: &mut Criterion) {
    let mut group = c.benchmark_group("actor_costs");
    group
        .sample_size(20)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(1));
    for extra_variables in [0, 256] {
        let suffix = if extra_variables == 0 {
            String::new()
        } else {
            format!("_variables_{extra_variables}")
        };
        for mode in ["idle", "walk", "swing"] {
            for repeated in [false, true] {
                let (store, actors) = crowd(mode, extra_variables);
                let mut alpha = 0.25;
                group.bench_function(
                    BenchmarkId::new(
                        format!(
                            "{}{}",
                            if repeated {
                                "repeat_128"
                            } else {
                                "changing_128"
                            },
                            suffix
                        ),
                        mode,
                    ),
                    |b| {
                        b.iter(|| {
                            if !repeated {
                                alpha = if alpha == 0.25 { 0.75 } else { 0.25 };
                            }
                            let mut frame =
                                ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], alpha);
                            for id in actors.keys() {
                                black_box(frame.layers_with_skin(*id).unwrap());
                            }
                        })
                    },
                );
            }
        }
        for mode in ["idle", "walk", "swing"] {
            let (mut store, mut actors) = crowd(mode, extra_variables);
            group.bench_function(
                BenchmarkId::new(format!("tick_and_six_frames_128{suffix}"), mode),
                |b| {
                    b.iter(|| {
                        for actor in actors.values_mut() {
                            if mode == "walk" {
                                actor.previous_position = actor.position;
                                actor.position[0] += 0.2;
                            }
                            if mode == "swing" {
                                store.start_swing(actor.runtime_id, 6, 6);
                            }
                        }
                        store.advance_tick(&actors, None, None, true, false, |_| {
                            ActorTickContext::default()
                        });
                        for alpha in [0.0, 0.16, 0.33, 0.5, 0.66, 0.83] {
                            let mut frame =
                                ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], alpha);
                            for id in actors.keys() {
                                black_box(frame.layers_with_skin(*id).unwrap());
                            }
                        }
                    })
                },
            );
        }
    }
    group.finish();
}
#[path = "actor_costs/frame_workers.rs"]
mod frame_workers;

/// Compares identical nonzero poses, including validation and excluding plan construction.
fn composition_costs(criterion: &mut Criterion) {
    let mut group = criterion.benchmark_group("actor_costs/composition");
    group
        .sample_size(30)
        .warm_up_time(Duration::from_millis(500))
        .measurement_time(Duration::from_secs(2));
    for count in [17, 64] {
        let mut fixture = CompositionFixture::new(count);
        assert_eq!(fixture.recursive(), fixture.flat());
        group.bench_with_input(BenchmarkId::new("recursive", count), &count, |bench, _| {
            bench.iter(|| black_box(fixture.recursive()));
        });
        group.bench_with_input(BenchmarkId::new("flat", count), &count, |bench, _| {
            bench.iter(|| {
                black_box(fixture.flat());
            });
        });
    }
    group.finish();
}

criterion_group!(
    benches,
    actor_costs,
    composition_costs,
    frame_workers::costs
);
criterion_main!(benches);
