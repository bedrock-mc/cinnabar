use super::*;
use crate::dropped_items::preparation::{DropPreparation, publication_frustum, retain_lifetimes};
use client_world::{CanonicalItemStack, DroppedItemView, MAX_DROPPED_ITEM_COPIES};
use glam::{Mat4, Vec3};
use render_model::{
    ActorCullView, DroppedItemSpawnPose, dropped_item::DroppedItemFrustum,
    native_dropped_item_transform,
};
use std::cell::Cell;

/// Counts the actual lazy resolution, model construction and light boundaries.
#[derive(Default)]
struct Work {
    resolves: Cell<usize>,
    builds: Cell<usize>,
    lights: Cell<usize>,
}

/// Supplies a canonical item view without loading game assets or advancing a clock.
fn item(position: [f32; 3]) -> DroppedItemView {
    DroppedItemView {
        runtime_id: 7,
        spawn_revision: 1,
        position,
        age_ticks: 0.0,
        bob_phase: 0.0,
        bob_multiplier: 1.0,
        render_scale: 1.0,
        yaw_radians: 0.0,
        item: CanonicalItemStack {
            use_animation: Default::default(),
            grip: Default::default(),
            glint: Default::default(),
            identity: assets::ItemStackIdentity {
                network_id: 1,
                count: 1,
                ..assets::ItemStackIdentity::empty()
            },
            identifier: Some("test:item".into()),
            visual: ItemVisualRoute::Missing,
            map_id: None,
            charged_projectile: None,
            charged_projectile_role: Default::default(),
            damage: None,
            durability: assets::item::durability::ItemDurability::NotDamageable,
            enchanted: false,
        },
        copy_offsets: [[0.0; 3]; MAX_DROPPED_ITEM_COPIES],
        copy_count: 1,
    }
}

/// Uses the WebGPU clip box so every boundary can be addressed without projection estimates.
fn frustum() -> DroppedItemFrustum {
    publication_frustum(
        Some(ActorCullView {
            clip_from_world: Mat4::IDENTITY,
            camera_position: Vec3::ZERO,
            max_distance: 100.0,
        }),
        true,
    )
    .unwrap()
}

/// Makes a native sprite whose dimensions exercise the retained square envelope.
fn sprite() -> DroppedItemModel {
    DroppedItemModel::NativeSprite(DroppedItemSprite {
        width: 2,
        height: 1,
        rgba8: Arc::from([255; 8]),
    })
}

/// Makes the carried and ordinary block cube route without terrain carriers.
fn cube() -> DroppedItemModel {
    DroppedItemModel::Cube(DroppedItemCube {
        tile: 1,
        faces: std::array::from_fn(|_| Arc::from([255; 4])),
        tints: [render_model::OPAQUE_WHITE; 6],
    })
}

/// Retains an authored plane that can extend well beyond a unit block.
fn block(extent: i16) -> DroppedItemModel {
    DroppedItemModel::Block(DroppedItemBlock {
        materials: Arc::from([(
            DroppedItemSprite {
                width: 1,
                height: 1,
                rgba8: Arc::from([255; 4]),
            },
            render_model::OPAQUE_WHITE,
        )]),
        quads: Arc::from([assets::ModelQuad {
            positions: [
                [-extent, -extent, 128],
                [extent, -extent, 128],
                [extent, extent, 128],
                [-extent, extent, 128],
            ],
            uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
            material: 0,
            flags: assets::MODEL_QUAD_FLAG_TWO_SIDED,
        }]),
        rotation: 0,
    })
}

/// Protects all consumers before any item is prepared, as the production demand pass does.
fn frame(cache: &mut ModelCache, keys: &[ModelKey]) {
    cache.begin_frame();
    for key in keys {
        cache.request(key);
    }
    cache.finish_demand();
}

/// Supplies ordinary first-render inputs; tests can override the lifetime origin and camera.
fn preparation<'a>(
    view: &'a DroppedItemView,
    frustum: Option<&'a DroppedItemFrustum>,
) -> DropPreparation<'a> {
    DropPreparation {
        view,
        native_origin: view.position,
        camera: None,
        frustum,
    }
}

/// Exercises the production preparation and publication with deterministic work spies.
fn draw(
    cache: &mut ModelCache,
    prep: DropPreparation<'_>,
    key: &ModelKey,
    model: DroppedItemModel,
    work: &Work,
    output: &mut Vec<DroppedItemInstance>,
) -> bool {
    let shape = if matches!(
        model,
        DroppedItemModel::NativeSprite(_) | DroppedItemModel::Sprite(_)
    ) {
        DroppedItemShape::Sprite
    } else {
        DroppedItemShape::Cube
    };
    let Some(prepared) = prep.model(cache, key, shape, |cache| {
        work.resolves.set(work.resolves.get() + 1);
        cache.model(
            key.clone(),
            || Some(model.texture_layers()),
            || {
                work.builds.set(work.builds.get() + 1);
                Some(model.clone())
            },
        )
    }) else {
        return false;
    };
    prepared.append(prep.view, output, |_| {
        work.lights.set(work.lights.get() + 1);
        (7, 15)
    });
    true
}

#[test]
fn hidden_admitted_models_skip_resolution_builds_and_lights_on_every_plane() {
    let frustum = frustum();
    for model in [cube(), sprite(), block(512)] {
        let mut cache = ModelCache::default();
        let key = ModelKey::Block {
            hashed: false,
            id: 5,
        };
        let work = Work::default();
        for position in [
            [-10.0, 0.0, 0.5],
            [10.0, 0.0, 0.5],
            [0.0, -10.0, 0.5],
            [0.0, 10.0, 0.5],
            [0.0, 0.0, -10.0],
            [0.0, 0.0, 10.0],
        ] {
            let view = item(position);
            frame(&mut cache, std::slice::from_ref(&key));
            let mut output = Vec::new();
            assert!(draw(
                &mut cache,
                preparation(&view, Some(&frustum)),
                &key,
                model.clone(),
                &work,
                &mut output
            ));
            assert!(output.is_empty());
            assert_eq!(
                (work.resolves.get(), work.builds.get(), work.lights.get()),
                (1, 1, 0)
            );
            assert!(
                cache
                    .spawn_poses
                    .contains_key(&(view.runtime_id, view.spawn_revision))
            );
        }
    }
}

#[test]
fn surviving_output_matches_unculled_order_transforms_and_glint() {
    for model in [cube(), sprite(), block(512)] {
        let views = [0.0, 10.0, 1.05, -0.5].map(|x| item([x, -0.2, 0.5]));
        let keys: Vec<_> = (0..views.len())
            .map(|id| ModelKey::Block {
                hashed: false,
                id: id as u32,
            })
            .collect();
        let mut baseline_cache = ModelCache::default();
        let mut culled_cache = ModelCache::default();
        frame(&mut baseline_cache, &keys);
        frame(&mut culled_cache, &keys);
        let frustum = frustum();
        let mut expected = Vec::new();
        let mut actual = Vec::new();
        let work = Work::default();
        for (index, (mut view, key)) in views.into_iter().zip(&keys).enumerate() {
            view.runtime_id += index as u64;
            view.copy_count = (index + 1) as u8;
            view.copy_offsets[1] = [0.1, 0.0, 0.0];
            view.item.enchanted = index == 2;
            view.item.glint = assets::item::glint::ItemGlint::Session {
                enabled: index == 3,
            };
            let mut baseline = Vec::new();
            draw(
                &mut baseline_cache,
                preparation(&view, None),
                key,
                model.clone(),
                &Work::default(),
                &mut baseline,
            );
            if index != 1 {
                expected.extend(baseline);
            }
            draw(
                &mut culled_cache,
                preparation(&view, Some(&frustum)),
                key,
                model.clone(),
                &work,
                &mut actual,
            );
        }
        assert_eq!(actual, expected);
        assert_eq!(work.lights.get(), 3);
        assert_eq!(
            culled_cache.models, baseline_cache.models,
            "cold admission remains visibility independent"
        );
    }
}

#[test]
fn hidden_to_visible_keeps_first_admitted_camera_for_cached_and_cold_models() {
    for cached in [false, true] {
        let mut cache = ModelCache::default();
        let key = ModelKey::Block {
            hashed: false,
            id: 1,
        };
        frame(&mut cache, std::slice::from_ref(&key));
        if cached {
            cache
                .model(key.clone(), || Some(1), || Some(sprite()))
                .unwrap();
        }
        let frustum = frustum();
        let mut view = item([10.0, 0.0, 0.5]);
        let first_camera = Some(([0.5, 0.0, 0.0], 30.0));
        let expected = DroppedItemSpawnPose::new([0.0; 3], first_camera);
        let mut prep = preparation(&view, Some(&frustum));
        prep.native_origin = [0.0; 3];
        prep.camera = first_camera;
        let mut output = Vec::new();
        draw(
            &mut cache,
            prep,
            &key,
            sprite(),
            &Work::default(),
            &mut output,
        );
        assert!(output.is_empty());
        assert_eq!(cache.spawn_poses[&(7, 1)], expected);
        view.position = [0.0, -0.3, 0.5];
        view.age_ticks = 4.0;
        let mut prep = preparation(&view, Some(&frustum));
        prep.camera = Some(([0.0; 3], 130.0));
        draw(
            &mut cache,
            prep,
            &key,
            sprite(),
            &Work::default(),
            &mut output,
        );
        let mut position = view.position;
        position[1] += expected.bob(view.age_ticks, view.bob_phase, DroppedItemShape::Sprite);
        assert_eq!(
            output[0].world_from_item,
            native_dropped_item_transform(
                position,
                expected.yaw(view.yaw_radians),
                [0.0; 3],
                view.render_scale,
                DroppedItemShape::Sprite
            )
        );
        assert_eq!(cache.spawn_poses[&(7, 1)], expected);
    }
}

#[test]
fn failed_admission_does_not_capture_and_hidden_lifetimes_are_cleaned_up() {
    let mut cache = ModelCache::default();
    let key = ModelKey::Block {
        hashed: false,
        id: 1,
    };
    frame(&mut cache, std::slice::from_ref(&key));
    let frustum = frustum();
    let mut view = item([10.0, 0.0, 0.5]);
    assert!(
        preparation(&view, Some(&frustum))
            .model(&mut cache, &key, DroppedItemShape::Sprite, |_| None)
            .is_none()
    );
    assert!(cache.spawn_poses.is_empty());
    draw(
        &mut cache,
        preparation(&view, Some(&frustum)),
        &key,
        sprite(),
        &Work::default(),
        &mut Vec::new(),
    );
    assert!(cache.spawn_poses.contains_key(&(7, 1)));
    view.spawn_revision += 1;
    retain_lifetimes(&mut cache, std::slice::from_ref(&view));
    assert!(cache.spawn_poses.is_empty());
    draw(
        &mut cache,
        preparation(&view, Some(&frustum)),
        &key,
        sprite(),
        &Work::default(),
        &mut Vec::new(),
    );
    assert!(cache.spawn_poses.contains_key(&(7, 2)));
    retain_lifetimes(&mut cache, &[]);
    assert!(cache.spawn_poses.is_empty());
}

#[test]
fn all_live_copy_offsets_actor_scale_and_pickup_position_can_keep_an_item_visible() {
    let mut cache = ModelCache::default();
    let key = ModelKey::Block {
        hashed: false,
        id: 1,
    };
    frame(&mut cache, std::slice::from_ref(&key));
    let frustum = frustum();
    for last in 0..MAX_DROPPED_ITEM_COPIES {
        let mut view = item([1.3, -0.3, 0.5]);
        view.copy_count = (last + 1) as u8;
        view.yaw_radians = std::f32::consts::FRAC_PI_2;
        view.copy_offsets[last] = [0.0, 0.0, -0.2];
        let mut output = Vec::new();
        draw(
            &mut cache,
            preparation(&view, Some(&frustum)),
            &key,
            cube(),
            &Work::default(),
            &mut output,
        );
        assert_eq!(output.len(), last + 1, "every live copy can intersect");
    }
    for (position, scale, multiplier) in
        [([1.4, -0.3, 0.5], 4.0, 1.0), ([0.0, 0.0, 0.5], 0.25, 0.1)]
    {
        let mut view = item(position);
        view.render_scale = scale;
        view.bob_multiplier = multiplier;
        let mut prep = preparation(&view, Some(&frustum));
        prep.native_origin = [10.0, 0.0, 0.5];
        let mut output = Vec::new();
        draw(
            &mut cache,
            prep,
            &key,
            cube(),
            &Work::default(),
            &mut output,
        );
        assert_eq!(
            output.len(),
            1,
            "use current pickup interpolation and shrinking scale"
        );
    }
}

#[test]
fn bob_lift_and_close_spawn_rise_are_applied_before_rejection() {
    let frustum = frustum();
    let camera = Some(([0.5, 0.0, 0.0], 30.0));
    let pose = DroppedItemSpawnPose::new([0.0; 3], camera);
    for (model, shape) in [
        (cube(), DroppedItemShape::Cube),
        (sprite(), DroppedItemShape::Sprite),
    ] {
        let mut cache = ModelCache::default();
        let key = ModelKey::Block {
            hashed: false,
            id: 1,
        };
        frame(&mut cache, std::slice::from_ref(&key));
        for age in [0.0, 4.0, 8.0, 12.0] {
            for phase in [0.0, std::f32::consts::FRAC_PI_2] {
                let bob = pose.bob(age, phase, shape);
                let mut view = item([0.0, -1.0 - bob + 0.01, 0.5]);
                view.age_ticks = age;
                view.bob_phase = phase;
                let mut prep = preparation(&view, Some(&frustum));
                prep.native_origin = [0.0; 3];
                prep.camera = camera;
                let mut output = Vec::new();
                draw(
                    &mut cache,
                    prep,
                    &key,
                    model.clone(),
                    &Work::default(),
                    &mut output,
                );
                assert_eq!(output.len(), 1, "{shape:?}, age {age}, phase {phase}");
            }
        }
    }
}

#[test]
fn oversized_unknown_or_invalid_bounds_and_multiple_views_fail_open() {
    let frustum = frustum();
    for model in [
        block(4096),
        DroppedItemModel::Sprite(DroppedItemSprite {
            width: 1,
            height: 1,
            rgba8: Arc::from([255; 4]),
        }),
    ] {
        let mut cache = ModelCache::default();
        let key = ModelKey::Block {
            hashed: false,
            id: 1,
        };
        frame(&mut cache, std::slice::from_ref(&key));
        let view = item([2.0, 0.0, 0.5]);
        let mut output = Vec::new();
        draw(
            &mut cache,
            preparation(&view, Some(&frustum)),
            &key,
            model,
            &Work::default(),
            &mut output,
        );
        assert_eq!(output.len(), 1);
    }
    for invalid in 0..6 {
        let mut cache = ModelCache::default();
        let key = ModelKey::Block {
            hashed: false,
            id: 1,
        };
        frame(&mut cache, std::slice::from_ref(&key));
        let mut view = item([10.0, 0.0, 0.5]);
        match invalid {
            0 => view.age_ticks = f32::NAN,
            1 => view.render_scale = f32::INFINITY,
            2 => view.position[0] = f32::NAN,
            3 => view.copy_offsets[0][0] = f32::NAN,
            4 => view.bob_multiplier = f32::NAN,
            _ => view.yaw_radians = f32::NAN,
        }
        let mut output = Vec::new();
        draw(
            &mut cache,
            preparation(&view, Some(&frustum)),
            &key,
            cube(),
            &Work::default(),
            &mut output,
        );
        assert_eq!(output.len(), 1);
    }
    let valid = ActorCullView {
        clip_from_world: Mat4::IDENTITY,
        camera_position: Vec3::ZERO,
        max_distance: 1.0,
    };
    assert!(publication_frustum(Some(valid), false).is_none());
    assert!(publication_frustum(None, true).is_none());
    assert!(
        publication_frustum(
            Some(ActorCullView {
                max_distance: f32::NAN,
                ..valid
            }),
            true
        )
        .is_none()
    );
    assert!(
        publication_frustum(
            Some(ActorCullView {
                clip_from_world: Mat4::ZERO,
                ..valid
            }),
            true
        )
        .is_none()
    );
}

#[test]
fn generation_changes_drop_bounds_and_captures_before_reusing_a_model_slot() {
    let assets = Arc::new(RuntimeAssets::diagnostic());
    let first = render_scene::ChunkTextureAssets::with_revision(Arc::clone(&assets), 1).identity();
    let second = render_scene::ChunkTextureAssets::with_revision(assets, 2).identity();
    let mut cache = ModelCache::default();
    let key = ModelKey::Block {
        hashed: false,
        id: 1,
    };
    let view = item([10.0, 0.0, 0.5]);
    let frustum = frustum();
    for (session, assets, icons) in [
        (1, None, 0),
        (2, None, 0),
        (2, Some(first), 0),
        (2, Some(second), 0),
        (2, Some(second), 1),
    ] {
        cache.sync(session, assets, icons);
        assert!(cache.bounds(0).is_none());
        assert!(cache.spawn_poses.is_empty());
        frame(&mut cache, std::slice::from_ref(&key));
        draw(
            &mut cache,
            preparation(&view, Some(&frustum)),
            &key,
            cube(),
            &Work::default(),
            &mut Vec::new(),
        );
        assert!(cache.bounds(0).is_some());
        assert_eq!(cache.spawn_poses.len(), 1);
    }
}

#[test]
fn hidden_consumers_keep_atlas_protection_and_cold_admission_order_under_pressure() {
    let frustum = frustum();
    let hidden = item([10.0, 0.0, 0.5]);
    let visible = item([0.0, -0.2, 0.5]);
    let keys: Vec<_> = (0..render_model::MAX_ITEM_LAYERS as u32)
        .map(|id| ModelKey::Block { hashed: false, id })
        .collect();
    let mut cache = ModelCache::default();
    frame(&mut cache, &keys);
    for key in &keys[..keys.len() - 1] {
        assert!(draw(
            &mut cache,
            preparation(&hidden, Some(&frustum)),
            key,
            sprite(),
            &Work::default(),
            &mut Vec::new()
        ));
    }
    let delayed = keys.last().unwrap();
    assert!(!draw(
        &mut cache,
        preparation(&visible, Some(&frustum)),
        delayed,
        sprite(),
        &Work::default(),
        &mut Vec::new()
    ));
    frame(&mut cache, &keys);
    assert!(!draw(
        &mut cache,
        preparation(&visible, Some(&frustum)),
        delayed,
        sprite(),
        &Work::default(),
        &mut Vec::new()
    ));
    // A hidden current consumer stays protected while an absent consumer can be reclaimed.
    frame(&mut cache, &[keys[0].clone(), delayed.clone()]);
    assert!(draw(
        &mut cache,
        preparation(&visible, Some(&frustum)),
        delayed,
        sprite(),
        &Work::default(),
        &mut Vec::new()
    ));
    let work = Work::default();
    draw(
        &mut cache,
        preparation(&hidden, Some(&frustum)),
        &keys[0],
        sprite(),
        &work,
        &mut Vec::new(),
    );
    assert_eq!(
        (work.resolves.get(), work.builds.get(), work.lights.get()),
        (0, 0, 0)
    );
    assert!(cache.models.len() < render_model::MAX_ITEM_LAYERS);
}

#[test]
fn publisher_disables_frustum_rejection_for_missing_or_additional_3d_cameras() {
    use bevy::ecs::system::SystemState;
    use bevy::prelude::{Camera3d, World};
    let view = Some(ActorCullView {
        clip_from_world: Mat4::IDENTITY,
        camera_position: Vec3::ZERO,
        max_distance: 100.0,
    });
    let mut world = World::new();
    let mut state = SystemState::<DroppedItemPublisher<'_, '_>>::new(&mut world);
    assert!(
        state
            .get_mut(&mut world)
            .unwrap()
            .culling_view(view)
            .is_none()
    );
    let first = world.spawn(Camera3d::default()).id();
    assert!(
        state
            .get_mut(&mut world)
            .unwrap()
            .culling_view(view)
            .is_some()
    );
    let other = world.spawn(Camera3d::default()).id();
    assert!(
        state
            .get_mut(&mut world)
            .unwrap()
            .culling_view(view)
            .is_none()
    );
    world.despawn(other);
    assert!(
        state
            .get_mut(&mut world)
            .unwrap()
            .culling_view(view)
            .is_some()
    );
    world
        .entity_mut(first)
        .insert(bevy::render::camera::TemporalJitter::default());
    assert!(
        state
            .get_mut(&mut world)
            .unwrap()
            .culling_view(view)
            .is_none()
    );
    world
        .entity_mut(first)
        .remove::<bevy::render::camera::TemporalJitter>();
    world
        .entity_mut(first)
        .get_mut::<bevy::prelude::Camera>()
        .unwrap()
        .sub_camera_view = Some(Default::default());
    assert!(
        state
            .get_mut(&mut world)
            .unwrap()
            .culling_view(view)
            .is_none()
    );
}

#[test]
fn a_hidden_admitted_route_does_not_fall_through_to_an_icon() {
    let mut cache = ModelCache::default();
    let key = ModelKey::CarriedBlock {
        hashed: false,
        id: 1,
    };
    frame(&mut cache, std::slice::from_ref(&key));
    let view = item([10.0, 0.0, 0.5]);
    let frustum = frustum();
    let prep = preparation(&view, Some(&frustum));
    let prepared = prep
        .model(&mut cache, &key, DroppedItemShape::Cube, |cache| {
            cache.model(
                key.clone(),
                || Some(cube().texture_layers()),
                || Some(cube()),
            )
        })
        .or_else(|| panic!("hidden admission must not request a lower-priority model"));
    let mut output = Vec::new();
    prepared
        .unwrap()
        .append(&view, &mut output, |_| panic!("hidden lighting"));
    assert!(output.is_empty());
}
