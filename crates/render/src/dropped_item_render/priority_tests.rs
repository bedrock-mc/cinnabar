//! Current dropped models keep both texture and vertex capacity ahead of cached dial frames.

use super::upload_tests::item_world;
use super::*;
use bevy::ecs::system::RunSystemOnce;
use render_model::{DroppedItemInstance, DroppedItemSprite, MAX_ITEM_LAYERS};
use std::sync::Arc;

/// Makes a current scene instance without animation or local assets.
fn instance(model: u32) -> DroppedItemInstance {
    DroppedItemInstance {
        model,
        world_from_item: render_model::dropped_item_transform([0.0; 3], 0.0, 1.0),
        block_level: 15,
        sky_level: 15,
        overlay_rgba8: 0,
        glint: false,
    }
}

/// Builds distinct synthetic dial frames with expensive checkerboard-alpha geometry.
fn detailed_frames(count: usize, side: u32) -> Vec<DroppedItemModel> {
    (0..count)
        .map(|frame| {
            DroppedItemModel::NativeSprite(DroppedItemSprite {
                width: side,
                height: side,
                rgba8: (0..side * side)
                    .flat_map(|texel| {
                        [
                            frame as u8,
                            73,
                            91,
                            if (texel % side + texel / side).is_multiple_of(2) {
                                255
                            } else {
                                0
                            },
                        ]
                    })
                    .collect::<Vec<_>>()
                    .into(),
            })
        })
        .collect()
}

#[test]
fn detailed_cached_frames_cannot_displace_current_items_and_retry_when_selected() {
    let mut models = detailed_frames(128, 32);
    let ordinary = models.len() as u32;
    models.push(DroppedItemModel::Sprite(DroppedItemSprite {
        width: 1,
        height: 1,
        rgba8: Arc::from([255; 4]),
    }));
    let models: Arc<[DroppedItemModel]> = models.into();
    let mut world = item_world();
    world.resource_mut::<DroppedItemScene>().publish(
        1,
        Arc::clone(&models),
        &[instance(ordinary - 1), instance(ordinary)],
        &[],
        1.0,
    );
    world.run_system_once(prepare_items).unwrap();
    let withheld = {
        let gpu = world.resource::<ItemGpu>();
        assert_eq!(gpu.draws.len(), 2);
        assert!(!gpu.ranges[(ordinary - 1) as usize].vertices.is_empty());
        assert!(!gpu.ranges[ordinary as usize].vertices.is_empty());
        assert!(
            gpu.mesh_buffer.as_ref().unwrap().size()
                <= assets::texture::policy::MAX_SUBSCRIBER_BYTES as u64
        );
        gpu.ranges[..ordinary as usize]
            .iter()
            .position(|range| range.vertices.is_empty())
            .expect("speculative frames must exceed the mesh budget") as u32
    };
    // A different resident frame can be selected without changing the cache's model revision.
    world.resource_mut::<DroppedItemScene>().publish(
        1,
        Arc::clone(&models),
        &[instance(withheld), instance(ordinary)],
        &[],
        1.0,
    );
    world.run_system_once(prepare_items).unwrap();
    let (mesh, atlas) = {
        let gpu = world.resource::<ItemGpu>();
        assert_eq!(gpu.models_revision, 1);
        assert_eq!(gpu.draws.len(), 2);
        assert!(!gpu.ranges[withheld as usize].vertices.is_empty());
        assert!(!gpu.ranges[ordinary as usize].vertices.is_empty());
        (
            gpu.mesh_buffer.as_ref().unwrap().id(),
            gpu._atlas.as_ref().unwrap().id(),
        )
    };
    world.run_system_once(prepare_items).unwrap();
    assert_eq!(
        world
            .resource::<ItemGpu>()
            .mesh_buffer
            .as_ref()
            .unwrap()
            .id(),
        mesh
    );
    let resident = world.resource::<ItemGpu>().ranges[..ordinary as usize]
        .iter()
        .enumerate()
        .find(|(slot, range)| *slot != withheld as usize && !range.vertices.is_empty())
        .unwrap()
        .0 as u32;
    world.resource_mut::<DroppedItemScene>().publish(
        1,
        models,
        &[instance(resident), instance(ordinary)],
        &[],
        1.0,
    );
    world.run_system_once(prepare_items).unwrap();
    let gpu = world.resource::<ItemGpu>();
    assert_eq!(gpu.draws.len(), 2);
    assert_eq!(gpu.mesh_buffer.as_ref().unwrap().id(), mesh);
    assert_eq!(gpu._atlas.as_ref().unwrap().id(), atlas);
}

#[test]
fn current_frames_fit_before_speculation_with_a_small_device_buffer() {
    let mut models = detailed_frames(2, 32);
    models.push(DroppedItemModel::Sprite(DroppedItemSprite {
        width: 1,
        height: 1,
        rgba8: Arc::from([255; 4]),
    }));
    let mut atlas = vec![255; (MIN_ITEM_ATLAS_SIDE * MIN_ITEM_ATLAS_SIDE * 4) as usize];
    let current_vertices = build_model(&mut atlas, &models[1], MIN_ITEM_ATLAS_SIDE)
        .unwrap()
        .len()
        + build_model(&mut atlas, &models[2], MIN_ITEM_ATLAS_SIDE)
            .unwrap()
            .len();
    let budget = (current_vertices * ITEM_MESH_VERTEX_BYTES) as u64;
    let prepared = PreparedModels::new(
        &models,
        &[false, true, true],
        render_model::dropped_item::ItemModelBudget::new(
            assets::texture::policy::device_policy(),
            MAX_ITEM_LAYERS,
            budget,
        ),
    );
    assert!(prepared.ranges[0].vertices.is_empty());
    assert!(!prepared.ranges[1].vertices.is_empty());
    assert!(!prepared.ranges[2].vertices.is_empty());
    assert_eq!(prepared.vertices.len(), current_vertices);
}

#[test]
fn prioritized_mesh_handles_sample_their_own_texture_layers() {
    let models = [(1, 11), (16, 40), (128, 80)].map(|(side, color)| {
        DroppedItemModel::Sprite(DroppedItemSprite {
            width: side,
            height: side,
            rgba8: vec![color; (side * side * 4) as usize].into(),
        })
    });
    let prepared = PreparedModels::new(
        &models,
        &[false, false, true],
        render_model::dropped_item::ItemModelBudget::new(
            assets::texture::policy::device_policy(),
            MAX_ITEM_LAYERS,
            assets::texture::policy::MAX_SUBSCRIBER_BYTES as u64,
        ),
    );
    let layer_bytes = (prepared.side * prepared.side * 4) as usize;
    for (slot, model) in models.iter().enumerate() {
        let range = &prepared.ranges[slot].vertices;
        assert!(!range.is_empty());
        let layer = prepared.vertices[range.start as usize].layer as usize;
        let DroppedItemModel::Sprite(sprite) = model else {
            unreachable!()
        };
        assert_eq!(
            &prepared.atlas[layer * layer_bytes..layer * layer_bytes + 4],
            &sprite.rgba8[..4]
        );
        let width = sprite.width as usize;
        for row in 0..sprite.height as usize {
            let source = row * width * 4;
            let target = layer * layer_bytes + row * prepared.side as usize * 4;
            assert_eq!(
                &prepared.atlas[target..target + width * 4],
                &sprite.rgba8[source..source + width * 4]
            );
        }
    }
}

#[test]
fn a_rejected_current_model_does_not_rebuild_when_drawable_frames_change() {
    let models: Arc<[DroppedItemModel]> = Arc::from([
        DroppedItemModel::Sprite(DroppedItemSprite {
            width: 1,
            height: 1,
            rgba8: Arc::from([255; 4]),
        }),
        DroppedItemModel::Sprite(DroppedItemSprite {
            width: 1,
            height: 1,
            rgba8: Arc::from([73; 4]),
        }),
        DroppedItemModel::Sprite(DroppedItemSprite {
            width: 0,
            height: 0,
            rgba8: Arc::from([]),
        }),
    ]);
    let mut world = item_world();
    world.resource_mut::<DroppedItemScene>().publish(
        1,
        Arc::clone(&models),
        &[instance(0), instance(2)],
        &[],
        1.0,
    );
    world.run_system_once(prepare_items).unwrap();
    let mesh = world
        .resource::<ItemGpu>()
        .mesh_buffer
        .as_ref()
        .unwrap()
        .id();
    for frame in [1, 0, 1] {
        world.resource_mut::<DroppedItemScene>().publish(
            1,
            Arc::clone(&models),
            &[instance(frame), instance(2)],
            &[],
            1.0,
        );
        world.run_system_once(prepare_items).unwrap();
        let gpu = world.resource::<ItemGpu>();
        assert_eq!(gpu.draws.len(), 1);
        assert_eq!(gpu.mesh_buffer.as_ref().unwrap().id(), mesh);
        assert!(gpu.ranges[2].vertices.is_empty());
    }
}

#[test]
fn a_capacity_rejected_current_model_retries_when_other_current_geometry_shrinks() {
    let mut models = detailed_frames(3, 224);
    models.push(DroppedItemModel::Sprite(DroppedItemSprite {
        width: 1,
        height: 1,
        rgba8: Arc::from([255; 4]),
    }));
    let models: Arc<[DroppedItemModel]> = models.into();
    let mut world = item_world();
    world.resource_mut::<DroppedItemScene>().publish(
        1,
        Arc::clone(&models),
        &[instance(0), instance(1), instance(2)],
        &[],
        1.0,
    );
    world.run_system_once(prepare_items).unwrap();
    {
        let gpu = world.resource::<ItemGpu>();
        assert!(!gpu.ranges[0].vertices.is_empty());
        assert!(!gpu.ranges[1].vertices.is_empty());
        assert!(gpu.ranges[2].vertices.is_empty());
        assert!(gpu.ranges[2].capacity_rejected);
        assert!(!gpu.ranges[3].vertices.is_empty());
    }
    world.resource_mut::<DroppedItemScene>().publish(
        1,
        models,
        &[instance(1), instance(2), instance(3)],
        &[],
        1.0,
    );
    world.run_system_once(prepare_items).unwrap();
    let gpu = world.resource::<ItemGpu>();
    assert_eq!(gpu.models_revision, 1);
    assert_eq!(gpu.draws.len(), 3);
    for slot in [1, 2, 3] {
        assert!(!gpu.ranges[slot].vertices.is_empty());
    }
}
