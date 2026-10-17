//! Retained dimension buckets and transactional dirty-page write planning.

use crate::{gpu_allocation, queue_writes::ProfiledQueue as _};
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

use super::{UiGpu, pipeline::UiPipeline};
use render_model::{
    UiTextureCatalog, UiTextureFormat, UiTextureLocation, UiTexturePage, UiTexturePlan,
};

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
}

#[derive(Default)]
pub(super) struct TextureUploadState {
    static_identity: Option<[u8; 32]>,
    /// Plan whose locations the resident identities below occupy.
    plan: Option<UiTexturePlan>,
    /// Per page, the content identity resident at its location in `plan`.
    uploaded: Vec<Option<[u8; 32]>>,
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
        let replanned = self.plan.as_ref() != Some(catalog.plan());
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
            .extend(catalog.pages().iter().map(|page| Some(page.identity())));
        self.static_identity = Some(catalog.static_identity());
        if self.plan.as_ref() != Some(catalog.plan()) {
            self.plan = Some(catalog.plan().clone());
        }
        Ok(())
    }
}

#[derive(Default)]
pub(super) struct UiGpuTextures {
    pub(super) fonts: render_model::FontAtlasFrame,
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
                .all(|(id, page)| *id == Some(page.identity()))
    }
    pub(super) fn prepare(
        &mut self,
        catalog: &UiTextureCatalog,
        device: &RenderDevice,
        queue: &RenderQueue,
        profile: Option<&super::profile::UiProfile>,
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
        let replanned = self.allocation_plan.as_ref() != Some(catalog.plan());
        if self.allocation_identity.is_some()
            && !replanned
            && (self.buckets.len() != catalog.plan().buckets().len()
                || self.locations != catalog.plan().locations())
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
        if replanned {
            self.reallocate(catalog, device)?;
        }
        if self.buckets.len() != catalog.plan().buckets().len() {
            return Err(UiRenderRejectReason::InvalidTextureExtent);
        }
        let dirty = self.state.dirty(catalog)?;
        if dirty.is_empty() {
            return Ok(());
        }
        let bytes = dirty
            .iter()
            .map(|&index| catalog.pages()[index].pixels().len() as u64)
            .sum();
        let buckets = &self.buckets;
        let state = &mut self.state;
        let issued = gpu_allocation::checked(device, "UI texture page upload", bytes, || {
            state.execute(catalog, &dirty, |_, page, location| {
                write_page(
                    queue,
                    &buckets[location.bucket].texture,
                    page,
                    location,
                    profile,
                );
                Ok::<(), UiRenderRejectReason>(())
            })
        });
        issued.unwrap_or_else(|| {
            self.state.forget(&dirty);
            Err(UiRenderRejectReason::TextureAllocationFailed { bytes })
        })
    }

    /// Replaces the buckets for a new plan, keeping every resident bucket whose extent,
    /// format and layer count are unchanged so a resized page reallocates only the
    /// buckets it moves between. Out-of-memory leaves the previous buckets resident.
    fn reallocate(
        &mut self,
        catalog: &UiTextureCatalog,
        device: &RenderDevice,
    ) -> Result<(), UiRenderRejectReason> {
        let plan = catalog.plan();
        let old = self
            .allocation_plan
            .as_ref()
            .map_or(&[][..], UiTexturePlan::buckets);
        let mut claimed = vec![false; self.buckets.len().min(old.len())];
        let sources = plan
            .buckets()
            .iter()
            .map(|bucket| {
                let index = (0..claimed.len()).find(|&i| !claimed[i] && old[i] == *bucket)?;
                claimed[index] = true;
                Some(index)
            })
            .collect::<Vec<_>>();
        let fresh = plan
            .buckets()
            .iter()
            .zip(&sources)
            .filter_map(|(bucket, source)| source.is_none().then_some(bucket))
            .collect::<Vec<_>>();
        let bytes = fresh
            .iter()
            .map(|bucket| {
                u64::from(bucket.dimensions[0])
                    * u64::from(bucket.dimensions[1])
                    * u64::from(bucket.layers)
                    * bucket.format.bytes_per_texel() as u64
            })
            .sum();
        // Views and bindings are only made once every texture exists, since using an
        // invalid texture raises an uncaptured, fatal validation error.
        let textures =
            gpu_allocation::checked(device, "bounded UI dimension bucket", bytes, || {
                fresh
                    .iter()
                    .map(|bucket| create_texture(device, bucket))
                    .collect::<Vec<_>>()
            })
            .ok_or(UiRenderRejectReason::TextureAllocationFailed { bytes })?;
        let created = gpu_allocation::checked(device, "UI bucket bindings", 0, || {
            fresh
                .iter()
                .zip(textures)
                .map(|(bucket, texture)| bind_texture(device, bucket, texture))
                .collect::<Vec<_>>()
        })
        .ok_or(UiRenderRejectReason::TextureAllocationFailed { bytes })?;
        let mut previous = std::mem::take(&mut self.buckets)
            .into_iter()
            .map(Some)
            .collect::<Vec<_>>();
        let mut kept = vec![None; previous.len()];
        let mut created = created.into_iter();
        for (index, source) in sources.iter().enumerate() {
            let bucket = match *source {
                Some(old) => {
                    kept[old] = Some(index);
                    previous[old].take()
                }
                None => created.next(),
            };
            self.buckets
                .push(bucket.expect("each planned bucket is kept or created once"));
        }
        self.state.rebind(plan, &kept);
        // Glyph residency is cheap to rebuild and may refer to moved font pages.
        self.fonts.clear();
        self.allocation_identity = Some(catalog.static_identity());
        self.allocation_plan = Some(plan.clone());
        self.locations = plan.locations().to_vec();
        self.bytes = plan.bytes();
        Ok(())
    }
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
    let coverage = u32::from(bucket.format == UiTextureFormat::Coverage);
    let format_uniform =
        device.create_buffer_with_data(&bevy::render::render_resource::BufferInitDescriptor {
            label: Some("UI bucket page format"),
            contents: bytemuck::cast_slice(&[coverage, 0, 0, 0]),
            usage: bevy::render::render_resource::BufferUsages::UNIFORM,
        });
    GpuBucket {
        texture,
        view,
        format_uniform,
        bind_group: None,
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
    for bucket in &mut textures.buckets {
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
