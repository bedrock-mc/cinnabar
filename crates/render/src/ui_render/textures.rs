//! Retained dimension buckets and transactional dirty-page write planning.

use bevy::render::{
    render_resource::{
        BindGroup, Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
        TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
        TextureViewDescriptor, TextureViewDimension,
    },
    renderer::{RenderDevice, RenderQueue},
};

use bevy::prelude::{Res, ResMut};
use bevy::render::render_resource::{BindGroupEntry, BindingResource, PipelineCache};
use render_model::UiRenderRejectReason;

use super::{UiGpu, UiPipeline};
use render_model::{UiTextureCatalog, UiTextureLocation, UiTexturePage, UiTexturePlan};

/// Observes schedule-separated device-resource changes, not arbitrary context IDs.
pub(crate) struct DeviceObservation {
    last_observed: bevy::ecs::change_detection::Tick,
    invalidated: bool,
}
impl DeviceObservation {
    pub(crate) fn new(now: bevy::ecs::change_detection::Tick) -> Self {
        Self {
            last_observed: now,
            invalidated: false,
        }
    }
    pub(crate) fn observe(
        &mut self,
        changed: bevy::ecs::change_detection::Tick,
        now: bevy::ecs::change_detection::Tick,
        same_device: bool,
    ) -> bool {
        let gap = now.get().wrapping_sub(self.last_observed.get());
        self.invalidated |= !same_device
            || gap >= bevy::ecs::change_detection::MAX_CHANGE_AGE
            || changed.is_newer_than(self.last_observed, now);
        self.last_observed = now;
        !self.invalidated
    }
}

pub(super) struct GpuBucket {
    pub(super) texture: Texture,
    pub(super) view: TextureView,
    pub(super) bind_group: Option<BindGroup>,
}

#[derive(Default)]
pub(super) struct TextureUploadState {
    static_identity: Option<[u8; 32]>,
    plan: Option<UiTexturePlan>,
    uploaded: Vec<[u8; 32]>,
}

impl TextureUploadState {
    fn dirty(&self, catalog: &UiTextureCatalog) -> Result<Vec<usize>, UiRenderRejectReason> {
        if self
            .static_identity
            .is_some_and(|id| id != catalog.static_identity())
        {
            return Err(UiRenderRejectReason::TextureIdentityConflict {
                identity: catalog.static_identity(),
            });
        }
        Ok(catalog
            .pages()
            .iter()
            .enumerate()
            .filter_map(|(index, page)| {
                (self.plan.as_ref().is_none_or(|plan| plan != catalog.plan())
                    || self.uploaded.get(index) != Some(&page.identity()))
                .then_some(index)
            })
            .collect())
    }

    /// The executor is shared by device preparation and deterministic recording
    /// tests. Only successful issuance of ALL writes commits content IDs.
    fn execute<E>(
        &mut self,
        catalog: &UiTextureCatalog,
        dirty: &[usize],
        mut write: impl FnMut(usize, &UiTexturePage, UiTextureLocation) -> Result<(), E>,
    ) -> Result<(), E> {
        for &index in dirty {
            write(
                index,
                &catalog.pages()[index],
                catalog.plan().locations()[index],
            )?;
        }
        self.uploaded.clear();
        self.uploaded
            .extend(catalog.pages().iter().map(UiTexturePage::identity));
        self.static_identity = Some(catalog.static_identity());
        self.plan = Some(catalog.plan().clone());
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct UiGpuTextures {
    pub(super) buckets: Vec<GpuBucket>,
    pub(super) state: TextureUploadState,
    pub(super) locations: Vec<UiTextureLocation>,
    pub(super) bytes: usize,
    allocation_identity: Option<[u8; 32]>,
    allocation_plan: Option<UiTexturePlan>,
}

impl UiGpuTextures {
    pub(super) fn allocated_buckets(&self) -> &[render_model::UiTextureBucket] {
        self.allocation_plan
            .as_ref()
            .map_or(&[], |plan| plan.buckets())
    }
    pub(super) fn resident(&self, catalog: &UiTextureCatalog) -> bool {
        self.allocation_identity == Some(catalog.static_identity())
            && self.allocation_plan.as_ref() == Some(catalog.plan())
            && self.buckets.len() == catalog.plan().buckets().len()
            && self.locations == catalog.plan().locations()
            && self.state.uploaded.len() == catalog.pages().len()
            && self
                .state
                .uploaded
                .iter()
                .zip(catalog.pages())
                .all(|(id, page)| *id == page.identity())
    }
    pub(super) fn prepare(
        &mut self,
        catalog: &UiTextureCatalog,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> Result<(), UiRenderRejectReason> {
        let limits = device.limits();
        catalog.plan().validate_device(
            limits.max_texture_dimension_2d,
            limits.max_texture_array_layers,
        )?;
        if self
            .allocation_identity
            .is_some_and(|id| id != catalog.static_identity())
        {
            return Err(UiRenderRejectReason::TextureIdentityConflict {
                identity: catalog.static_identity(),
            });
        }
        let resized = self
            .allocation_plan
            .as_ref()
            .is_some_and(|plan| plan != catalog.plan());
        if self.allocation_identity.is_some()
            && !resized
            && (self.buckets.len() != catalog.plan().buckets().len()
                || self.locations != catalog.plan().locations())
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let dirty = self.state.dirty(catalog)?;
        let format = TextureFormat::Rgba8Unorm.guaranteed_format_features(device.features());
        if !format
            .allowed_usages
            .contains(TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST)
            || !format
                .flags
                .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        // All catalog and per-device admission checks precede allocation/writes.
        if resized {
            self.buckets.clear();
            self.locations.clear();
            self.state = TextureUploadState::default();
        }
        if self.buckets.is_empty() {
            self.allocation_identity = Some(catalog.static_identity());
            self.allocation_plan = Some(catalog.plan().clone());
            for bucket in catalog.plan().buckets() {
                let texture = device.create_texture(&TextureDescriptor {
                    label: Some("bounded UI dimension bucket"),
                    size: Extent3d {
                        width: bucket.dimensions[0],
                        height: bucket.dimensions[1],
                        depth_or_array_layers: bucket.layers,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: TextureDimension::D2,
                    format: TextureFormat::Rgba8Unorm,
                    usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let view = texture.create_view(&TextureViewDescriptor {
                    label: Some("bounded UI dimension bucket view"),
                    dimension: Some(TextureViewDimension::D2Array),
                    ..Default::default()
                });
                self.buckets.push(GpuBucket {
                    texture,
                    view,
                    bind_group: None,
                });
            }
            self.locations = catalog.plan().locations().to_vec();
            self.bytes = catalog.plan().bytes();
        }
        if self.buckets.len() != catalog.plan().buckets().len() {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        if dirty.is_empty() {
            return Ok(());
        }
        let buckets = &self.buckets;
        self.state.execute(catalog, &dirty, |_, page, location| {
            let [width, height] = page.dimensions();
            queue.write_texture(
                TexelCopyTextureInfo {
                    texture: &buckets[location.bucket].texture,
                    mip_level: 0,
                    origin: Origin3d {
                        x: 0,
                        y: 0,
                        z: location.layer,
                    },
                    aspect: Default::default(),
                },
                page.pixels(),
                TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            Ok::<(), UiRenderRejectReason>(())
        })
    }
}

/// Bind each page bucket with the viewport and both samplers once the frame is accepted.
pub(super) fn prepare_ui_bind_group(
    render_device: Res<RenderDevice>,
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<UiPipeline>,
    mut gpu: ResMut<UiGpu>,
) {
    if gpu.accepted_revision.is_none() || &gpu.device != render_device.wgpu_device() {
        return;
    }
    let viewport = gpu.viewport_buffer.clone();
    let sampler = gpu.sampler.clone();
    let linear_sampler = gpu.linear_sampler.clone();
    for bucket in &mut gpu.textures.buckets {
        if bucket.bind_group.is_some() {
            continue;
        }
        bucket.bind_group = Some(render_device.create_bind_group(
            "shared retained UI bind group",
            &pipeline_cache.get_bind_group_layout(&pipeline.bind_group_layout),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: viewport.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&bucket.view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::Sampler(&sampler),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(&linear_sampler),
                },
            ],
        ));
    }
}

#[cfg(test)]
mod tests {
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
        let changed = base.replace_dynamic(replacement).unwrap();
        assert_eq!(changed.static_identity(), base.static_identity());
        assert_eq!(changed.plan(), base.plan());
        let target = base.dynamic_start() + UI_LOCAL_FONT_PAGE_OFFSET;
        assert_eq!(state.dirty(&changed).unwrap(), [target]);
        let mut written = Vec::new();
        state
            .execute(&changed, &[target], |index, _, _| {
                written.push(index);
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(written, [target]);
        assert!(state.dirty(&changed).unwrap().is_empty());
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
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue(std::sync::Arc::new(
            bevy::render::renderer::WgpuWrapper::new(queue),
        ));
        let catalog = catalog(0);
        let mut gpu = UiGpuTextures::default();
        gpu.prepare(&catalog, &device, &queue).unwrap();
        assert_eq!(gpu.buckets.len(), 2);
        assert!(gpu.state.dirty(&catalog).unwrap().is_empty());
        assert!(gpu.resident(&catalog));
        let changed_generation =
            UiTextureCatalog::with_source_identity(catalog.pages().to_vec(), 1, [7; 32]).unwrap();
        assert!(gpu.prepare(&changed_generation, &device, &queue).is_err());
        assert_eq!(gpu.buckets.len(), 2);
        gpu.buckets.pop();
        assert!(!gpu.resident(&catalog));
        for _ in 0..10 {
            assert!(gpu.prepare(&catalog, &device, &queue).is_err());
            assert_eq!(gpu.buckets.len(), 1);
        }
    }

    #[test]
    fn ui_model_resize_rebuilds_uploads_and_never_retains_old_bucket_bindings() {
        use super::super::{
            UiGpu, UiRenderInput, UiRenderScene, UiRenderSceneResource, UiRenderStatsResource,
        };
        use bevy::ecs::system::RunSystemOnce;
        use render_model::{
            UI_DYNAMIC_PAGE_SIDE, UI_MODEL_ATLAS_PAGE_OFFSET, UI_MODEL_ATLAS_SIDE,
            UI_PLAYER_SKIN_PAGE_OFFSET,
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
            .run_system_once(super::super::prepare_ui_resources)
            .unwrap();
        world
            .run_system_once(super::super::prepare_ui_bind_group)
            .unwrap();
        let initial_textures = world
            .resource::<UiGpu>()
            .textures
            .buckets
            .iter()
            .map(|bucket| bucket.texture.id())
            .collect::<Vec<_>>();
        assert!(
            world
                .resource::<UiGpu>()
                .textures
                .buckets
                .iter()
                .all(|b| b.bind_group.is_some())
        );

        scene
            .publish(
                input(2, resized.clone()),
                world.resource::<UiRenderStatsResource>(),
            )
            .unwrap();
        world.insert_resource(UiRenderSceneResource(scene.clone()));
        world
            .run_system_once(super::super::prepare_ui_resources)
            .unwrap();
        let gpu = world.resource::<UiGpu>();
        assert_eq!(gpu.accepted_revision, Some(2));
        assert!(gpu.textures.resident(&resized));
        assert!(
            gpu.textures
                .buckets
                .iter()
                .all(|bucket| bucket.bind_group.is_none())
        );
        assert!(
            gpu.textures
                .buckets
                .iter()
                .all(|bucket| !initial_textures.contains(&bucket.texture.id()))
        );
        assert_eq!(gpu.textures.bytes, resized.plan().bytes());
        assert_eq!(gpu.textures.locations, resized.plan().locations());
        world
            .run_system_once(super::super::prepare_ui_bind_group)
            .unwrap();
        assert!(
            world
                .resource::<UiGpu>()
                .textures
                .buckets
                .iter()
                .all(|b| b.bind_group.is_some())
        );

        scene
            .publish(
                input(3, base.clone()),
                world.resource::<UiRenderStatsResource>(),
            )
            .unwrap();
        world.insert_resource(UiRenderSceneResource(scene));
        world
            .run_system_once(super::super::prepare_ui_resources)
            .unwrap();
        let gpu = world.resource::<UiGpu>();
        assert_eq!(gpu.accepted_revision, Some(3));
        assert!(gpu.textures.resident(&base));
        assert!(
            gpu.textures
                .buckets
                .iter()
                .all(|bucket| bucket.bind_group.is_none())
        );
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
        assert_eq!(state.plan.as_ref(), Some(base.plan()));
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
        assert_eq!(state.plan.as_ref(), Some(resized.plan()));
    }
}
