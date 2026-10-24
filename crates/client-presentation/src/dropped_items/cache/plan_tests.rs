//! Complete-plan selection against independently admitted artwork routes.
use super::*;
use crate::session_artwork::ItemArtwork;
use render_model::{DroppedItemCube, DroppedItemSprite, dropped_item::ItemModelBudget};

struct Artwork {
    model: DroppedItemModel,
    vertices: usize,
    builds: std::cell::Cell<usize>,
}

impl Artwork {
    /// Builds original test pixels and measures their actual downstream geometry.
    fn new(side: u32, cube: bool, checkerboard: bool) -> Self {
        let pixels: Arc<[u8]> = (0..side * side)
            .flat_map(|pixel| {
                let alpha = if checkerboard && (pixel / side + pixel % side).is_multiple_of(2) {
                    0
                } else {
                    255
                };
                [80, 120, 160, alpha]
            })
            .collect();
        let model = if cube {
            DroppedItemModel::Cube(DroppedItemCube {
                tile: side,
                faces: std::array::from_fn(|_| Arc::clone(&pixels)),
                tints: [render_model::OPAQUE_WHITE; 6],
            })
        } else {
            DroppedItemModel::NativeSprite(DroppedItemSprite {
                width: side,
                height: side,
                rgba8: pixels,
            })
        };
        let vertices = model
            .mesh(
                model
                    .atlas_side(assets::texture::policy::device_policy())
                    .unwrap(),
                1,
            )
            .unwrap()
            .len();
        Self {
            model,
            vertices,
            builds: std::cell::Cell::new(0),
        }
    }
}

/// Resolves each item's full source chain, including aliases sharing one model.
fn resolve(
    cache: &mut ModelCache,
    models: &[&Artwork],
    items: &[[Option<usize>; 4]],
) -> Vec<Option<u32>> {
    items
        .iter()
        .map(|routes| {
            cache.artwork(|cache, source| {
                let route = match source {
                    ItemArtwork::SessionCube => 0,
                    ItemArtwork::SessionSprite => 1,
                    ItemArtwork::StartupCube => 2,
                    ItemArtwork::StartupSprite => 3,
                };
                let id = routes[route]?;
                let model = &models[id].model;
                cache.model(
                    ModelKey::Block {
                        hashed: false,
                        id: id as u32,
                    },
                    || planning::ModelCost::from_model(model),
                    || {
                        models[id].builds.set(models[id].builds.get() + 1);
                        Some(model.clone())
                    },
                )
            })
        })
        .collect()
}

/// Counts one complete route plan without using the cache's planning or residency code.
fn oracle(
    mut budget: ItemModelBudget,
    models: &[&Artwork],
    items: &[[Option<usize>; 4]],
    sprites: bool,
) -> usize {
    budget.reset();
    let mut admitted = HashSet::<usize>::default();
    let mut pixels = 0;
    items
        .iter()
        .filter(|routes| {
            routes.iter().enumerate().any(|(route, id)| {
                if sprites && route % 2 == 0 {
                    return false;
                }
                let Some(id) = id else { return false };
                if admitted.contains(id) {
                    return true;
                }
                let artwork = models[*id];
                let bytes = pixel_cost(&artwork.model);
                if bytes > assets::texture::policy::MAX_SUBSCRIBER_BYTES - pixels
                    || budget.admit(&artwork.model, artwork.vertices).is_none()
                {
                    return false;
                }
                pixels += bytes;
                admitted.insert(*id);
                true
            })
        })
        .count()
}

/// Checks selection and upload together, including repeated preparations with resident artwork.
fn assert_best_plan(models: &[&Artwork], items: &[[Option<usize>; 4]], budget: ItemModelBudget) {
    let cubes = oracle(budget, models, items, false);
    let sprites = oracle(budget, models, items, true);
    let mut cache = ModelCache {
        budget,
        ..Default::default()
    };
    for _ in 0..2 {
        let chosen = cache.prepare(|cache| resolve(cache, models, items));
        let drawn = chosen.iter().flatten().count();
        assert_eq!(
            drawn,
            cubes.max(sprites),
            "cube plan {cubes}, sprite plan {sprites}"
        );
        assert_eq!(cache.cube_plan_downgrades, u64::from(sprites > cubes));
        assert_eq!(cache.prepared_items, drawn as u64);
        assert_eq!(cache.omitted_items, (items.len() - drawn) as u64);
        cache.finish_frame();
        let mut upload = budget;
        upload.reset();
        let mut seen = HashSet::<u32>::default();
        for slot in chosen.into_iter().flatten() {
            if seen.insert(slot) {
                let model = &cache.shared[slot as usize];
                let vertices = model
                    .mesh(model.atlas_side(upload.policy()).unwrap(), 1)
                    .unwrap();
                assert!(upload.admit(model, vertices.len()).is_some());
            }
        }
    }
}

#[test]
fn regression_43_checkerboard_blocks_keep_the_fully_drawable_cube_plan() {
    let cube = Artwork::new(16, true, false);
    let icon = Artwork::new(128, false, true);
    let models: Vec<_> = (0..43).flat_map(|_| [&cube, &icon]).collect();
    let items: Vec<_> = (0..43)
        .map(|id| [Some(id * 2), Some(id * 2 + 1), None, None])
        .collect();
    let budget = ItemModelBudget::new(assets::texture::policy::device_policy(), 256, u64::MAX);
    assert_eq!(oracle(budget, &models, &items, false), 43);
    assert_eq!(oracle(budget, &models, &items, true), 7);
    assert_best_plan(&models, &items, budget);
}

/// Advances a fixed-seed generator without adding a random dependency to the crate.
fn random(seed: &mut u64, limit: usize) -> usize {
    *seed ^= *seed << 13;
    *seed ^= *seed >> 7;
    *seed ^= *seed << 17;
    (*seed % limit as u64) as usize
}

#[test]
fn bounded_random_plans_draw_at_least_both_baselines_within_both_budgets() {
    let cubes = [16, 128, 512].map(|side| Artwork::new(side, true, false));
    let sprites = [1, 16, 128, 1024].map(|side| Artwork::new(side, false, false));
    let checkerboards = [8, 32, 128].map(|side| Artwork::new(side, false, true));
    let icons: Vec<_> = sprites.iter().chain(&checkerboards).collect();
    let mut seed = 0x2133_6a09_e667_f3bc;
    for _ in 0..96 {
        let count = 16 + random(&mut seed, 65);
        let mut models = Vec::new();
        let mut items = Vec::new();
        for _ in 0..count {
            if !items.is_empty() && random(&mut seed, 7) == 0 {
                items.push(items[random(&mut seed, items.len())]);
                continue;
            }
            let routes = std::array::from_fn(|route| {
                if random(&mut seed, 4) == 0 {
                    return None;
                }
                let artwork = if route % 2 == 0 {
                    &cubes[random(&mut seed, cubes.len())]
                } else {
                    icons[random(&mut seed, icons.len())]
                };
                models.push(artwork);
                Some(models.len() - 1)
            });
            items.push(routes);
        }
        let layers = [16, 64, 128, 256, MAX_ITEM_LAYERS][random(&mut seed, 5)];
        let mesh_bytes = [64 * 1024, 512 * 1024, 4 * 1024 * 1024, u64::MAX][random(&mut seed, 4)];
        let budget =
            ItemModelBudget::new(assets::texture::policy::device_policy(), layers, mesh_bytes);
        assert_best_plan(&models, &items, budget);
    }
}

#[test]
fn device_rejections_never_build_and_recheck_only_after_budget_changes() {
    let artwork = [
        Artwork::new(16, false, false),
        Artwork::new(16, false, false),
    ];
    let models: Vec<_> = artwork.iter().collect();
    let items = [[None, Some(0), None, None], [None, Some(1), None, None]].repeat(32);
    for layers in [true, false] {
        for art in &artwork {
            art.builds.set(0);
        }
        let budget = |expanded: bool| {
            ItemModelBudget::new(
                assets::texture::policy::device_policy(),
                if layers && !expanded {
                    2
                } else {
                    MAX_ITEM_LAYERS
                },
                if !layers && !expanded {
                    (artwork[0].vertices * render_model::dropped_item::ITEM_MESH_VERTEX_BYTES)
                        as u64
                } else {
                    u64::MAX
                },
            )
        };
        let mut cache = ModelCache {
            budget: budget(false),
            ..Default::default()
        };
        let rejected_key = ModelKey::Block {
            hashed: false,
            id: 1,
        };
        let mut checks = None;
        for _ in 0..24 {
            let chosen = cache.prepare(|cache| resolve(cache, &models, &items));
            assert_eq!(chosen.iter().flatten().count(), 32);
            cache.finish_frame();
            cache.warm_icon(
                rejected_key.clone(),
                || planning::ModelCost::from_model(&artwork[1].model),
                || {
                    artwork[1].builds.set(artwork[1].builds.get() + 1);
                    Some(artwork[1].model.clone())
                },
            );
            assert_eq!(artwork[0].builds.get(), 1, "admitted artwork is retained");
            assert_eq!(
                artwork[1].builds.get(),
                0,
                "rejected artwork is never built"
            );
            let current = cache.admission_checks[&rejected_key];
            assert_eq!(
                *checks.get_or_insert(current),
                current,
                "unchanged rejections reuse their budget decision"
            );
        }
        let checks_before_expansion = cache.admission_checks[&rejected_key];
        cache.budget = budget(true);
        for _ in 0..24 {
            let chosen = cache.prepare(|cache| resolve(cache, &models, &items));
            assert_eq!(chosen.iter().flatten().count(), items.len());
            cache.finish_frame();
            assert!(artwork.iter().all(|art| art.builds.get() == 1));
            assert!(cache.admission_checks[&rejected_key] > checks_before_expansion);
        }
    }
}

#[test]
fn rejected_source_is_reconsidered_after_competing_demand_or_artwork_changes() {
    let artwork = [
        Artwork::new(16, false, false),
        Artwork::new(16, false, false),
    ];
    let models: Vec<_> = artwork.iter().collect();
    let items = [[None, Some(0), None, None], [None, Some(1), None, None]];
    let budget = ItemModelBudget::new(assets::texture::policy::device_policy(), 2, u64::MAX);
    let mut cache = ModelCache {
        budget,
        ..Default::default()
    };
    for _ in 0..12 {
        assert_eq!(
            cache
                .prepare(|cache| resolve(cache, &models, &items))
                .iter()
                .flatten()
                .count(),
            1
        );
        cache.finish_frame();
    }
    assert_eq!(artwork[1].builds.get(), 0);
    for _ in 0..12 {
        assert!(cache.prepare(|cache| resolve(cache, &models, &items[1..]))[0].is_some());
        cache.finish_frame();
    }
    assert_eq!(artwork[1].builds.get(), 1, "changed demand frees capacity");
    cache.sync(0, None, 1);
    cache.budget = budget;
    assert!(cache.prepare(|cache| resolve(cache, &models, &items[1..]))[0].is_some());
    assert_eq!(
        artwork[1].builds.get(),
        2,
        "new artwork generation replaces old costs and pixels"
    );
}

#[test]
fn selected_residents_survive_a_new_model_before_them_in_demand_order() {
    let art = Artwork::new(1, false, false);
    let models: Vec<_> = (0..MAX_ITEM_LAYERS).map(|_| &art).collect();
    let mut items: Vec<_> = (0..MAX_ITEM_LAYERS - 1)
        .map(|id| [None, Some(id), None, None])
        .collect();
    let mut cache = ModelCache::default();
    assert!(
        cache
            .prepare(|cache| resolve(cache, &models, &items))
            .iter()
            .all(Option::is_some)
    );
    cache.finish_frame();
    let builds = cache.model_builds;
    items.pop();
    items.insert(0, [None, Some(MAX_ITEM_LAYERS - 1), None, None]);
    for _ in 0..12 {
        assert!(
            cache
                .prepare(|cache| resolve(cache, &models, &items))
                .iter()
                .all(Option::is_some)
        );
        cache.finish_frame();
        assert_eq!(
            cache.model_builds,
            builds + 1,
            "only the newly selected source is built"
        );
    }
}

#[test]
fn animation_warming_reuses_rejection_after_later_consumers_fill_the_budget() {
    let artwork = [
        Artwork::new(1, false, false),
        Artwork::new(16, false, false),
        Artwork::new(1, false, false),
    ];
    let models: Vec<_> = artwork.iter().collect();
    let items = [0, 1, 2].map(|id| [None, Some(id), None, None]);
    let budget = ItemModelBudget::new(
        assets::texture::policy::device_policy(),
        MAX_ITEM_LAYERS,
        ((artwork[0].vertices + artwork[2].vertices)
            * render_model::dropped_item::ITEM_MESH_VERTEX_BYTES) as u64,
    );
    let mut cache = ModelCache {
        budget,
        ..Default::default()
    };
    let key = ModelKey::Block {
        hashed: false,
        id: 1,
    };
    let mut checks = None;
    for frame in 0..24 {
        assert_eq!(
            cache
                .prepare(|cache| resolve(cache, &models, &items))
                .iter()
                .flatten()
                .count(),
            2
        );
        assert!(
            cache
                .warm_icon(
                    key.clone(),
                    || planning::ModelCost::from_model(&artwork[1].model),
                    || {
                        artwork[1].builds.set(artwork[1].builds.get() + 1);
                        Some(artwork[1].model.clone())
                    }
                )
                .is_none()
        );
        cache.finish_frame();
        let current = cache.admission_checks[&key];
        if frame > 0 {
            assert_eq!(
                *checks.get_or_insert(current),
                current,
                "unchanged later upload state reuses its warming refusal"
            );
        }
        assert_eq!(artwork[1].builds.get(), 0);
    }
    cache.budget = ItemModelBudget::new(
        assets::texture::policy::device_policy(),
        MAX_ITEM_LAYERS,
        u64::MAX,
    );
    let mut allowed_checks = None;
    for _ in 0..12 {
        cache.prepare(|cache| resolve(cache, &models, &[items[0], items[2]]));
        assert!(
            cache
                .warm_icon(
                    key.clone(),
                    || planning::ModelCost::from_model(&artwork[1].model),
                    || {
                        artwork[1].builds.set(artwork[1].builds.get() + 1);
                        Some(artwork[1].model.clone())
                    }
                )
                .is_some()
        );
        cache.finish_frame();
        let current = cache.admission_checks[&key];
        assert_eq!(*allowed_checks.get_or_insert(current), current);
        assert_eq!(artwork[1].builds.get(), 1);
    }
}
