//! Complete-plan selection against independently admitted artwork routes.
use super::*;
use crate::session_artwork::ItemArtwork;
use render_model::{DroppedItemCube, DroppedItemSprite, dropped_item::ItemModelBudget};

struct Artwork {
    model: DroppedItemModel,
    vertices: usize,
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
        Self { model, vertices }
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
                    || Some(model.texture_layers()),
                    || Some(model.clone()),
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
