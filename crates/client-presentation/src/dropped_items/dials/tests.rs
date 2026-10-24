use std::sync::Arc;

use super::*;
use crate::dropped_items::DroppedItemPublisher;
use assets::icon::dial::{DIAL_STRIPS, DialKind};
use render_model::DroppedItemModel;

/// Creates distinct sprites for every dial frame and one unrelated item without carriers.
fn icons() -> UiPresentationRuntime {
    let mut sprites = vec![assets::IconSprite {
        width: 1,
        height: 1,
        rgba8: vec![0, 71, 121, 255].into(),
    }];
    let mut entries = vec![assets::IconEntry {
        identifier: "test:ordinary".into(),
        metadata: 0,
        sprite: 0,
    }];
    for strip in DIAL_STRIPS {
        entries.push(assets::IconEntry {
            identifier: strip.item.into(),
            metadata: 0,
            sprite: 0,
        });
        let count = match strip.kind {
            DialKind::Compass | DialKind::RecoveryCompass => inventory::COMPASS_FRAMES,
            DialKind::Clock => inventory::CLOCK_FRAMES,
        };
        for frame in 0..count {
            let sprite = sprites.len() as u32;
            entries.push(assets::IconEntry {
                identifier: strip.frames_identifier.into(),
                metadata: frame,
                sprite,
            });
            sprites.push(assets::IconSprite {
                width: 1,
                height: 1,
                rgba8: vec![sprite as u8, 71, 121, 255].into(),
            });
        }
    }
    entries.sort_by(|a, b| (&a.identifier, a.metadata).cmp(&(&b.identifier, b.metadata)));
    let catalog = assets::RuntimeIconCatalog::decode(
        &assets::encode_icon_catalog([5; 32], &sprites, &entries).unwrap(),
    )
    .unwrap();
    UiPresentationRuntime::with_hud_and_icons(
        ui_engine::test_support::fixture_font(),
        client_ui::test_support::fixture_hud(),
        Arc::new(catalog),
    )
    .unwrap()
}

/// Resolves current models before warming all active strips, as the publisher does.
fn publish(cache: &mut ModelCache, icons: &UiPresentationRuntime, dials: bool) -> Vec<u32> {
    let mut animation = DialModels::default();
    let mut current = vec![("test:ordinary", 0)];
    if dials {
        for strip in DIAL_STRIPS {
            animation.observe(icons, strip.item, 0);
            current.push(icons.dial_icon_key(strip.item, 0).unwrap());
        }
    }
    cache.begin_frame();
    for &(identifier, metadata) in &current {
        cache.request(&ModelKey::icon(
            icons.item_icon(identifier, metadata).unwrap(),
            true,
        ));
    }
    cache.finish_demand();
    let handles = current
        .into_iter()
        .map(|(identifier, metadata)| {
            DroppedItemPublisher::icon_model(cache, icons, identifier, metadata, true).unwrap()
        })
        .collect();
    animation.warm(cache, icons);
    cache.finish_frame();
    handles
}

#[test]
fn every_dial_frame_changes_handles_without_republishing_scene_models() {
    let mut icons = icons();
    let mut cache = ModelCache::default();
    icons.set_dial_frames(inventory::DialFrames {
        compass: Some(0),
        recovery_compass: Some(0),
        clock: Some(0),
    });
    let first = publish(&mut cache, &icons, true);
    let revision = cache.revision;
    let shared = Arc::clone(&cache.shared);
    let resident_count = cache.models.len();
    let mut previous = first;
    for frame in 1..=inventory::CLOCK_FRAMES * 2 {
        icons.set_dial_frames(inventory::DialFrames {
            compass: Some(frame % inventory::COMPASS_FRAMES),
            recovery_compass: Some(frame % inventory::COMPASS_FRAMES),
            clock: Some(frame % inventory::CLOCK_FRAMES),
        });
        let current = publish(&mut cache, &icons, true);
        assert_eq!(current[0], previous[0], "unrelated item keeps its handle");
        for (&handle, &old) in current[1..].iter().zip(&previous[1..]) {
            assert_ne!(handle, old, "dial selects a different model");
            assert!(matches!(
                cache.shared[handle as usize],
                DroppedItemModel::NativeSprite(_)
            ));
        }
        assert_eq!(
            cache.revision, revision,
            "animation must not rebuild GPU models"
        );
        assert!(Arc::ptr_eq(&shared, &cache.shared));
        assert_eq!(cache.models.len(), resident_count);
        previous = current;
    }
}

#[test]
fn removed_dials_release_published_frames_and_reuse_cached_models_on_return() {
    let mut icons = icons();
    let mut cache = ModelCache::default();
    icons.set_dial_frames(inventory::DialFrames {
        compass: Some(0),
        recovery_compass: Some(0),
        clock: Some(0),
    });
    let handles = publish(&mut cache, &icons, true);
    let resident_count = cache.models.len();
    let ordinary = publish(&mut cache, &icons, false);
    assert_eq!(ordinary, handles[..1]);
    for (slot, model) in cache.shared.iter().enumerate() {
        assert_eq!(
            matches!(model, DroppedItemModel::Vacant),
            slot != ordinary[0] as usize
        );
    }
    let inactive_revision = cache.revision;
    publish(&mut cache, &icons, false);
    assert_eq!(cache.revision, inactive_revision);
    assert_eq!(publish(&mut cache, &icons, true), handles);
    assert_eq!(cache.models.len(), resident_count);
}

/// Publishes an HD item and a clock, optionally retaining clock models from earlier frames.
fn publish_hd_clock(
    cache: &mut ModelCache,
    icons: &UiPresentationRuntime,
    hd: &DroppedItemModel,
    hd_first: bool,
    max_array_layers: usize,
) -> (u32, u32) {
    cache.budget = render_model::dropped_item::ItemModelBudget::new(
        assets::texture::policy::device_policy(),
        max_array_layers,
        u64::MAX,
    );
    let hd_key = ModelKey::Block {
        hashed: false,
        id: u32::MAX,
    };
    let (identifier, metadata) = icons.dial_icon_key("minecraft:clock", 0).unwrap();
    let clock_key = ModelKey::icon(icons.item_icon(identifier, metadata).unwrap(), true);
    cache.begin_frame();
    cache.request(&hd_key);
    cache.request(&clock_key);
    cache.finish_demand();
    let admit_hd = |cache: &mut ModelCache| {
        cache
            .model(
                hd_key.clone(),
                || render_model::dropped_item::ItemModelCost::from_model(hd),
                || Some(hd.clone()),
            )
            .unwrap()
    };
    let admit_clock = |cache: &mut ModelCache| {
        DroppedItemPublisher::icon_model(cache, icons, identifier, metadata, true).unwrap()
    };
    let (hd_handle, clock_handle) = if hd_first {
        (admit_hd(cache), admit_clock(cache))
    } else {
        let clock = admit_clock(cache);
        (admit_hd(cache), clock)
    };
    let mut animation = DialModels::default();
    animation.observe(icons, "minecraft:clock", 0);
    animation.warm(cache, icons);
    cache.finish_frame();
    (hd_handle, clock_handle)
}

#[test]
fn hd_items_and_every_current_clock_frame_fit_with_either_residency_order() {
    let hd = DroppedItemModel::Sprite(render_model::DroppedItemSprite {
        width: 2048,
        height: 2048,
        rgba8: vec![255; assets::texture::policy::MAX_IMAGE_BYTES].into(),
    });
    for hd_first in [false, true] {
        for max_array_layers in [3, render_model::MAX_ITEM_LAYERS] {
            let mut cache = ModelCache::default();
            let mut icons = icons();
            icons.set_dial_frames(inventory::DialFrames {
                clock: Some(0),
                ..Default::default()
            });
            if !hd_first {
                cache.begin_frame();
                cache.finish_demand();
                let mut animation = DialModels::default();
                animation.observe(&icons, "minecraft:clock", 0);
                animation.warm(&mut cache, &icons);
                cache.finish_frame();
            }
            for frame in 0..inventory::CLOCK_FRAMES * 2 {
                icons.set_dial_frames(inventory::DialFrames {
                    clock: Some(frame % inventory::CLOCK_FRAMES),
                    ..Default::default()
                });
                let (identifier, metadata) = icons.dial_icon_key("minecraft:clock", 0).unwrap();
                let previous = Arc::clone(&cache.shared);
                let revision = cache.revision;
                let (hd_handle, clock_handle) =
                    publish_hd_clock(&mut cache, &icons, &hd, hd_first, max_array_layers);
                assert_eq!(
                    &cache.shared[hd_handle as usize], &hd,
                    "HD item remains published"
                );
                assert_eq!(
                    &cache.shared[clock_handle as usize],
                    &icon_sprite(&icons, identifier, metadata, true).unwrap(),
                    "current clock frame remains published"
                );
                if previous
                    .get(clock_handle as usize)
                    .is_none_or(|model| matches!(model, DroppedItemModel::Vacant))
                {
                    assert_ne!(
                        cache.revision, revision,
                        "newly visible cached frames trigger upload"
                    );
                }
                let policy = assets::texture::policy::device_policy();
                let side = cache
                    .shared
                    .iter()
                    .filter_map(|model| model.atlas_side(policy))
                    .max()
                    .unwrap();
                let layers = 1 + cache
                    .shared
                    .iter()
                    .map(DroppedItemModel::texture_layers)
                    .sum::<usize>();
                let bytes = policy.page_bytes(side, side).unwrap() * layers;
                assert!(bytes <= assets::texture::policy::MAX_SUBSCRIBER_BYTES);
                assert!(layers <= max_array_layers);
            }
        }
    }
}
