//! UI texture bucket residency, reuse and allocation-failure recovery.

use super::*;

#[test]
fn device_observation_handles_clamped_aging_wrap_and_unknown_gaps() {
    use bevy::ecs::change_detection::{MAX_CHANGE_AGE, Tick};
    let mut observation = DeviceObservation::new(Tick::new(10));
    assert!(observation.observe(Tick::new(1), Tick::new(11), true));
    let near_age = MAX_CHANGE_AGE - 1;
    assert!(
        observation.observe(Tick::new(1), Tick::new(11 + near_age), true),
        "old unchanged resource aging is not a replacement"
    );
    assert!(
        observation.observe(Tick::new(12), Tick::new(12 + near_age), true),
        "clamped old resource stamp is not newer than last observation"
    );
    let mut wrapped = DeviceObservation::new(Tick::new(u32::MAX - 2));
    assert!(wrapped.observe(Tick::new(u32::MAX - 5), Tick::new(1), true));
    assert!(!wrapped.observe(Tick::new(2), Tick::new(3), true));
    assert!(
        !wrapped.observe(Tick::new(0), Tick::new(4), true),
        "observed invalidation is permanent"
    );
    let mut missing = DeviceObservation::new(Tick::new(10));
    assert!(
        !missing.observe(Tick::new(1), Tick::new(10 + MAX_CHANGE_AGE), true),
        "unknown detection-window gap fails closed"
    );
    assert!(!missing.observe(Tick::new(1), Tick::new(11 + MAX_CHANGE_AGE), true));
    assert!(
        !DeviceObservation::new(Tick::new(10)).observe(
            Tick::new(1),
            Tick::new(11 + MAX_CHANGE_AGE),
            true
        ),
        "gap beyond detection window is also unknown"
    );
    let mut mismatch = DeviceObservation::new(Tick::new(1));
    assert!(!mismatch.observe(Tick::new(1), Tick::new(2), false));
    assert!(!mismatch.observe(Tick::new(1), Tick::new(3), true));
    assert!(
        DeviceObservation::new(Tick::new(3)).observe(Tick::new(1), Tick::new(4), true),
        "actual recreation starts a new observer"
    );
}

fn catalog(value: u8) -> UiTextureCatalog {
    UiTextureCatalog::new(
        vec![
            UiTexturePage::owned([1024, 1024], vec![255; 1024 * 1024 * 4].into()).unwrap(),
            UiTexturePage::owned([256, 256], vec![value; 256 * 256 * 4].into()).unwrap(),
        ],
        1,
    )
    .unwrap()
}

#[test]
fn local_font_replacement_writes_one_reserved_page_without_static_reallocation() {
    use render_model::{
        MAX_UI_DYNAMIC_PAGES, UI_DYNAMIC_PAGE_SIDE, UI_LOCAL_FONT_PAGE_OFFSET,
        UI_LOCAL_FONT_PAGE_SIDE,
    };

    let page = |side, value| {
        UiTexturePage::owned(
            [side; 2],
            vec![value; side as usize * side as usize * 4].into(),
        )
        .unwrap()
    };
    let mut pages = vec![page(1, 255)];
    pages.extend((0..MAX_UI_DYNAMIC_PAGES).map(|offset| {
        page(
            if offset == UI_LOCAL_FONT_PAGE_OFFSET {
                UI_LOCAL_FONT_PAGE_SIDE
            } else {
                UI_DYNAMIC_PAGE_SIDE
            },
            0,
        )
    }));
    let base = UiTextureCatalog::new(pages, 1).unwrap();
    let mut state = TextureUploadState::default();
    let dirty = state.dirty(&base).unwrap();
    state
        .execute(&base, &dirty, |_, _, _| Ok::<_, ()>(()))
        .unwrap();
    let mut replacement = base.pages()[base.dynamic_start()..].to_vec();
    replacement[UI_LOCAL_FONT_PAGE_OFFSET] = page(UI_LOCAL_FONT_PAGE_SIDE, 41);
    let current = base.replace_dynamic(replacement).unwrap();
    assert_eq!(current.static_identity(), base.static_identity());
    assert_eq!(current.plan(), base.plan());
    let target = base.dynamic_start() + UI_LOCAL_FONT_PAGE_OFFSET;
    assert_eq!(state.dirty(&current).unwrap(), [target]);
    let mut written = Vec::new();
    state
        .execute(&current, &[target], |index, page, _| {
            written.push(index);
            assert_eq!(page.format(), base.pages()[target].format());
            assert_eq!(page.pixels().len(), base.pages()[target].pixels().len());
            Ok::<_, ()>(())
        })
        .unwrap();
    assert_eq!(written, [target]);
    assert!(state.dirty(&current).unwrap().is_empty());
    let mut wrong = base.pages()[base.dynamic_start()..].to_vec();
    wrong[UI_LOCAL_FONT_PAGE_OFFSET] = page(UI_DYNAMIC_PAGE_SIDE, 0);
    assert!(base.replace_dynamic(wrong).is_err());
}

#[test]
fn recording_executor_writes_only_changed_layers_and_retries_refusal() {
    let base = catalog(0);
    let mut state = TextureUploadState::default();
    let first = state.dirty(&base).unwrap();
    assert_eq!(first, [0, 1]);
    state
        .execute(&base, &first, |_, _, _| Ok::<_, ()>(()))
        .unwrap();
    assert!(state.dirty(&base).unwrap().is_empty());
    for value in 1..=100 {
        let changed = base
            .replace_dynamic(vec![
                UiTexturePage::owned([256, 256], vec![value; 256 * 256 * 4].into()).unwrap(),
            ])
            .unwrap();
        let dirty = state.dirty(&changed).unwrap();
        assert_eq!(dirty, [1]);
        assert!(state.execute(&changed, &dirty, |_, _, _| Err(())).is_err());
        assert_eq!(state.dirty(&changed).unwrap(), [1]);
        let mut written = Vec::new();
        state
            .execute(&changed, &dirty, |i, page, location| {
                written.push((i, page.pixels().len(), location));
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(written.len(), 1);
        assert_eq!(written[0].1, 256 * 256 * 4);
        assert!(state.dirty(&changed).unwrap().is_empty());
    }
    assert!(state.dirty(&catalog(101)).is_ok());
    let different_static = UiTextureCatalog::new(
        vec![
            UiTexturePage::owned([1024, 1024], vec![0; 1024 * 1024 * 4].into()).unwrap(),
            base.pages()[1].clone(),
        ],
        1,
    )
    .unwrap();
    assert!(state.dirty(&different_static).is_err());
}

#[test]
fn partial_initial_issuance_never_commits_and_retries_all_pages() {
    let catalog = catalog(0);
    let mut state = TextureUploadState::default();
    let dirty = state.dirty(&catalog).unwrap();
    assert!(
        state
            .execute(&catalog, &dirty, |i, _, _| if i == 1 {
                Err(())
            } else {
                Ok(())
            })
            .is_err()
    );
    assert!(state.static_identity.is_none());
    assert_eq!(state.dirty(&catalog).unwrap(), [0, 1]);
}

#[test]
fn missing_bucket_and_new_generation_never_recreate_or_reuse_uploaded_ids() {
    let (device, queue) = noop_device();
    let catalog = catalog(0);
    let mut gpu = UiGpuTextures::default();
    gpu.prepare(&catalog, &device, &queue, None, std::time::Duration::ZERO)
        .unwrap();
    assert_eq!(gpu.buckets.len(), 2);
    assert!(gpu.state.dirty(&catalog).unwrap().is_empty());
    assert!(gpu.resident(&catalog));
    let changed_generation =
        UiTextureCatalog::with_source_identity(catalog.pages().to_vec(), 1, [7; 32]).unwrap();
    assert!(
        gpu.prepare(
            &changed_generation,
            &device,
            &queue,
            None,
            std::time::Duration::ZERO
        )
        .is_err()
    );
    assert_eq!(gpu.buckets.len(), 2);
    gpu.buckets.pop();
    assert!(!gpu.resident(&catalog));
    for _ in 0..10 {
        assert!(
            gpu.prepare(&catalog, &device, &queue, None, std::time::Duration::ZERO)
                .is_err()
        );
        assert_eq!(gpu.buckets.len(), 1);
    }
}

#[test]
fn ui_model_resize_replaces_only_changed_buckets_and_rebinds_them() {
    use bevy::ecs::system::RunSystemOnce;
    use render_model::{
        UI_DYNAMIC_PAGE_SIDE, UI_MODEL_ATLAS_PAGE_OFFSET, UI_MODEL_ATLAS_SIDE,
        UI_PLAYER_SKIN_PAGE_OFFSET,
    };
    use {
        super::super::{UiGpu, UiRenderInput, UiRenderScene},
        render_scene::{UiRenderSceneResource, UiRenderStatsResource},
    };

    let small = UiTexturePage::owned(
        [UI_DYNAMIC_PAGE_SIDE; 2],
        vec![0; (UI_DYNAMIC_PAGE_SIDE * UI_DYNAMIC_PAGE_SIDE * 4) as usize].into(),
    )
    .unwrap();
    let mut pages = vec![UiTexturePage::owned([1; 2], vec![255; 4].into()).unwrap()];
    pages.extend(vec![small; UI_MODEL_ATLAS_PAGE_OFFSET + 1]);
    let base = UiTextureCatalog::new(pages, 1).unwrap();
    let mut replacements = base.pages()[base.dynamic_start()..].to_vec();
    let skin_side = render_api::CLASSIC_SKIN_SIDE as u32;
    replacements[UI_PLAYER_SKIN_PAGE_OFFSET] = UiTexturePage::owned(
        [skin_side; 2],
        vec![7; (skin_side * skin_side * 4) as usize].into(),
    )
    .unwrap();
    replacements[UI_MODEL_ATLAS_PAGE_OFFSET] = UiTexturePage::owned(
        [UI_MODEL_ATLAS_SIDE; 2],
        vec![11; (UI_MODEL_ATLAS_SIDE * UI_MODEL_ATLAS_SIDE * 4) as usize].into(),
    )
    .unwrap();
    let resized = base.replace_dynamic(replacements).unwrap();
    assert_eq!(base.static_identity(), resized.static_identity());
    let mut world = super::super::ordered_command_tests::binding_world();
    let input = |revision, catalog| UiRenderInput {
        revision,
        viewport_size: [64; 2],
        safe_area: [0; 4],
        vertices: std::sync::Arc::from([]),
        indices: std::sync::Arc::from([]),
        batches: std::sync::Arc::from([]),
        textures: std::sync::Arc::new(catalog),
    };
    let mut scene = UiRenderScene::default();
    scene
        .publish(
            input(1, base.clone()),
            world.resource::<UiRenderStatsResource>(),
        )
        .unwrap();
    world.insert_resource(UiRenderSceneResource(scene.clone()));
    world
        .run_system_once(super::super::resources::prepare_ui_resources)
        .unwrap();
    world
        .run_system_once(super::super::prepare_ui_bind_group)
        .unwrap();
    let initial_textures = world
        .resource::<UiGpu>()
        .textures
        .buckets
        .iter()
        .map(|bucket| bucket.as_ref().unwrap().texture.id())
        .collect::<Vec<_>>();
    assert!(
        world.resource::<UiGpu>().textures.buckets.iter().all(|b| b
            .as_ref()
            .unwrap()
            .bind_group
            .is_some())
    );

    scene
        .publish(
            input(2, resized.clone()),
            world.resource::<UiRenderStatsResource>(),
        )
        .unwrap();
    world.insert_resource(UiRenderSceneResource(scene.clone()));
    world
        .run_system_once(super::super::resources::prepare_ui_resources)
        .unwrap();
    let gpu = world.resource::<UiGpu>();
    assert_eq!(gpu.accepted_revision, Some(2));
    assert!(gpu.textures.resident(&resized));
    assert_only_changed_buckets_replaced(&gpu.textures, &base, &resized, &initial_textures);
    let resized_textures = gpu
        .textures
        .buckets
        .iter()
        .map(|bucket| bucket.as_ref().unwrap().texture.id())
        .collect::<Vec<_>>();
    assert_eq!(gpu.textures.bytes, resized.resident_plan().bytes());
    assert_eq!(gpu.textures.locations, resized.resident_plan().locations());
    world
        .run_system_once(super::super::prepare_ui_bind_group)
        .unwrap();
    assert!(
        world.resource::<UiGpu>().textures.buckets.iter().all(|b| b
            .as_ref()
            .unwrap()
            .bind_group
            .is_some())
    );

    scene
        .publish(
            input(3, base.clone()),
            world.resource::<UiRenderStatsResource>(),
        )
        .unwrap();
    world.insert_resource(UiRenderSceneResource(scene));
    world
        .run_system_once(super::super::resources::prepare_ui_resources)
        .unwrap();
    let gpu = world.resource::<UiGpu>();
    assert_eq!(gpu.accepted_revision, Some(3));
    assert!(gpu.textures.resident(&base));
    assert_only_changed_buckets_replaced(&gpu.textures, &resized, &base, &resized_textures);
}

/// Asserts buckets unchanged between plans keep their texture and binding while every
/// other bucket is new and unbound.
fn assert_only_changed_buckets_replaced(
    gpu: &UiGpuTextures,
    from: &UiTextureCatalog,
    to: &UiTextureCatalog,
    from_textures: &[bevy::render::render_resource::TextureId],
) {
    assert!(
        to.resident_plan()
            .buckets()
            .iter()
            .any(|bucket| from.resident_plan().buckets().contains(bucket)),
        "fixture keeps at least one bucket"
    );
    for (bucket, planned) in gpu.buckets.iter().zip(to.resident_plan().buckets()) {
        let bucket = bucket.as_ref().unwrap();
        let unchanged = from.resident_plan().buckets().contains(planned);
        assert_eq!(from_textures.contains(&bucket.texture.id()), unchanged);
        assert_eq!(bucket.bind_group.is_some(), unchanged);
    }
}

/// Builds a static 1024 page with three filled dynamic pages, and a resize of the
/// player skin and first model atlas that moves two pages to new buckets.
fn resizable_catalogs() -> (UiTextureCatalog, UiTextureCatalog) {
    use render_model::{
        UI_DYNAMIC_PAGE_SIDE, UI_MODEL_ATLAS_PAGE_OFFSET, UI_MODEL_ATLAS_SIDE,
        UI_PLAYER_SKIN_PAGE_OFFSET,
    };
    let page = |side: u32, value| {
        UiTexturePage::owned([side; 2], vec![value; (side * side * 4) as usize].into()).unwrap()
    };
    let mut pages = vec![page(1024, 1)];
    pages.extend((0..=UI_MODEL_ATLAS_PAGE_OFFSET).map(|_| page(UI_DYNAMIC_PAGE_SIDE, 3)));
    let base = UiTextureCatalog::new(pages, 1).unwrap();
    let mut replacements = base.pages()[base.dynamic_start()..].to_vec();
    replacements[UI_PLAYER_SKIN_PAGE_OFFSET] = page(render_api::CLASSIC_SKIN_SIDE as u32, 7);
    replacements[UI_MODEL_ATLAS_PAGE_OFFSET] = page(UI_MODEL_ATLAS_SIDE, 11);
    let resized = base.replace_dynamic(replacements).unwrap();
    (base, resized)
}

/// Creates a no-op device and queue whose allocations always succeed.
fn noop_device() -> (RenderDevice, RenderQueue) {
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    (RenderDevice::from(device), RenderQueue::new(queue))
}

#[test]
fn resizing_a_page_reuploads_only_pages_in_replaced_buckets() {
    let (device, queue) = noop_device();
    let (base, resized) = resizable_catalogs();
    let mut gpu = UiGpuTextures::default();
    gpu.prepare(&base, &device, &queue, None, std::time::Duration::ZERO)
        .unwrap();
    let kept = gpu.buckets[0].as_ref().unwrap().texture.id();
    let uploads = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let observed = std::rc::Rc::clone(&uploads);
    let _observe = crate::gpu_allocation::inject(move |label, bytes| {
        if label == "UI texture page upload" {
            observed.borrow_mut().push(bytes);
        }
        false
    });
    gpu.prepare(&resized, &device, &queue, None, std::time::Duration::ZERO)
        .unwrap();
    assert!(gpu.resident(&resized));
    assert_eq!(
        gpu.buckets[0].as_ref().unwrap().texture.id(),
        kept,
        "static 1024 bucket is unchanged"
    );
    let static_bytes = resized.pages()[0].pixels().len() as u64;
    let all_bytes: u64 = resized
        .pages()
        .iter()
        .map(|page| page.pixels().len() as u64)
        .sum();
    assert_eq!(
        uploads.borrow().iter().sum::<u64>(),
        all_bytes - static_bytes
    );
}

/// Unused reserved pages after a bucket's last used page share one GPU layer; filling one
/// gives it its own layer and leaves the other buckets' textures alone.
#[test]
fn blank_reserved_pages_share_one_layer_until_filled() {
    use render_model::{UI_DYNAMIC_PAGE_SIDE, UI_LOCAL_FONT_PAGE_OFFSET, UI_LOCAL_FONT_PAGE_SIDE};
    let (device, queue) = noop_device();
    let page = |side: u32, value| {
        UiTexturePage::owned([side; 2], vec![value; (side * side * 4) as usize].into()).unwrap()
    };
    let side = UI_DYNAMIC_PAGE_SIDE;
    let mut pages = vec![page(1024, 1), page(side, 2)];
    pages.extend((0..=UI_LOCAL_FONT_PAGE_OFFSET).map(|offset| {
        if offset == UI_LOCAL_FONT_PAGE_OFFSET {
            page(UI_LOCAL_FONT_PAGE_SIDE, 0)
        } else {
            page(side, 0)
        }
    }));
    let base = UiTextureCatalog::new(pages, 2).unwrap();
    let mut gpu = UiGpuTextures::default();
    gpu.prepare(&base, &device, &queue, None, Duration::ZERO)
        .unwrap();
    let layers = |gpu: &UiGpuTextures, side: u32| {
        let bucket = base
            .plan()
            .buckets()
            .iter()
            .position(|bucket| bucket.dimensions == [side; 2])
            .unwrap();
        gpu.buckets[bucket]
            .as_ref()
            .unwrap()
            .texture
            .depth_or_array_layers()
    };
    // The static page and one layer shared by every blank reserved page.
    assert_eq!(layers(&gpu, side), 2);
    assert_eq!(layers(&gpu, UI_LOCAL_FONT_PAGE_SIDE), 1);
    let shared = gpu.locations[2];
    assert!(
        gpu.locations[2..2 + UI_LOCAL_FONT_PAGE_OFFSET]
            .iter()
            .all(|location| *location == shared)
    );
    let kept = layers(&gpu, UI_LOCAL_FONT_PAGE_SIDE);
    let font_texture = |gpu: &UiGpuTextures| {
        gpu.buckets[gpu.locations[2 + UI_LOCAL_FONT_PAGE_OFFSET].bucket]
            .as_ref()
            .unwrap()
            .texture
            .id()
    };
    let font_before = font_texture(&gpu);
    let mut replacement = base.pages()[base.dynamic_start()..].to_vec();
    replacement[3] = page(side, 9);
    let filled = base.replace_dynamic(replacement).unwrap();
    gpu.prepare(&filled, &device, &queue, None, Duration::ZERO)
        .unwrap();
    assert!(gpu.resident(&filled));
    // Pages up to the filled slot keep their own layers, the rest share the one after it.
    assert_eq!(
        layers(&gpu, side),
        6,
        "static, three kept blanks, the filled page, shared"
    );
    assert_eq!(gpu.locations[2 + 3].layer, 4);
    assert_eq!(gpu.locations[2 + 4].layer, 5);
    assert_eq!(layers(&gpu, UI_LOCAL_FONT_PAGE_SIDE), kept);
    assert_eq!(font_texture(&gpu), font_before);
}

#[test]
fn bucket_allocation_failure_keeps_unchanged_buckets_and_retries() {
    let (device, queue) = noop_device();
    let (base, resized) = resizable_catalogs();
    let mut gpu = UiGpuTextures::default();
    gpu.prepare(&base, &device, &queue, None, Duration::ZERO)
        .unwrap();
    let kept = gpu.buckets[0].as_ref().unwrap().texture.id();
    {
        let _failure =
            crate::gpu_allocation::inject(|label, _| label == "bounded UI dimension bucket");
        assert!(
            matches!(gpu.prepare(&resized, &device, &queue, None, Duration::ZERO), Err(UiRenderRejectReason::TextureAllocationFailed { bytes }) if bytes > 0)
        );
        assert_eq!(gpu.buckets[0].as_ref().unwrap().texture.id(), kept);
        assert!(gpu.page_resident(0));
        assert!(gpu.buckets.iter().any(Option::is_none));
    }
    gpu.prepare(&resized, &device, &queue, None, TEXTURE_RETRY_DELAY)
        .unwrap();
    assert!(gpu.resident(&resized));
}

#[test]
fn page_upload_failure_leaves_pages_dirty_until_a_complete_retry() {
    let (device, queue) = noop_device();
    let catalog = catalog(0);
    let mut gpu = UiGpuTextures::default();
    {
        let _failure = crate::gpu_allocation::inject(|label, _| label == "UI texture page upload");
        assert_eq!(
            gpu.prepare(&catalog, &device, &queue, None, std::time::Duration::ZERO),
            Err(UiRenderRejectReason::TextureAllocationFailed {
                bytes: (1024 * 1024 + 256 * 256) * 4
            })
        );
    }
    assert!(!gpu.resident(&catalog));
    assert_eq!(gpu.state.dirty(&catalog).unwrap(), [0, 1]);
    gpu.prepare(&catalog, &device, &queue, None, TEXTURE_RETRY_DELAY)
        .unwrap();
    assert!(gpu.resident(&catalog));
}

#[test]
fn ui_model_plan_change_requires_all_writes_and_commits_only_after_complete_issuance() {
    use render_model::{UI_DYNAMIC_PAGE_SIDE, UI_PLAYER_SKIN_PAGE_OFFSET};
    let mut pages = vec![catalog(0).pages()[0].clone()];
    pages.extend(vec![
        catalog(0).pages()[1].clone();
        UI_PLAYER_SKIN_PAGE_OFFSET + 1
    ]);
    let base = UiTextureCatalog::new(pages, 1).unwrap();
    let mut replacements = base.pages()[1..].to_vec();
    let side = render_api::CLASSIC_SKIN_SIDE as u32;
    replacements[UI_PLAYER_SKIN_PAGE_OFFSET] =
        UiTexturePage::owned([side; 2], vec![0; (side * side * 4) as usize].into()).unwrap();
    let resized = base.replace_dynamic(replacements).unwrap();
    let mut state = TextureUploadState::default();
    state
        .execute(&base, &state.dirty(&base).unwrap(), |_, _, _| {
            Ok::<_, ()>(())
        })
        .unwrap();
    let all = state.dirty(&resized).unwrap();
    assert_eq!(all, (0..resized.pages().len()).collect::<Vec<_>>());
    assert!(
        state
            .execute(&resized, &all, |index, _, _| if index == 1 {
                Err(())
            } else {
                Ok(())
            })
            .is_err()
    );
    assert_eq!(state.plan.as_ref(), Some(base.resident_plan()));
    assert_eq!(state.dirty(&resized).unwrap(), all);
    let mut issued = Vec::new();
    state
        .execute(&resized, &all, |index, page, location| {
            issued.push((index, page.dimensions(), location));
            Ok::<_, ()>(())
        })
        .unwrap();
    assert_eq!(issued[1].1, [UI_DYNAMIC_PAGE_SIDE; 2]);
    assert_eq!(issued[2].1, [side; 2]);
    assert!(state.dirty(&resized).unwrap().is_empty());
    assert_eq!(state.plan.as_ref(), Some(resized.resident_plan()));
}

#[test]
fn resize_releases_superseded_buckets_before_retrying_under_budget() {
    let (device, queue) = noop_device();
    let (base, resized) = resizable_catalogs();
    let mut gpu = UiGpuTextures::default();
    gpu.prepare(&base, &device, &queue, None, std::time::Duration::ZERO)
        .unwrap();
    let old = std::sync::Arc::downgrade(&gpu.buckets[1].as_ref().unwrap().allocation_lifetime);
    let superseded_bytes = base.pages()[1..]
        .iter()
        .map(|page| page.pixels().len())
        .sum::<usize>();
    let budget = resized.plan().bytes() as u64;
    let mut allocated_bytes = base.pages()[0].pixels().len() as u64;
    let attempts = std::rc::Rc::new(std::cell::Cell::new(0));
    let observed = attempts.clone();
    let _failure = crate::gpu_allocation::inject(move |label, bytes| {
        if label != "bounded UI dimension bucket" {
            return false;
        }
        observed.set(observed.get() + 1);
        let live_old_bytes = if old.upgrade().is_some() {
            superseded_bytes as u64
        } else {
            0
        };
        let failed = allocated_bytes + bytes + live_old_bytes > budget;
        if !failed {
            allocated_bytes += bytes;
        }
        failed
    });
    gpu.prepare(&resized, &device, &queue, None, std::time::Duration::ZERO)
        .unwrap();
    assert!(gpu.resident(&resized));
    assert!(
        attempts.get() >= 2,
        "overlapping allocations must exhaust the budget first"
    );
}

/// Publishes one triangle per page so preparation exercises actual batch admission.
fn texture_scene(catalog: UiTextureCatalog) -> super::super::UiRenderInput {
    use render_model::{UiRenderBatch, UiRenderVertex, UiScissor};
    super::super::UiRenderInput {
        revision: 1,
        viewport_size: [64; 2],
        safe_area: [0; 4],
        vertices: vec![
            UiRenderVertex {
                position: [1.; 2],
                clip_z: 0.,
                clip_w: 1.,
                uv: [0.; 2],
                color: [255; 4],
                style_flags: 0,
                alpha_cutoff: -1.,
                model_light: 1.,
                overlay_color: [0.; 4],
            };
            3
        ]
        .into(),
        indices: (0..catalog.pages().len())
            .flat_map(|_| [0, 1, 2])
            .collect::<Vec<_>>()
            .into(),
        batches: (0..catalog.pages().len())
            .map(|page| {
                UiRenderBatch::new(
                    page as u32,
                    UiScissor::new(0, 0, 64, 64),
                    page as u32 * 3,
                    3,
                    0,
                )
            })
            .collect::<Vec<_>>()
            .into(),
        textures: std::sync::Arc::new(catalog),
    }
}

#[test]
fn failed_bucket_keeps_other_pages_and_ui_batches_drawable() {
    use bevy::ecs::system::RunSystemOnce;
    use render_scene::{UiRenderSceneResource, UiRenderStatsResource};
    for failed_stage in [
        "bounded UI dimension bucket",
        "UI bucket bindings",
        "UI texture page upload",
        "UI font atlas upload",
    ] {
        let mut world = super::super::ordered_command_tests::binding_world();
        let input = if failed_stage == "UI font atlas upload" {
            let mut input = super::super::font_atlas_tests::input();
            let pages = vec![
                catalog(0).pages()[0].clone(),
                input.textures.pages()[0].clone(),
            ];
            input.textures = std::sync::Arc::new(UiTextureCatalog::new(pages, 2).unwrap());
            let mut font = input.batches[0];
            font.texture_page = 1;
            font.first_index = input.indices.len() as u32;
            input.indices = input
                .indices
                .iter()
                .copied()
                .cycle()
                .take(input.indices.len() * 2)
                .collect::<Vec<_>>()
                .into();
            input.batches = vec![input.batches[0], font].into();
            input
        } else {
            texture_scene(catalog(0))
        };
        let catalog = input.textures.clone();
        let failed_bytes = catalog.pages()[1].pixels().len() as u64;
        let mut scene = super::super::UiRenderScene::default();
        scene
            .publish(input, world.resource::<UiRenderStatsResource>())
            .unwrap();
        world.insert_resource(UiRenderSceneResource(scene));
        let _failure = crate::gpu_allocation::inject(move |label, bytes| {
            label == failed_stage
                && (failed_stage == "UI font atlas upload"
                    || bytes == failed_bytes
                    || bytes == (1024 * 1024 * 4) + failed_bytes)
        });
        world
            .run_system_once(super::super::resources::prepare_ui_resources)
            .unwrap();
        world
            .run_system_once(super::super::prepare_ui_bind_group)
            .unwrap();
        let gpu = world.resource::<super::super::UiGpu>();
        assert_eq!(gpu.accepted_revision, Some(1));
        assert_eq!(gpu.batches.len(), 2);
        assert_eq!(
            gpu.textures.state.uploaded,
            [Some(catalog.pages()[0].identity()), None]
        );
        assert!(!gpu.textures.resident(&catalog));
        assert!(gpu.textures.page_binding(0).is_some());
        assert!(gpu.textures.page_binding(1).is_none());
        let drawable = super::super::resolved_batches(
            gpu.accepted_revision,
            &gpu.batches,
            &gpu.textures.locations,
            gpu.textures.allocated_buckets(),
        )
        .unwrap()
        .filter(|(_, batch, _)| {
            gpu.textures
                .page_binding(batch.texture_page as usize)
                .is_some()
        })
        .map(|(index, _, _)| index)
        .collect::<Vec<_>>();
        assert_eq!(drawable, [0]);
        assert!(!super::super::overlay::screen_pages_complete(gpu));
        let generation = gpu.textures.residency_generation;
        world
            .run_system_once(super::super::resources::prepare_ui_resources)
            .unwrap();
        assert_eq!(
            world.resource::<super::super::UiGpu>().accepted_revision,
            Some(1)
        );
        drop(_failure);
        world.resource_mut::<super::super::UiGpu>().started -= TEXTURE_RETRY_DELAY;
        world
            .run_system_once(super::super::resources::prepare_ui_resources)
            .unwrap();
        world
            .run_system_once(super::super::prepare_ui_bind_group)
            .unwrap();
        let gpu = world.resource::<super::super::UiGpu>();
        assert_eq!(gpu.accepted_revision, Some(1));
        assert!(gpu.textures.resident(&catalog));
        assert!(gpu.textures.page_binding(1).is_some());
        assert_ne!(gpu.textures.residency_generation, generation);
        assert!(super::super::overlay::screen_pages_complete(gpu));
    }
}

#[test]
fn unchanged_failed_plan_waits_for_backoff_but_changed_plan_retries() {
    use std::{cell::Cell, rc::Rc, time::Duration};
    let (device, queue) = noop_device();
    let (base, resized) = resizable_catalogs();
    let mut gpu = UiGpuTextures::default();
    let attempts = Rc::new(Cell::new(0));
    let observed = attempts.clone();
    let _failure = crate::gpu_allocation::inject(move |label, _| {
        if label != "bounded UI dimension bucket" {
            return false;
        }
        observed.set(observed.get() + 1);
        true
    });
    assert!(
        gpu.prepare(&base, &device, &queue, None, Duration::ZERO)
            .is_err()
    );
    let first = attempts.get();
    assert!(
        gpu.prepare(&base, &device, &queue, None, Duration::from_millis(1))
            .is_err()
    );
    assert_eq!(attempts.get(), first, "next frame must not retry");
    assert!(
        gpu.prepare(&base, &device, &queue, None, Duration::from_secs(1))
            .is_err()
    );
    let retried = attempts.get();
    assert!(
        retried > first,
        "bounded backoff must allow another attempt"
    );
    assert!(
        gpu.prepare(&resized, &device, &queue, None, Duration::from_secs(1))
            .is_err()
    );
    assert!(attempts.get() > retried, "plan changes retry immediately");
}
