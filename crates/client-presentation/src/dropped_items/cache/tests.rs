use super::*;
use crate::dropped_items::DroppedItemPublisher;
use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use render_model::{DroppedItemCube, DroppedItemSprite};

/// Creates a one-layer model without local carriers.
fn sprite() -> DroppedItemModel {
    DroppedItemModel::Sprite(DroppedItemSprite {
        width: 1,
        height: 1,
        rgba8: Arc::from([255; 4]),
    })
}

/// Makes an independent key for capacity tests.
fn key(id: u32) -> ModelKey {
    ModelKey::Block { hashed: false, id }
}

/// Admits one sprite through the same lazy construction path as the publisher.
fn admit(cache: &mut ModelCache, id: u32) -> Option<u32> {
    cache.model(key(id), || Some(1), || Some(sprite()))
}

/// Starts a frame and protects its current consumers before calculating capacity.
fn frame(cache: &mut ModelCache, ids: impl IntoIterator<Item = u32>) {
    cache.begin_frame();
    for id in ids {
        cache.request(&key(id));
    }
    cache.finish_demand();
}

#[test]
fn atlas_reclaims_despawned_models_and_keeps_live_handles() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    for id in 0..(MAX_ITEM_LAYERS - 1) as u32 {
        assert_eq!(admit(&mut cache, id), Some(id));
    }
    let delayed = MAX_ITEM_LAYERS as u32;
    assert_eq!(admit(&mut cache, delayed), None);
    frame(&mut cache, [0, 100, delayed]);
    let live = [admit(&mut cache, 0), admit(&mut cache, 100)];
    assert!(admit(&mut cache, delayed).is_some());
    assert_eq!(live, [Some(0), Some(100)]);
    assert_eq!(admit(&mut cache, 100), Some(100));
    assert!(cache.layers < MAX_ITEM_LAYERS);
    cache.finish_frame(
        assets::texture::policy::device_policy(),
        render_model::MAX_ITEM_LAYERS,
    );
    assert_eq!(cache.shared.len(), MAX_ITEM_LAYERS - 1);
    assert!(!matches!(cache.shared[100], DroppedItemModel::Vacant));
}

#[test]
fn inactive_hd_models_cannot_size_the_current_atlas() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    let hd = cache
        .model(
            key(0),
            || Some(1),
            || {
                Some(DroppedItemModel::Sprite(DroppedItemSprite {
                    width: 2048,
                    height: 2048,
                    rgba8: vec![255; 2048 * 2048 * 4].into(),
                }))
            },
        )
        .unwrap();
    cache.finish_frame(
        assets::texture::policy::device_policy(),
        render_model::MAX_ITEM_LAYERS,
    );
    let first_revision = cache.revision;
    frame(&mut cache, 1..=8);
    let ordinary = (1..=8)
        .map(|id| {
            cache
                .model(
                    key(id),
                    || Some(1),
                    || {
                        Some(DroppedItemModel::Sprite(DroppedItemSprite {
                            width: 16,
                            height: 16,
                            rgba8: vec![73; 16 * 16 * 4].into(),
                        }))
                    },
                )
                .unwrap()
        })
        .collect::<Vec<_>>();
    cache.finish_frame(
        assets::texture::policy::device_policy(),
        render_model::MAX_ITEM_LAYERS,
    );
    assert!(matches!(
        cache.shared[hd as usize],
        DroppedItemModel::Vacant
    ));
    assert!(
        matches!(cache.models[hd as usize], DroppedItemModel::Sprite(_)),
        "inactive source stays cached"
    );
    for slot in ordinary {
        let DroppedItemModel::Sprite(sprite) = &cache.shared[slot as usize] else {
            panic!("current item remains published");
        };
        assert_eq!([sprite.width, sprite.height], [16; 2]);
    }
    assert!(cache.revision > first_revision);
    let shared = Arc::clone(&cache.shared);
    frame(&mut cache, 1..=8);
    cache.finish_frame(
        assets::texture::policy::device_policy(),
        render_model::MAX_ITEM_LAYERS,
    );
    assert!(Arc::ptr_eq(&shared, &cache.shared));
    frame(&mut cache, [0, 1]);
    assert_eq!(
        cache.model(
            key(0),
            || panic!("reuse cached cost"),
            || panic!("reuse cached model")
        ),
        Some(hd)
    );
    cache.finish_frame(
        assets::texture::policy::device_policy(),
        render_model::MAX_ITEM_LAYERS,
    );
    assert!(matches!(
        cache.shared[hd as usize],
        DroppedItemModel::Sprite(_)
    ));
}

#[test]
fn evicted_cpu_pixels_are_released_and_returning_models_are_rebuilt() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    for id in 0..(MAX_ITEM_LAYERS - 1) as u32 {
        admit(&mut cache, id);
    }
    let DroppedItemModel::Sprite(old) = &cache.models[0] else {
        panic!("sprite");
    };
    let old_pixels = Arc::downgrade(&old.rgba8);
    cache.finish_frame(
        assets::texture::policy::device_policy(),
        render_model::MAX_ITEM_LAYERS,
    );
    frame(&mut cache, []);
    assert_eq!(admit(&mut cache, 900), Some(0));
    cache.finish_frame(
        assets::texture::policy::device_policy(),
        render_model::MAX_ITEM_LAYERS,
    );
    assert!(
        old_pixels.upgrade().is_none(),
        "eviction releases CPU pixels"
    );
    frame(&mut cache, []);
    let slot = admit(&mut cache, 0).expect("a returning model is rebuilt");
    assert!(matches!(
        cache.models[slot as usize],
        DroppedItemModel::Sprite(_)
    ));
}

#[test]
fn unchanged_full_pressure_does_bounded_work_after_long_history() {
    let mut cache = ModelCache::default();
    let resident_count = (MAX_ITEM_LAYERS - 1) as u32;
    // Accumulate session history with many completely replaced cohorts.
    for first in (0..60_000).step_by(resident_count as usize) {
        frame(&mut cache, []);
        for id in first..first + resident_count {
            admit(&mut cache, id).unwrap();
        }
    }
    cache.finish_frame(
        assets::texture::policy::device_policy(),
        render_model::MAX_ITEM_LAYERS,
    );
    let live: Vec<_> = cache.index.keys().cloned().collect();
    for _ in 0..3 {
        let before = cache.inspections;
        cache.begin_frame();
        for key in &live {
            cache.request(key);
        }
        cache.finish_demand();
        let revision = cache.revision;
        for occurrence in 0..8_000 {
            assert_eq!(
                cache.model(
                    key(100_000 + occurrence % 20),
                    || panic!("no cost work under full pressure"),
                    || panic!("rejected models must not be built")
                ),
                None
            );
        }
        cache.finish_frame(
            assets::texture::policy::device_policy(),
            render_model::MAX_ITEM_LAYERS,
        );
        assert_eq!(cache.inspections - before, MAX_ITEM_LAYERS - 1);
        assert_eq!(cache.index.len(), MAX_ITEM_LAYERS - 1);
        assert_eq!(cache.residents.len(), MAX_ITEM_LAYERS - 1);
        assert_eq!(cache.revision, revision);
    }
}

#[test]
fn cube_admission_reclaims_six_layers_without_moving_a_live_sprite() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    for id in 0..(MAX_ITEM_LAYERS - 1) as u32 {
        admit(&mut cache, id);
    }
    frame(&mut cache, [50]);
    assert!(
        cache
            .model(
                key(900),
                || Some(assets::BlockFace::ALL.len()),
                || Some(DroppedItemModel::Cube(DroppedItemCube {
                    tile: 1,
                    faces: std::array::from_fn(|_| Arc::from([255; 4])),
                    tints: [0; 6],
                }))
            )
            .is_some()
    );
    assert_eq!(admit(&mut cache, 50), Some(50));
    assert_eq!(cache.layers, MAX_ITEM_LAYERS - 1);
    assert_eq!(
        cache
            .models
            .iter()
            .filter(|model| matches!(model, DroppedItemModel::Vacant))
            .count(),
        5
    );
}

#[test]
fn insufficient_mixed_capacity_skips_construction_and_retries_after_despawn() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    let live_count = (MAX_ITEM_LAYERS - 4) as u32;
    for id in 0..live_count {
        admit(&mut cache, id);
    }
    let cost_queries = std::cell::Cell::new(0);
    for _ in 0..3 {
        frame(&mut cache, (0..live_count).chain([900]));
        let before = cache.inspections;
        for _ in 0..100 {
            assert_eq!(
                cache.model(
                    key(900),
                    || {
                        cost_queries.set(cost_queries.get() + 1);
                        Some(assets::BlockFace::ALL.len())
                    },
                    || panic!("three layers cannot fit a cube")
                ),
                None
            );
        }
        assert_eq!(cache.inspections, before);
        assert_eq!(
            cost_queries.get(),
            1,
            "unchanged rejected models reuse their layer estimate"
        );
    }
    frame(&mut cache, (3..live_count).chain([900]));
    assert!(
        cache
            .model(
                key(900),
                || Some(assets::BlockFace::ALL.len()),
                || Some(DroppedItemModel::Cube(DroppedItemCube {
                    tile: 1,
                    faces: std::array::from_fn(|_| Arc::from([255; 4])),
                    tints: [0; 6],
                }))
            )
            .is_some()
    );
}

#[test]
fn late_requests_protect_a_previously_reclaimable_slot() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    for id in 0..(MAX_ITEM_LAYERS - 1) as u32 {
        admit(&mut cache, id);
    }
    frame(&mut cache, []);
    assert_eq!(admit(&mut cache, 0), Some(0));
    assert_eq!(admit(&mut cache, 900), Some(1));
    assert_eq!(admit(&mut cache, 0), Some(0));
}

#[test]
fn rejected_layer_estimates_stay_bounded_and_drop_unused_keys() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    for id in 0..60_000 {
        assert_eq!(
            cache.model(
                key(id),
                || Some(MAX_ITEM_LAYERS),
                || panic!("oversized models must not be built")
            ),
            None
        );
    }
    assert_eq!(cache.rejected.len(), MAX_ITEM_LAYERS - 1);
    assert!(cache.models.is_empty() && cache.index.is_empty());
    frame(&mut cache, 0..(MAX_ITEM_LAYERS - 1) as u32);
    assert_eq!(
        cache.model(
            key(0),
            || panic!("demanded rejection reuses its layer estimate"),
            || panic!("oversized model")
        ),
        None
    );
    frame(&mut cache, []);
    assert!(cache.rejected.is_empty());
}

#[test]
fn warm_icon_lookup_and_admission_do_not_allocate() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    let key = ModelKey::icon(
        ui::IconRef {
            color_mask: None,
            page: 0,
            uv: [0, 0, 1, 1],
            glint: false,
        },
        true,
    );
    assert_eq!(
        cache.model(key.clone(), || Some(1), || Some(sprite())),
        Some(0)
    );
    cache.finish_frame(
        assets::texture::policy::device_policy(),
        render_model::MAX_ITEM_LAYERS,
    );
    let before = test_alloc::thread_allocations();
    for _ in 0..100 {
        cache.request(&key);
        assert_eq!(
            cache.model(
                key.clone(),
                || panic!("warm cost"),
                || panic!("warm geometry")
            ),
            Some(0)
        );
    }
    assert_eq!(test_alloc::thread_allocations() - before, 0);
}

#[test]
fn new_session_icons_drop_cached_sprite_models() {
    let mut cache = ModelCache::default();
    cache.sync(1, None, 0);
    frame(&mut cache, []);
    admit(&mut cache, 1);
    cache.sync(1, None, 0);
    assert!(cache.index.contains_key(&key(1)));
    cache.sync(1, None, 1);
    assert!(cache.index.is_empty() && cache.models.is_empty());
}

#[test]
fn permanent_block_build_failures_are_attempted_once_until_sync_changes() {
    let mut cache = ModelCache::default();
    let costs = std::cell::Cell::new(0);
    let builds = std::cell::Cell::new(0);
    let assets = Arc::new(assets::RuntimeAssets::diagnostic());
    let first = render_scene::ChunkTextureAssets::with_revision(Arc::clone(&assets), 1).identity();
    let second = render_scene::ChunkTextureAssets::with_revision(assets, 2).identity();
    let generations = [
        (1, first, 0),
        (1, second, 0),
        (1, second, 1),
        (2, second, 1),
    ];
    for (generation, (session, assets, icons)) in generations.into_iter().enumerate() {
        for _ in 0..3 {
            cache.sync(session, Some(assets), icons);
            frame(&mut cache, [900]);
            for _ in 0..100 {
                assert_eq!(
                    cache.model(
                        key(900),
                        || {
                            costs.set(costs.get() + 1);
                            Some(1)
                        },
                        || {
                            builds.set(builds.get() + 1);
                            None
                        }
                    ),
                    None
                );
            }
        }
        assert_eq!(costs.get(), generation + 1);
        assert_eq!(builds.get(), generation + 1);
        assert!(cache.models.is_empty());
    }
}

/// Unknown block IDs or unsupported visual kinds are estimated once, not on every frame.
#[test]
fn blocks_without_a_layer_estimate_are_estimated_once() {
    let mut cache = ModelCache::default();
    let costs = std::cell::Cell::new(0);
    for _ in 0..3 {
        frame(&mut cache, [901]);
        for _ in 0..100 {
            let estimate = || {
                costs.set(costs.get() + 1);
                None
            };
            assert_eq!(cache.model(key(901), estimate, || Some(sprite())), None);
        }
    }
    assert_eq!(costs.get(), 1);
    assert!(cache.models.is_empty());
}

#[test]
fn permanent_block_failures_are_bounded_and_follow_demand() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    for id in 0..60_000 {
        assert_eq!(cache.model(key(id), || Some(1), || None), None);
    }
    assert_eq!(cache.rejected.len(), MAX_ITEM_LAYERS - 1);
    assert!(cache.models.is_empty() && cache.index.is_empty());
    frame(&mut cache, 0..(MAX_ITEM_LAYERS - 1) as u32);
    assert_eq!(
        cache.model(
            key(0),
            || panic!("known block failure skips layer counting"),
            || panic!("known block failure skips construction")
        ),
        None
    );
    frame(&mut cache, []);
    assert!(cache.rejected.is_empty());
    let builds = std::cell::Cell::new(0);
    assert_eq!(
        cache.model(
            key(0),
            || Some(1),
            || {
                builds.set(builds.get() + 1);
                None
            }
        ),
        None
    );
    assert_eq!(builds.get(), 1, "unused negative entries are forgotten");
}

#[test]
fn transient_icon_and_carried_block_build_failures_are_retried() {
    for key in [
        ModelKey::icon(
            ui::IconRef {
                color_mask: None,
                page: 0,
                uv: [0, 0, 1, 1],
                glint: false,
            },
            true,
        ),
        ModelKey::CarriedBlock {
            hashed: false,
            id: 900,
        },
    ] {
        let mut cache = ModelCache::default();
        for succeeds in [false, true] {
            cache.begin_frame();
            cache.request(&key);
            cache.finish_demand();
            let result = cache.model(key.clone(), || Some(1), || succeeds.then(sprite));
            assert_eq!(result, succeeds.then_some(0));
        }
    }
}

/// Installs two synthetic icon variants and an alias, without local game assets.
fn icons() -> UiPresentationRuntime {
    use assets::{IconEntry, IconSprite, RuntimeIconCatalog, encode_icon_catalog};
    let sprites = [
        IconSprite {
            width: 32,
            height: 32,
            rgba8: vec![255; 32 * 32 * 4].into(),
        },
        IconSprite {
            width: 32,
            height: 32,
            rgba8: vec![40; 32 * 32 * 4].into(),
        },
    ];
    let mut entries = [
        IconEntry {
            identifier: "test:sprite".into(),
            metadata: 0,
            sprite: 0,
        },
        IconEntry {
            identifier: "test:sprite".into(),
            metadata: 7,
            sprite: 1,
        },
        IconEntry {
            identifier: "test:alias".into(),
            metadata: 0,
            sprite: 0,
        },
    ];
    entries.sort_by(|a, b| (&a.identifier, a.metadata).cmp(&(&b.identifier, b.metadata)));
    let catalog =
        RuntimeIconCatalog::decode(&encode_icon_catalog([5; 32], &sprites, &entries).unwrap())
            .unwrap();
    UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        client_ui::test_support::fixture_hud(),
        Arc::new(catalog),
    )
    .unwrap()
}

#[test]
fn cpu_retention_is_bounded_across_many_metadata_keys() {
    let icons = icons();
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    let base =
        DroppedItemPublisher::icon_model(&mut cache, &icons, "test:sprite", 0, true).unwrap();
    let variant =
        DroppedItemPublisher::icon_model(&mut cache, &icons, "test:sprite", 7, true).unwrap();
    assert_ne!(base, variant);
    for metadata in 8..60_008 {
        assert_eq!(
            DroppedItemPublisher::icon_model(&mut cache, &icons, "test:sprite", metadata, true),
            Some(base)
        );
    }
    assert_eq!(
        DroppedItemPublisher::icon_model(&mut cache, &icons, "test:alias", 0, true),
        Some(base)
    );
    let placed =
        DroppedItemPublisher::icon_model(&mut cache, &icons, "test:sprite", 0, false).unwrap();
    assert_ne!(
        base, placed,
        "native and static geometry use distinct placement routes"
    );
    assert_eq!(cache.index.len(), 3);
    assert_eq!(cache.layers, 3);
    let retained_bytes: usize = cache
        .models
        .iter()
        .map(|model| match model {
            DroppedItemModel::Sprite(sprite) | DroppedItemModel::NativeSprite(sprite) => {
                sprite.rgba8.len()
            }
            _ => panic!("sprite"),
        })
        .sum();
    assert_eq!(retained_bytes, 3 * 32 * 32 * 4);
}

#[test]
fn rejected_icons_retain_no_pixels_or_request_keys() {
    let icons = icons();
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    for id in 0..(MAX_ITEM_LAYERS - 1) as u32 {
        admit(&mut cache, id);
    }
    for metadata in 0..60_000 {
        assert_eq!(
            DroppedItemPublisher::icon_model(&mut cache, &icons, "test:sprite", metadata, true),
            None
        );
    }
    assert_eq!(cache.index.len(), MAX_ITEM_LAYERS - 1);
    assert!(
        cache
            .index
            .keys()
            .all(|key| matches!(key, ModelKey::Block { .. }))
    );
}

#[test]
fn hd_pixel_budget_reclaims_unused_models_and_preserves_current_consumers() {
    let pixels: Arc<[u8]> = vec![255; assets::texture::policy::MAX_IMAGE_BYTES].into();
    let build = || {
        Some(DroppedItemModel::Sprite(DroppedItemSprite {
            width: 2048,
            height: 2048,
            rgba8: Arc::clone(&pixels),
        }))
    };
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    for id in 0..4 {
        assert!(cache.model(key(id), || Some(1), build).is_some());
    }
    frame(&mut cache, [0, 1, 2, 3, 4]);
    assert!(cache.model(key(4), || Some(1), build).is_none());
    frame(&mut cache, [0, 4]);
    assert!(cache.model(key(4), || Some(1), build).is_some());
    assert!(cache.index.contains_key(&key(0)));
    assert_eq!(
        cache.models.iter().map(pixel_cost).sum::<usize>(),
        assets::texture::policy::MAX_SUBSCRIBER_BYTES
    );
}

#[test]
fn byte_budget_rejections_skip_builds_until_capacity_changes() {
    let face: Arc<[u8]> = vec![255; 1024 * 1024 * 4].into();
    let cube = || {
        Some(DroppedItemModel::Cube(DroppedItemCube {
            tile: 1024,
            faces: std::array::from_fn(|_| Arc::clone(&face)),
            tints: [render_model::OPAQUE_WHITE; 6],
        }))
    };
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    let first = cache.model(key(0), || Some(6), cube).unwrap();
    let second = cache.model(key(1), || Some(6), cube).unwrap();
    cache.finish_frame(
        assets::texture::policy::device_policy(),
        render_model::MAX_ITEM_LAYERS,
    );
    let shared = Arc::clone(&cache.shared);
    let costs = std::cell::Cell::new(0);
    let builds = std::cell::Cell::new(0);
    for _ in 0..3 {
        frame(&mut cache, [0, 1, 2]);
        for _ in 0..100 {
            assert_eq!(
                cache.model(
                    key(2),
                    || {
                        costs.set(costs.get() + 1);
                        Some(6)
                    },
                    || {
                        builds.set(builds.get() + 1);
                        cube()
                    },
                ),
                None
            );
        }
        cache.finish_frame(
            assets::texture::policy::device_policy(),
            render_model::MAX_ITEM_LAYERS,
        );
        assert!(Arc::ptr_eq(&shared, &cache.shared));
        assert_eq!(cache.index[&key(0)], first as usize);
        assert_eq!(cache.index[&key(1)], second as usize);
    }
    assert_eq!(costs.get(), 1);
    assert_eq!(builds.get(), 1);
    frame(&mut cache, [1, 2]);
    cache.request(&key(0));
    assert!(
        cache
            .model(
                key(2),
                || panic!("cached cost"),
                || panic!("late demand protects bytes")
            )
            .is_none()
    );
    frame(&mut cache, [0, 2]);
    assert!(
        cache
            .model(key(2), || panic!("cached cost"), cube)
            .is_some()
    );
    assert_eq!(cache.index[&key(0)], first as usize);
    assert!(!cache.index.contains_key(&key(1)));
}

#[test]
fn speculative_icon_frames_do_not_evict_models_at_the_layer_limit() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    for id in 0..(MAX_ITEM_LAYERS - 1) as u32 {
        admit(&mut cache, id).unwrap();
    }
    frame(&mut cache, [0]);
    let icon = ModelKey::Icon {
        page: 0,
        uv: [0, 0, 1, 1],
        native: true,
    };
    assert_eq!(cache.warm_icon(icon, || panic!("no free layers")), None);
    assert_eq!(cache.index.len(), MAX_ITEM_LAYERS - 1);
    assert_eq!(cache.index[&key(0)], 0);
    assert!(
        cache.index.contains_key(&key(1)),
        "inactive models remain cached"
    );
    assert!(
        admit(&mut cache, u32::MAX).is_some(),
        "current items can still reclaim"
    );
    assert_eq!(cache.index[&key(0)], 0);
}

#[test]
fn speculative_icon_frames_do_not_evict_models_at_the_pixel_limit() {
    let pixels: Arc<[u8]> = vec![255; assets::texture::policy::MAX_IMAGE_BYTES].into();
    let build = || {
        Some(DroppedItemModel::Sprite(DroppedItemSprite {
            width: 2048,
            height: 2048,
            rgba8: Arc::clone(&pixels),
        }))
    };
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    let count = assets::texture::policy::MAX_SUBSCRIBER_BYTES / pixels.len();
    for id in 0..count as u32 {
        cache.model(key(id), || Some(1), build).unwrap();
    }
    frame(&mut cache, [0]);
    let icon = ModelKey::Icon {
        page: 0,
        uv: [0, 0, 1, 1],
        native: true,
    };
    assert_eq!(cache.warm_icon(icon, || Some(sprite())), None);
    assert_eq!(cache.index.len(), count);
    assert_eq!(
        cache.pixel_bytes,
        assets::texture::policy::MAX_SUBSCRIBER_BYTES
    );
    assert!(
        admit(&mut cache, u32::MAX).is_some(),
        "current items can still reclaim"
    );
    assert_eq!(cache.index[&key(0)], 0);
}

#[test]
fn late_current_models_can_reclaim_speculative_frames_without_double_charging() {
    let mut cache = ModelCache::default();
    frame(&mut cache, []);
    for id in 0..(MAX_ITEM_LAYERS - 1) as u32 {
        cache.warm_icon(key(id), || Some(sprite())).unwrap();
    }
    let selected = cache.model(
        key(0),
        || panic!("cached frame"),
        || panic!("cached pixels"),
    );
    assert_eq!(selected, Some(0));
    assert_eq!(cache.available, MAX_ITEM_LAYERS - 2);
    let current = admit(&mut cache, u32::MAX).unwrap();
    assert_ne!(current, 0);
    assert_eq!(cache.available, MAX_ITEM_LAYERS - 3);
    cache.finish_frame(assets::texture::policy::device_policy(), MAX_ITEM_LAYERS);
    assert!(!matches!(cache.shared[0], DroppedItemModel::Vacant));
    assert!(!matches!(
        cache.shared[current as usize],
        DroppedItemModel::Vacant
    ));
}
