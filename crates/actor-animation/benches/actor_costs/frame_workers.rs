//! Changing-fraction crowd costs using an optional locally installed entity carrier.
use super::*;

/// Loads an installed carrier; benchmark sources contain no game assets.
#[expect(
    clippy::print_stderr,
    reason = "Reports the benchmark's named missing-fixture skip."
)]
fn installed_assets() -> Option<Arc<RuntimeEntityAssets>> {
    let path = std::env::var_os("CINNABAR_ENTITY_CARRIER")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../.local/assets/compiled/vanilla-v1.mcbeent")
        });
    match std::fs::read(&path) {
        Ok(bytes) => Some(Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("missing-fixture: frame worker benchmarks need an installed entity carrier");
            None
        }
        Err(error) => panic!("cannot read entity carrier {}: {error}", path.display()),
    }
}

/// Creates an original 64-bone skin retaining the installed driver's bone names.
fn custom_skin(names: &[Box<str>]) -> Arc<render_api::SkinGeometrySource> {
    let bones = (0..64).map(|index| {
        let name = names.get(index).map_or_else(|| format!("extra{index}"), ToString::to_string);
        let parent = if index == 0 { String::new() } else { format!(",\"parent\":\"{}\"", names[0]) };
        format!(r#"{{"name":"{name}"{parent},"pivot":[0,12,0],"cubes":[{{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0]}}]}}"#)
    }).collect::<Vec<_>>().join(",");
    actor_animation::test_support::skin_source(&format!(
        r#"{{"format_version":"1.12.0","minecraft:geometry":[{{"description":{{"identifier":"geometry.skin_cache","texture_width":64,"texture_height":64}},"bones":[{bones}]}}]}}"#
    ))
}

/// Builds independent moving actors with nonzero remote swing endpoints.
fn crowd(
    assets: Arc<RuntimeEntityAssets>,
    count: u64,
    mode: &str,
) -> (
    ActorAnimationStore,
    HashMap<u64, test_actor::TestActor>,
    Vec<u64>,
) {
    let entity = assets
        .symbols()
        .iter()
        .find(|symbol| {
            symbol.kind == EntityAssetKind::Entity
                && symbol.identifier.rsplit(':').next() == Some(mode)
        })
        .map(|symbol| Arc::<str>::from(&*symbol.identifier));
    let mut store = ActorAnimationStore::with_assets(assets);
    store.set_world_budget(usize::MAX / 4);
    let mut actors: HashMap<_, _> = (1..=count)
        .map(|id| {
            (
                id,
                test_actor::TestActor {
                    runtime_id: id,
                    unique_id: id as i64,
                    on_ground: Some(true),
                    kind: if mode == "pig" {
                        protocol::ActorKind::Entity {
                            identifier: entity.clone().expect("installed crowd entity"),
                        }
                    } else {
                        protocol::ActorKind::Player {
                            uuid: (id as u128).to_le_bytes(),
                            username: "fixture".into(),
                        }
                    },
                    ..Default::default()
                },
            )
        })
        .collect();
    let ids: Vec<_> = (1..=count).collect();
    for &id in &ids {
        store.insert(1, 0, &actors[&id]);
    }
    let skin = (mode == "custom").then(|| custom_skin(store.get(1).unwrap().bone_names));
    if let Some(skin) = &skin {
        store.request_skin_preparation(skin);
        store.submit_skin_preparation();
        store.finish_skin_fixture_batch();
        store.begin_skin_preparation();
        for &id in &ids {
            store.sync_skin_model(id, Some(skin));
        }
    }
    for tick in 0..9 {
        for &id in &ids {
            let actor = actors.get_mut(&id).unwrap();
            actor.previous_position = actor.position;
            actor.position[0] += 0.2;
            actor.velocity[0] = 4.0;
            actor.status.native_velocity[0] = 0.2;
            if mode != "pig" && tick % 6 == 0 {
                store.start_swing(id, 6, 6);
            }
        }
        store.advance_tick(&actors, None, None, true, false, |_| ActorTickContext {
            skin_geometry: skin.clone(),
            ..Default::default()
        });
    }
    for &id in &ids {
        assert!(store.get(id).unwrap().pose_evaluated);
        if mode != "pig" {
            assert!(
                !ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], 0.25)
                    .samples_during_admission(id),
                "crowd must use the production body batch"
            );
        }
    }
    (store, actors, ids)
}

/// Measures the production reservation path against ordered serial consumers with no fallback.
pub(super) fn costs(c: &mut Criterion) {
    let Some(assets) = installed_assets() else {
        return;
    };
    let mut group = c.benchmark_group("frame_workers");
    group
        .sample_size(20)
        .warm_up_time(Duration::from_millis(300))
        .measurement_time(Duration::from_secs(1));
    for mode in ["player", "custom", "pig"] {
        for count in [1, 4, 8, 16, 32, 128, 512] {
            let (store, actors, ids) = crowd(Arc::clone(&assets), count, mode);
            for workers in [1, 2, 4] {
                let pool = rayon::ThreadPoolBuilder::new()
                    .num_threads(workers)
                    .build()
                    .unwrap();
                let mut alpha = 0.25;
                group.bench_function(BenchmarkId::new(format!("{mode}_{workers}"), count), |b| {
                    b.iter(|| {
                        pool.install(|| {
                            alpha = if alpha == 0.25 { 0.75 } else { 0.25 };
                            let mut frame =
                                ActorRenderFrame::new(&store, &actors, [0.0; 2], [0.0; 3], alpha);
                            frame.set_remaining_ops(usize::MAX / 4);
                            if workers != 1 {
                                frame.sample_ordered(&ids);
                            }
                            for &id in &ids {
                                black_box(frame.layers_with_skin(id).unwrap());
                            }
                        })
                    })
                });
                assert_eq!(store.frame_fallbacks(), 0);
            }
        }
    }
    group.finish();
}
