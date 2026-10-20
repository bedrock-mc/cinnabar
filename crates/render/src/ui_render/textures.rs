//! Retained dimension buckets and per-bucket dirty-page write planning.

use crate::{gpu_allocation, queue_writes::ProfiledQueue as _};
use bevy::render::{
    render_resource::{
        BindGroup, Extent3d, Origin3d, TexelCopyBufferLayout, TexelCopyTextureInfo, Texture,
        TextureDescriptor, TextureDimension, TextureFormat, TextureUsages, TextureView,
        TextureViewDescriptor, TextureViewDimension,
    },
    renderer::{RenderDevice, RenderQueue},
};
use std::time::Duration;

use bevy::prelude::{Res, ResMut};
use bevy::render::render_resource::{BindGroupEntry, BindingResource, PipelineCache};
use render_model::UiRenderRejectReason;

use super::{UiGpu, pipeline::UiPipeline};
use render_model::{
    UiTextureCatalog, UiTextureFormat, UiTextureLocation, UiTexturePage, UiTexturePlan,
};

/// Delay between failed attempts for an unchanged texture plan.
const TEXTURE_RETRY_DELAY: Duration = Duration::from_millis(250);

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
    /// `x` is 1 for a coverage bucket, which the shader samples as white with that alpha.
    pub(super) format_uniform: bevy::render::render_resource::Buffer,
    pub(super) bind_group: Option<BindGroup>,
    #[cfg(test)]
    allocation_lifetime: std::sync::Arc<()>,
}

#[derive(Default)]
pub(super) struct TextureUploadState {
    static_identity: Option<[u8; 32]>,
    /// Plan whose locations the resident identities below occupy.
    plan: Option<UiTexturePlan>,
    /// Per page, the content identity resident at its location in `plan`.
    pub(super) uploaded: Vec<Option<[u8; 32]>>,
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
        // Residency is only meaningful against the plan it was recorded or rebound for.
        let replanned = self.plan.as_ref() != Some(catalog.resident_plan());
        Ok(catalog
            .pages()
            .iter()
            .enumerate()
            .filter_map(|(index, page)| {
                (replanned || self.uploaded.get(index).copied().flatten() != Some(page.identity()))
                    .then_some(index)
            })
            .collect())
    }

    /// Moves residency onto `plan` after bucket reallocation. A page stays resident only
    /// when its old bucket was kept as its new bucket at the same layer; `kept` maps old
    /// bucket indices to the new index that retained the same texture.
    fn rebind(&mut self, plan: &UiTexturePlan, kept: &[Option<usize>]) {
        let old = self.plan.as_ref().map_or(&[][..], UiTexturePlan::locations);
        self.uploaded.resize(plan.locations().len(), None);
        for (index, uploaded) in self.uploaded.iter_mut().enumerate() {
            let retained = old.get(index).is_some_and(|old| {
                let new = plan.locations()[index];
                kept.get(old.bucket).copied().flatten() == Some(new.bucket)
                    && old.layer == new.layer
            });
            if !retained {
                *uploaded = None;
            }
        }
        self.plan = Some(plan.clone());
    }

    /// Marks pages whose writes the device may not have completed.
    fn forget(&mut self, pages: &[usize]) {
        for &index in pages {
            if let Some(uploaded) = self.uploaded.get_mut(index) {
                *uploaded = None;
            }
        }
    }

    /// The executor is shared by device preparation and deterministic recording
    /// tests. Only successful issuance of all supplied writes commits their content IDs.
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
                catalog.resident_plan().locations()[index],
            )?;
        }
        if self.plan.as_ref() != Some(catalog.resident_plan()) {
            self.uploaded.clear();
            self.plan = Some(catalog.resident_plan().clone());
        }
        self.uploaded.resize(catalog.pages().len(), None);
        for &index in dirty {
            self.uploaded[index] = Some(catalog.pages()[index].identity());
        }
        self.static_identity = Some(catalog.static_identity());
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct UiGpuTextures {
    pub(super) fonts: render_model::FontAtlasFrame,
    pub(super) buckets: Vec<Option<GpuBucket>>,
    pub(super) state: TextureUploadState,
    pub(super) locations: Vec<UiTextureLocation>,
    pub(super) bytes: usize,
    allocation_identity: Option<[u8; 32]>,
    allocation_plan: Option<UiTexturePlan>,
    retry: Option<(Duration, u64)>,
    pub(super) residency_generation: u64,
}

impl UiGpuTextures {
    /// Reports whether the requested page has successfully uploaded storage.
    pub(super) fn page_resident(&self, page: usize) -> bool {
        self.state.uploaded.get(page).is_some_and(Option::is_some)
            && self.locations.get(page).is_some_and(|location| {
                self.buckets
                    .get(location.bucket)
                    .is_some_and(Option::is_some)
            })
    }

    /// Invalidates failed glyph writes and delays another attempt on the same plan.
    pub(super) fn failed_font_uploads(&mut self, pages: &[usize], bytes: u64, now: Duration) {
        self.state.forget(pages);
        self.fonts.clear();
        self.retry = Some((now.saturating_add(TEXTURE_RETRY_DELAY), bytes));
        self.residency_generation = self.residency_generation.wrapping_add(1);
    }

    /// Resolves a drawable page's binding, leaving failed pages absent.
    pub(super) fn page_binding(&self, page: usize) -> Option<&BindGroup> {
        if !self.page_resident(page) {
            return None;
        }
        self.buckets[self.locations[page].bucket]
            .as_ref()?
            .bind_group
            .as_ref()
    }

    pub(super) fn allocated_buckets(&self) -> &[render_model::UiTextureBucket] {
        self.allocation_plan
            .as_ref()
            .map_or(&[], |plan| plan.buckets())
    }
    pub(super) fn resident(&self, catalog: &UiTextureCatalog) -> bool {
        self.allocation_identity == Some(catalog.static_identity())
            && self.allocation_plan.as_ref() == Some(catalog.resident_plan())
            && self.buckets.len() == catalog.resident_plan().buckets().len()
            && self.buckets.iter().all(Option::is_some)
            && self.locations == catalog.resident_plan().locations()
            && self.state.uploaded.len() == catalog.pages().len()
            && self
                .state
                .uploaded
                .iter()
                .zip(catalog.pages())
                .all(|(id, page)| *id == Some(page.identity()))
    }
    pub(super) fn prepare(
        &mut self,
        catalog: &UiTextureCatalog,
        device: &RenderDevice,
        queue: &RenderQueue,
        profile: Option<&super::profile::UiProfile>,
        now: Duration,
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
        // Unused reserved pages share a layer: allocation follows the resident plan.
        let replanned = self.allocation_plan.as_ref() != Some(catalog.resident_plan());
        if self.allocation_identity.is_some()
            && !replanned
            && (self.buckets.len() != catalog.resident_plan().buckets().len()
                || self.locations != catalog.resident_plan().locations())
        {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        for format in [TextureFormat::Rgba8Unorm, TextureFormat::R8Unorm] {
            let format = format.guaranteed_format_features(device.features());
            if !format
                .allowed_usages
                .contains(TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST)
                || !format
                    .flags
                    .contains(wgpu::TextureFormatFeatureFlags::FILTERABLE)
            {
                return Err(UiRenderRejectReason::InvalidTextureExtent);
            }
        }
        // All catalog and per-device admission checks precede allocation/writes.
        if !replanned {
            let dirty = self.state.dirty(catalog)?;
            if dirty
                .iter()
                .any(|&page| self.state.uploaded.get(page).is_some_and(Option::is_some))
            {
                self.residency_generation = self.residency_generation.wrapping_add(1);
            }
            self.state.forget(&dirty);
            if let Some((retry_at, bytes)) = self.retry
                && now < retry_at
            {
                return Err(UiRenderRejectReason::TextureAllocationFailed { bytes });
            }
        }
        let mut failed_bytes = 0;
        if replanned || self.buckets.iter().any(Option::is_none) {
            failed_bytes += self.reallocate(catalog, device);
        }
        let dirty = self.state.dirty(catalog)?;
        if dirty
            .iter()
            .any(|&page| catalog.pages()[page].font_atlas_side().is_some())
        {
            self.fonts.clear();
        }
        for (index, bucket) in self.buckets.iter().enumerate() {
            let Some(bucket) = bucket else {
                continue;
            };
            let pages = dirty
                .iter()
                .copied()
                .filter(|&page| self.locations[page].bucket == index)
                .collect::<Vec<_>>();
            if pages.is_empty() {
                continue;
            }
            let bytes = pages
                .iter()
                .map(|&page| catalog.pages()[page].pixels().len() as u64)
                .sum();
            let state = &mut self.state;
            let issued = gpu_allocation::checked(device, "UI texture page upload", bytes, || {
                state.execute(catalog, &pages, |_, page, location| {
                    queue.profiled_write_buffer(
                        &bucket.format_uniform,
                        render_model::ui_page_uniform::offset(location.layer),
                        bytemuck::cast_slice(&render_model::ui_page_uniform::region(page)),
                    );
                    write_page(queue, &bucket.texture, page, location, profile);
                    Ok::<(), UiRenderRejectReason>(())
                })
            });
            if issued.is_none() {
                self.state.forget(&pages);
                failed_bytes += bytes;
            } else {
                self.residency_generation = self.residency_generation.wrapping_add(1);
            }
        }
        self.retry =
            (failed_bytes > 0).then(|| (now.saturating_add(TEXTURE_RETRY_DELAY), failed_bytes));
        if failed_bytes > 0 {
            Err(UiRenderRejectReason::TextureAllocationFailed {
                bytes: failed_bytes,
            })
        } else {
            Ok(())
        }
    }

    /// Keeps unchanged buckets and retries missing storage after freeing superseded
    /// buckets on the first failure. Returns the bytes still missing from the new plan.
    fn reallocate(&mut self, catalog: &UiTextureCatalog, device: &RenderDevice) -> u64 {
        let plan = catalog.resident_plan();
        let old = self
            .allocation_plan
            .as_ref()
            .map_or(&[][..], UiTexturePlan::buckets);
        let mut previous = std::mem::take(&mut self.buckets);
        let mut kept = vec![None; previous.len()];
        for (index, bucket) in plan.buckets().iter().enumerate() {
            let source =
                (0..previous.len()).find(|&i| previous[i].is_some() && old.get(i) == Some(bucket));
            self.buckets.push(source.and_then(|old| {
                kept[old] = Some(index);
                previous[old].take()
            }));
        }
        // Moving the plan invalidates pages in all superseded or missing buckets.
        self.state.rebind(plan, &kept);
        self.residency_generation = self.residency_generation.wrapping_add(1);
        self.fonts.clear();
        self.allocation_identity = Some(catalog.static_identity());
        self.allocation_plan = Some(plan.clone());
        self.locations = plan.locations().to_vec();
        let mut failed_bytes = 0;
        for (index, bucket) in plan.buckets().iter().enumerate() {
            if self.buckets[index].is_some() {
                continue;
            }
            let mut created = allocate_bucket(device, bucket);
            if created.is_none() && previous.iter().any(Option::is_some) {
                // Free GPU storage even if an earlier submitted frame still owns a view.
                for old in previous.iter().flatten() {
                    old.texture.destroy();
                }
                previous.clear();
                created = allocate_bucket(device, bucket);
            }
            if created.is_none() {
                failed_bytes += bucket_bytes(bucket);
            }
            self.buckets[index] = created;
        }
        self.bytes = plan
            .buckets()
            .iter()
            .zip(&self.buckets)
            .filter(|(_, bucket)| bucket.is_some())
            .map(|(bucket, _)| bucket_bytes(bucket) as usize)
            .sum();
        failed_bytes
    }
}

/// Counts the storage requested by a validated dimension bucket.
fn bucket_bytes(bucket: &render_model::UiTextureBucket) -> u64 {
    u64::from(bucket.dimensions[0])
        * u64::from(bucket.dimensions[1])
        * u64::from(bucket.layers)
        * bucket.format.bytes_per_texel() as u64
}

/// Creates bindings only after a bucket's texture has passed its allocation scope.
fn allocate_bucket(
    device: &RenderDevice,
    bucket: &render_model::UiTextureBucket,
) -> Option<GpuBucket> {
    let bytes = bucket_bytes(bucket);
    let texture = gpu_allocation::checked(device, "bounded UI dimension bucket", bytes, || {
        create_texture(device, bucket)
    })?;
    gpu_allocation::checked(device, "UI bucket bindings", bytes, || {
        bind_texture(device, bucket, texture)
    })
}

/// Creates one bucket's array texture.
fn create_texture(device: &RenderDevice, bucket: &render_model::UiTextureBucket) -> Texture {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!(
        "ui.texture_allocate",
        width = bucket.dimensions[0],
        height = bucket.dimensions[1],
        layers = bucket.layers,
    )
    .entered();
    device.create_texture(&TextureDescriptor {
        label: Some("bounded UI dimension bucket"),
        size: Extent3d {
            width: bucket.dimensions[0],
            height: bucket.dimensions[1],
            depth_or_array_layers: bucket.layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: match bucket.format {
            UiTextureFormat::Rgba8 => TextureFormat::Rgba8Unorm,
            UiTextureFormat::Coverage => TextureFormat::R8Unorm,
        },
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Wraps a successfully created bucket texture with its view and page-format uniform.
fn bind_texture(
    device: &RenderDevice,
    bucket: &render_model::UiTextureBucket,
    texture: Texture,
) -> GpuBucket {
    let view = texture.create_view(&TextureViewDescriptor {
        label: Some("bounded UI dimension bucket view"),
        dimension: Some(TextureViewDimension::D2Array),
        ..Default::default()
    });
    let format = render_model::ui_page_uniform::words(bucket.format == UiTextureFormat::Coverage);
    let format_uniform =
        device.create_buffer_with_data(&bevy::render::render_resource::BufferInitDescriptor {
            label: Some("UI bucket page format"),
            contents: bytemuck::cast_slice(&format),
            usage: bevy::render::render_resource::BufferUsages::UNIFORM
                | bevy::render::render_resource::BufferUsages::COPY_DST,
        });
    GpuBucket {
        texture,
        view,
        format_uniform,
        bind_group: None,
        #[cfg(test)]
        allocation_lifetime: std::sync::Arc::new(()),
    }
}

/// Writes one whole non-font page to its bucket layer.
fn write_page(
    queue: &RenderQueue,
    texture: &Texture,
    page: &UiTexturePage,
    location: UiTextureLocation,
    profile: Option<&super::profile::UiProfile>,
) {
    if page.font_atlas_side().is_some() {
        return;
    }
    let [width, height] = page.dimensions();
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!(
        "ui.texture_write",
        bucket = location.bucket,
        layer = location.layer,
        width,
        height,
        bytes = page.pixels().len(),
    )
    .entered();
    if let Some(profile) = profile {
        profile.record_upload(
            super::profile::UploadKind::Texture,
            page.pixels().len() as u64,
        );
    }
    queue.profiled_write_texture(
        TexelCopyTextureInfo {
            texture,
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
            bytes_per_row: Some(width * page.format().bytes_per_texel() as u32),
            rows_per_image: Some(height),
        },
        Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
    );
}

/// Bind each page bucket with the viewport and both samplers once the frame is accepted.
pub(super) fn prepare_ui_bind_group(
    render_device: Res<RenderDevice>,
    render_queue: Res<RenderQueue>,
    (item_glint, profile): (
        Option<Res<render_scene::UiItemGlintTexture>>,
        Option<Res<super::profile::UiProfile>>,
    ),
    pipeline_cache: Res<PipelineCache>,
    pipeline: Res<UiPipeline>,
    mut gpu: ResMut<UiGpu>,
    stats: Res<render_scene::UiRenderStatsResource>,
) {
    if gpu.accepted_revision.is_none() || &gpu.device != render_device.wgpu_device() {
        return;
    }
    let changed = gpu.item_glint.prepare(
        &render_device,
        &render_queue,
        item_glint
            .as_ref()
            .and_then(|resource| resource.0.as_deref()),
        profile.as_deref(),
    );
    if changed {
        stats.update(|stats| {
            stats.retained_gpu_bytes = super::resources::retained_gpu_bytes(
                gpu.vertex_capacity,
                gpu.index_capacity,
                gpu.textures.bytes + gpu.item_glint.bytes,
            );
        });
    }
    // Disjoint field borrows let unchanged frames skip cloning the shared bindings.
    let UiGpu {
        viewport_buffer: viewport,
        sampler,
        linear_sampler,
        item_glint,
        textures,
        ..
    } = &mut *gpu;
    let (viewport, sampler, linear_sampler) = (&*viewport, &*sampler, &*linear_sampler);
    let (glint_view, glint_sampler) = (&item_glint.view, &item_glint.sampler);
    for bucket in textures.buckets.iter_mut().flatten() {
        if bucket.bind_group.is_some() && !changed {
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
                    resource: BindingResource::Sampler(sampler),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(linear_sampler),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: bucket.format_uniform.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: BindingResource::TextureView(glint_view),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: BindingResource::Sampler(glint_sampler),
                },
            ],
        ));
    }
}

#[cfg(test)]
#[path = "textures_tests.rs"]
mod tests;
