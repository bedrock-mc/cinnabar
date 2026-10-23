//! Slot-owned artwork uploads current demand first and prewarms unused pages, within a
//! per-frame upload budget.
use super::*;
use std::{collections::hash_map::RandomState, sync::Arc};
use {
    crate::actor::gpu::ActorDrawSpan,
    render_model::{
        ActorArtworkPageId, ActorArtworkPages, MAX_ACTOR_GPU_PIXEL_BYTES, MAX_ACTOR_TEXTURE_PAGES,
    },
};

#[path = "artwork_source.rs"]
mod source;
#[path = "artwork_upload.rs"]
mod upload;
use crate::actor::gpu::ArtworkRawViews;
use crate::gpu_allocation;
use source::TextureKey;
use upload::{PendingPage, Progress, UPLOAD_BYTES_PER_FRAME};

pub(super) const PAGES_PER_FRAME: usize = 4;

pub(super) struct GpuArtworkPage {
    source: TextureKey,
    _texture: Texture,
    pub view: TextureView,
    pub bind_group: Option<BindGroup>,
    pub color_mask: bool,
    pub multitexture: bool,
    raw_views: ArtworkRawViews,
}

#[cfg(feature = "shaderpacks")]
impl GpuArtworkPage {
    /// The page's array texture, for single-layer raw views; `None` until it is recreated to
    /// allow them.
    pub fn raw_texture(&self) -> Option<&Texture> {
        self.raw_views.0.then_some(&self._texture)
    }
}

/// One immutable glint raster, replaced only when its pixels change.
#[derive(Default)]
pub(super) struct GpuGlint {
    pub texture: Option<(Texture, TextureView)>,
    source: Option<TextureKey>,
    bytes: usize,
    raw_views: ArtworkRawViews,
}

impl GpuGlint {
    /// Uploads `raster` (or a transparent texel without one); `true` when the texture changed.
    fn prepare(
        &mut self,
        raster: Option<&render_model::EquipmentRaster>,
        hasher: &RandomState,
        device: &RenderDevice,
        queue: &RenderQueue,
        label: &'static str,
        raw_views: ArtworkRawViews,
    ) -> bool {
        let fallback = render_model::EquipmentRaster {
            width: 1,
            height: 1,
            rgba8: Arc::from([0u8; 4]),
        };
        let source = raster.unwrap_or(&fallback);
        let page = render_model::ActorTexturePage {
            width: source.width,
            height: source.height,
            layers: 1,
            rgba8: Arc::clone(&source.rgba8),
            color_mask: false,
            multitexture: false,
        };
        let key = self
            .source
            .as_ref()
            .filter(|key| key.shares_pixels_with(&page))
            .cloned()
            .unwrap_or_else(|| TextureKey::new(&page, hasher));
        let changed = self.source.as_ref() != Some(&key) || self.raw_views != raw_views;
        if !changed {
            return false;
        }
        let Some(texture) = upload_or_reduce(
            device,
            queue,
            &page,
            label,
            self.texture.is_none(),
            raw_views,
        ) else {
            return false;
        };
        let Some(view) = gpu_allocation::checked(device, "actor glint view", 0, || {
            texture.create_view(&TextureViewDescriptor::default())
        }) else {
            return false;
        };
        self.bytes = (texture.width() as usize) * (texture.height() as usize) * 4;
        self.texture = Some((texture, view));
        self.source = Some(key);
        self.raw_views = raw_views;
        changed
    }

    /// The glint texture for raw views; `None` until it is recreated to allow them.
    #[cfg(feature = "shaderpacks")]
    pub fn raw_texture(&self) -> Option<&Texture> {
        self.texture
            .as_ref()
            .filter(|_| self.raw_views.0)
            .map(|(texture, _)| texture)
    }
}

#[derive(Default)]
pub(super) struct GpuArtwork {
    identity: Option<([u8; 32], [u8; 32])>,
    pub pages: Vec<Option<GpuArtworkPage>>,
    /// Replacements still writing rows; the slot's previous page draws until one completes.
    pending: Vec<Option<PendingPage>>,
    prewarm_cursor: usize,
    pub glint: GpuGlint,
    /// The held-item glint, sampled when an instance selects the item raster.
    pub item_glint: GpuGlint,
    source_hasher: RandomState,
    /// Device limits reject the publication until its identity changes.
    rejected: bool,
    /// A demanded page that could not allocate retries after a bounded backoff.
    allocation: gpu_allocation::UploadRetry,
    /// The raw-view setting pages and glints are created with.
    raw_views: ArtworkRawViews,
}

impl GpuArtwork {
    /// Restarts pending uploads and recreates pages and glints when their view formats change.
    /// Completed pages keep drawing natively until their replacements finish uploading.
    pub fn set_raw_views(&mut self, raw_views: ArtworkRawViews) {
        if self.raw_views != raw_views {
            self.raw_views = raw_views;
            self.identity = None;
            self.pending.fill_with(|| None);
        }
    }

    /// Uploads current demand first, then prewarms unused pages, writing at most one frame's
    /// upload budget; a page binds once all of its rows are written.
    pub fn prepare_demanded(
        &mut self,
        pages: &ActorArtworkPages,
        demanded: &[ActorArtworkPageId],
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> bool {
        self.prepare_pages(
            pages,
            demanded.iter().copied(),
            device,
            queue,
            UPLOAD_BYTES_PER_FRAME,
        )
    }

    /// Exercises complete publication through the same path without a frame work budget.
    #[cfg(test)]
    pub fn prepare(
        &mut self,
        pages: &ActorArtworkPages,
        device: &RenderDevice,
        queue: &RenderQueue,
    ) -> bool {
        self.prepare_pages(
            pages,
            (1..=pages.pages.len()).map(|page| page as ActorArtworkPageId),
            device,
            queue,
            usize::MAX,
        )
    }

    /// Retains each slot independently; `budget` bounds the bytes written this call.
    fn prepare_pages(
        &mut self,
        pages: &ActorArtworkPages,
        demanded: impl Iterator<Item = ActorArtworkPageId>,
        device: &RenderDevice,
        queue: &RenderQueue,
        budget: usize,
    ) -> bool {
        let raw_views = self.raw_views;
        let identity = (pages.identity, pages.entity_identity);
        let changed = self.identity != Some(identity);
        if changed {
            self.identity = Some(identity);
            self.rejected = false;
            self.allocation = gpu_allocation::UploadRetry::default();
            let actor_changed = self.glint.prepare(
                pages.actor_glint.as_ref(),
                &self.source_hasher,
                device,
                queue,
                "immutable actor glint",
                raw_views,
            );
            let item_changed = self.item_glint.prepare(
                pages.item_glint.as_ref(),
                &self.source_hasher,
                device,
                queue,
                "immutable item glint",
                raw_views,
            );
            if actor_changed || item_changed {
                self.invalidate_bindings();
            }
            let bytes = pages.pages.iter().try_fold(
                render_model::PLAYER_SKIN_BUDGET_BYTES + self.glint.bytes + self.item_glint.bytes,
                |total, page| total.checked_add(page.rgba8.len()),
            );
            if pages.pages.len() + 1 > MAX_ACTOR_TEXTURE_PAGES
                || bytes.is_none_or(|bytes| bytes > MAX_ACTOR_GPU_PIXEL_BYTES)
                || pages
                    .pages
                    .iter()
                    .any(|page| page.layers > device.limits().max_texture_array_layers)
            {
                self.pages.clear();
                self.pending.clear();
                self.rejected = true;
                bevy::log::warn!(
                    "neutral actor artwork exceeds device limits; generic artwork unavailable"
                );
                return false;
            }
            self.pages.truncate(pages.pages.len());
            self.pages.resize_with(pages.pages.len(), || None);
            self.pending.truncate(pages.pages.len());
            self.pending.resize_with(pages.pages.len(), || None);
            self.prewarm_cursor = 0;
            for (index, page) in pages.pages.iter().enumerate() {
                if self.pages[index]
                    .as_ref()
                    .is_some_and(|gpu| !gpu.source.contains_layers(page.layers))
                {
                    self.pages[index] = None;
                }
                if let Some(gpu) = &mut self.pages[index]
                    && (gpu.color_mask != page.color_mask || gpu.multitexture != page.multitexture)
                {
                    gpu.bind_group = None;
                    gpu.color_mask = page.color_mask;
                    gpu.multitexture = page.multitexture;
                }
                if let Some(pending) = &mut self.pending[index] {
                    if !pending.source.shares_pixels_with(page) {
                        let source = TextureKey::new(page, &self.source_hasher);
                        if pending.source != source {
                            self.pending[index] = None;
                            continue;
                        }
                        pending.source = source;
                    }
                    pending.color_mask = page.color_mask;
                    pending.multitexture = page.multitexture;
                }
            }
        }
        if self.rejected {
            return false;
        }
        if self.allocation.failed() {
            if !self.allocation.take_ready() {
                return false;
            }
            self.pages.resize_with(pages.pages.len(), || None);
            self.pending.resize_with(pages.pages.len(), || None);
            self.prewarm_cursor = 0;
        }
        let demanded: Vec<_> = demanded
            .filter_map(|id| usize::from(id).checked_sub(1))
            .filter(|&index| index < pages.pages.len())
            .collect();
        let pending: Vec<_> = self
            .pending
            .iter()
            .enumerate()
            .filter_map(|(index, page)| page.as_ref().map(|_| index))
            .collect();
        let mut prepared = 0;
        let mut budget = budget;
        let mut attempted = std::collections::HashSet::new();
        let first_background = self.prewarm_cursor;
        let requests = demanded
            .iter()
            .chain(&pending)
            .map(|&index| (index, false))
            .chain((first_background..pages.pages.len()).map(|index| (index, true)));
        for (index, background) in requests {
            let page = &pages.pages[index];
            let current = self.pages[index].as_ref().is_some_and(|gpu| {
                gpu.source.shares_pixels_with(page) && gpu.raw_views == raw_views
            });
            if current || attempted.contains(&index) {
                if background {
                    self.prewarm_cursor = index + 1;
                }
                continue;
            }
            // Finish pending pages before allocating more speculative textures.
            if background
                && (prepared == PAGES_PER_FRAME || self.pending.iter().any(Option::is_some))
            {
                break;
            }
            if budget < usize::from(page.width) * 4 {
                break;
            }
            if background {
                prepared += 1;
                self.prewarm_cursor = index + 1;
            }
            attempted.insert(index);
            if !self.advance_page(index, page, device, queue, &mut budget)
                && demanded.contains(&index)
            {
                return self.allocation_failed(queue);
            }
        }
        if self.allocation.failed() {
            self.allocation.record(true, None);
        }
        true
    }

    /// Advances one slot before allocating another; failures keep a compatible old page.
    fn advance_page(
        &mut self,
        index: usize,
        page: &render_model::ActorTexturePage,
        device: &RenderDevice,
        queue: &RenderQueue,
        budget: &mut usize,
    ) -> bool {
        if self.pending[index].is_none() {
            let source = TextureKey::new(page, &self.source_hasher);
            if let Some(gpu) = &mut self.pages[index]
                && gpu.source == source
                && gpu.raw_views == self.raw_views
            {
                gpu.source = source;
                return true;
            }
            self.pending[index] = upload::start_page(
                device,
                page,
                source,
                self.pages[index].is_none(),
                self.raw_views,
            );
        }
        let Some(pending) = self.pending[index].take() else {
            return self.pages[index].is_some();
        };
        match upload::continue_page(device, queue, pending, budget) {
            Progress::Done(page) => self.pages[index] = Some(page),
            Progress::Pending(pending) => self.pending[index] = Some(pending),
            Progress::Failed => return self.pages[index].is_some(),
        }
        true
    }

    /// Releases every page so a later retry has the most memory, and backs off before it.
    fn allocation_failed(&mut self, queue: &RenderQueue) -> bool {
        let released =
            self.pages.iter().any(Option::is_some) || self.pending.iter().any(Option::is_some);
        self.pages.clear();
        self.pending.clear();
        self.allocation.record(false, released.then_some(queue));
        false
    }

    /// Returns a page once every row is written; demand still uploading draws nothing yet.
    pub(super) fn page(&self, index: usize) -> Option<&GpuArtworkPage> {
        self.pages.get(index).and_then(Option::as_ref)
    }

    /// Reports whether this page binds the exact immutable pixels supplied by the publication.
    #[cfg(feature = "publication-test-support")]
    pub(super) fn page_is_bound(
        &self,
        page: ActorArtworkPageId,
        source: &render_model::ActorTexturePage,
    ) -> bool {
        self.page(usize::from(page) - 1)
            .is_some_and(|page| page.bind_group.is_some() && page.source.shares_pixels_with(source))
    }

    /// Buffer and glint replacements invalidate bindings without discarding page textures.
    pub fn invalidate_bindings(&mut self) {
        for page in self.pages.iter_mut().flatten() {
            page.bind_group = None;
        }
    }
}

/// Uploads a glint whole, retrying fresh ones at half size until one texel remains.
fn upload_or_reduce(
    device: &RenderDevice,
    queue: &RenderQueue,
    page: &render_model::ActorTexturePage,
    label: &'static str,
    reduce: bool,
    raw_views: ArtworkRawViews,
) -> Option<Texture> {
    let mut fitted = page
        .fit_within(device.limits().max_texture_dimension_2d)
        .into_owned();
    loop {
        if let Some(texture) = upload_page(device, queue, &fitted, label, raw_views) {
            return Some(texture);
        }
        let side = u32::from(fitted.width.max(fitted.height));
        if !reduce || side <= 1 {
            return None;
        }
        fitted = fitted.fit_within((side / 2).max(1)).into_owned();
    }
}

/// Checks texture creation before writing pixels, and checks staging before publishing it.
fn upload_page(
    device: &RenderDevice,
    queue: &RenderQueue,
    page: &render_model::ActorTexturePage,
    label: &'static str,
    raw_views: ArtworkRawViews,
) -> Option<Texture> {
    #[cfg(feature = "tracy")]
    let _span = bevy::log::info_span!(
        "actor.artwork_upload",
        label,
        width = page.width,
        height = page.height,
        layers = page.layers,
        bytes = page.rgba8.len()
    )
    .entered();
    let size = Extent3d {
        width: u32::from(page.width),
        height: u32::from(page.height),
        depth_or_array_layers: page.layers,
    };
    let texture = gpu_allocation::checked(device, label, page.rgba8.len() as u64, || {
        device.create_texture(&TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: raw_views.view_formats(),
        })
    })?;
    #[cfg(test)]
    work_tests::record_upload(page.rgba8.len());
    gpu_allocation::checked(device, "actor page upload", page.rgba8.len() as u64, || {
        queue.profiled_write_texture(
            TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: default(),
                aspect: default(),
            },
            &page.rgba8,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(size.width * 4),
                rows_per_image: Some(size.height),
            },
            size,
        )
    })?;
    Some(texture)
}

/// Reuses span storage for adjacent opaque instances with equal vertex counts, pages,
/// materials and layers. Sorted-pass instances retain individual sort positions.
pub(super) fn draw_spans(
    spans: &mut Vec<ActorDrawSpan>,
    pages: &[ActorArtworkPageId],
    instances: &[render_model::ActorGpuInstance],
    geometry: &[render_model::ActorRigGeometrySpan],
    layers: impl IntoIterator<Item = u8>,
) {
    spans.clear();
    let mut last_layer = None;
    for (index, ((page, instance), layer)) in
        pages.iter().copied().zip(instances).zip(layers).enumerate()
    {
        let vertex_count = geometry
            .get(instance.geometry_id as usize)
            .map_or(0, |span| span.vertex_count);
        if let Some(span) = spans.last_mut().filter(|span| {
            span.page == page
                && !super::phase::sorted(instance.material)
                && span.vertex_count == vertex_count
                && span.material == instance.material
                && last_layer == Some(layer)
        }) {
            span.count += 1;
        } else {
            spans.push(ActorDrawSpan {
                material: instance.material,
                page,
                first: index as u32,
                count: 1,
                vertex_count,
            });
            last_layer = Some(layer);
        }
    }
}

/// Splits main-pass spans at coverage-only actors; mask preparation retains the complete spans.
pub(super) fn visible_spans(
    spans: &[ActorDrawSpan],
    manifest: &[render_model::ActorDrawManifestEntry],
    hidden: &[u64],
    main_count: Option<u32>,
) -> Vec<ActorDrawSpan> {
    let mut visible = Vec::with_capacity(spans.len());
    for span in spans {
        let mut run = None;
        for first in span.first..span.first + span.count {
            if main_count.map_or_else(
                || hidden.contains(&manifest[first as usize].identity.runtime_id),
                |count| first >= count,
            ) {
                if let Some(first_run) = run.take() {
                    visible.push(ActorDrawSpan {
                        first: first_run,
                        count: first - first_run,
                        ..*span
                    });
                }
            } else {
                run.get_or_insert(first);
            }
        }
        if let Some(first) = run {
            visible.push(ActorDrawSpan {
                first,
                count: span.first + span.count - first,
                ..*span
            });
        }
    }
    visible
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_depth_instances_keep_separate_sort_positions() {
        let material =
            assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
                depth_always: true,
                ..Default::default()
            }));
        let instances = [render_model::ActorGpuInstance {
            material,
            ..Default::default()
        }; 2];
        let mut spans = Vec::new();
        draw_spans(
            &mut spans,
            &[1; 2],
            &instances,
            &[render_model::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 36,
            }],
            std::iter::repeat(0),
        );
        assert_eq!(
            spans
                .iter()
                .map(|span| (span.first, span.count))
                .collect::<Vec<_>>(),
            [(0, 1), (1, 1)]
        );
    }

    #[test]
    fn blended_instances_keep_separate_sortable_spans_while_opaque_instances_batch() {
        let material = |blend| {
            assets::EntityRenderMaterial::Default.word(Some(assets::EntityRenderMaterialState {
                blend,
                ..Default::default()
            }))
        };
        let instances = [false, false, true, true].map(|blend| render_model::ActorGpuInstance {
            material: material(blend),
            ..Default::default()
        });
        let mut spans = Vec::new();
        draw_spans(
            &mut spans,
            &[1; 4],
            &instances,
            &[render_model::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 36,
            }],
            std::iter::repeat(0),
        );
        assert_eq!(
            spans
                .iter()
                .map(|span| (span.first, span.count))
                .collect::<Vec<_>>(),
            [(0, 2), (2, 1), (3, 1)]
        );
        assert!(spans.iter().all(|span| span.vertex_count == 36));
    }

    #[test]
    fn high_artwork_page_ids_survive_paged_build_and_draw_spans() {
        use render_model::{
            ActorRenderIdentity, ActorRigFrameBuilder, ActorRigRenderInput, ActorRigRoute,
            ActorRigSubmission,
        };
        let textures = (1..=u16::from(u8::MAX) + 2)
            .map(|height| assets::ActorTexture {
                source: u32::from(height),
                width: 1,
                height,
                pixel_sha256: [1; 32],
                rgba8: vec![255; usize::from(height) * 4].into(),
            })
            .collect::<Vec<_>>();
        let bindings = [0, textures.len() as u32 - 1].map(|texture| assets::ActorArtworkBinding {
            rig: texture,
            geometry_candidate: texture,
            entity_symbol: texture,
            geometry: 0,
            render_controller: 0,
            texture,
            material: "entity".into(),
            pose_mode: assets::ActorPoseMode::CompiledLiteral,
        });
        let pages = ActorArtworkPages::default().with_pack_artwork(&textures, &bindings);
        let low = pages.route(render_model::pack_rig_id(0)).unwrap();
        let high = pages
            .route(render_model::pack_rig_id(textures.len() as u32 - 1))
            .expect("valid high page retains its route");
        assert!(usize::from(high.page()) > usize::from(u8::MAX));
        let rig = render_model::pack_rig_id(0);
        let geometry =
            render_model::ActorRigGeometry::synthetic_cuboid(rig, [0.0; 3], [1.0; 3], 1).unwrap();
        let mut builder = ActorRigFrameBuilder::new([geometry]).unwrap();
        let actor = |runtime_id| ActorRigSubmission {
            material: Default::default(),
            culling_bounds: Default::default(),
            input: ActorRigRenderInput {
                identity: ActorRenderIdentity {
                    session_id: 1,
                    dimension: 0,
                    runtime_id,
                    spawn_revision: 1,
                    ingress_sequence: 1,
                    source_tick: Some(1),
                    movement_revision: 1,
                    pose_generation: render_api::PoseGeneration::new(1),
                    layer: render_model::ACTOR_LAYER_BODY,
                },
                rig,
                previous_bones: std::sync::Arc::from([render_model::RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0, 0.0, 0.0, 1.0],
                    axis_scale: render_model::UNIT_AXIS_SCALE,
                }]),
                current_bones: std::sync::Arc::from([render_model::RenderBoneTransform {
                    rotation: [0.0, 0.0, 0.0, 1.0],
                    translation_scale: [0.0, 0.0, 0.0, 1.0],
                    axis_scale: render_model::UNIT_AXIS_SCALE,
                }]),
                completed_tick: 1,
                reset_generation: 1,
            },
            world_from_actor: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            texture_layer: 0,
            route: ActorRigRoute::Compiled,
            tint: 0,
            uv_anim: render_model::IDENTITY_UV_ANIM,
            light: 0,
            overlay_rgba8: 0,
        };
        let page_of = |identity: &ActorRenderIdentity| {
            if identity.runtime_id == 2 {
                low.page()
            } else {
                high.page()
            }
        };
        let frame = builder.build_paged(0.5, None, [actor(1), actor(2), actor(3)], page_of);
        assert_eq!(frame.instances.len(), 3);
        let instance_pages = frame
            .manifest
            .iter()
            .map(|entry| page_of(&entry.identity))
            .collect::<Vec<_>>();
        let mut spans = Vec::new();
        draw_spans(
            &mut spans,
            &instance_pages,
            &frame.instances,
            &frame.geometry_spans,
            frame.manifest.iter().map(|entry| entry.identity.layer),
        );
        assert_eq!(spans.len(), 2);
        assert_eq!((spans[0].page, spans[0].count), (low.page(), 1));
        assert_eq!((spans[1].page, spans[1].count), (high.page(), 2));
    }

    #[test]
    fn coplanar_dissolve_passes_have_separate_ordered_draw_spans() {
        let instances = [
            assets::EntityRenderMaterial::DissolveDepth,
            assets::EntityRenderMaterial::DissolveColor,
        ]
        .map(|material| render_model::ActorGpuInstance {
            material: material as u32,
            ..Default::default()
        });
        let mut spans = Vec::new();
        draw_spans(
            &mut spans,
            &[1, 1],
            &instances,
            &[render_model::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 36,
            }],
            std::iter::repeat(0),
        );
        assert_eq!(spans.len(), 2);
        assert_eq!(
            (spans[0].first, spans[0].count, spans[0].material),
            (0, 1, instances[0].material)
        );
        assert_eq!(
            (spans[1].first, spans[1].count, spans[1].material),
            (1, 1, instances[1].material)
        );
    }
    #[test]
    fn spans_split_on_page_and_geometry_and_carry_exact_vertex_counts() {
        let instance = |geometry_id| render_model::ActorGpuInstance {
            geometry_id,
            ..Default::default()
        };
        let geometry = [
            render_model::ActorRigGeometrySpan {
                first_vertex: 0,
                vertex_count: 36,
            },
            render_model::ActorRigGeometrySpan {
                first_vertex: 36,
                vertex_count: 3024,
            },
        ];
        let mut spans = Vec::new();
        draw_spans(
            &mut spans,
            &[0, 1, 1, 1, 2],
            &[
                instance(0),
                instance(1),
                instance(1),
                instance(0),
                instance(0),
            ],
            &geometry,
            std::iter::repeat(0),
        );
        let span = |page, first, count, vertex_count| ActorDrawSpan {
            material: 0,
            page,
            first,
            count,
            vertex_count,
        };
        assert_eq!(
            spans,
            vec![
                span(0, 0, 1, 36),
                span(1, 1, 2, 3024),
                span(1, 3, 1, 36),
                span(2, 4, 1, 36),
            ]
        );
    }

    // A tall flipbook past the device limit draws downscaled instead of blanking every page.
    #[test]
    fn a_page_past_the_device_limit_uploads_downscaled() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue::new(queue);
        let side = device.limits().max_texture_dimension_2d;
        let height = u16::try_from(side * 2).unwrap();
        let mut pages = ActorArtworkPages::default();
        pages.identity = [1; 32];
        pages.pages = Arc::from([render_model::ActorTexturePage {
            width: 2,
            height,
            layers: 1,
            rgba8: vec![255; 2 * usize::from(height) * 4].into(),
            color_mask: true,
            multitexture: false,
        }]);
        let mut gpu = GpuArtwork::default();
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(gpu.pages.len(), 1);
        assert!(gpu.pages[0].as_ref().unwrap().color_mask);
    }

    #[test]
    fn replacement_artwork_supersedes_the_previous_generation() {
        let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
        let device = RenderDevice::from(device);
        let queue = RenderQueue::new(queue);
        let mut pages = ActorArtworkPages::default();
        pages.identity = [1; 32];
        pages.entity_identity = [2; 32];
        pages.pages = Arc::from([render_model::ActorTexturePage {
            width: 16,
            height: 16,
            layers: 1,
            rgba8: vec![255; 16 * 16 * 4].into(),
            color_mask: false,
            multitexture: false,
        }]);
        let mut gpu = GpuArtwork::default();
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(gpu.pages.len(), 1);
        assert!(gpu.prepare(&pages, &device, &queue));
        pages.entity_identity = [3; 32];
        assert!(gpu.prepare(&pages, &device, &queue));
        assert_eq!(gpu.pages.len(), 1);
        assert_eq!(gpu.identity, Some(([1; 32], [3; 32])));
        // Without artwork the old pages go, but the glint stays bound for player skins.
        assert!(gpu.prepare(&ActorArtworkPages::default(), &device, &queue));
        assert!(gpu.pages.is_empty() && gpu.glint.texture.is_some());
        assert_eq!(gpu.identity, Some(([0; 32], [0; 32])));
    }
}

#[cfg(test)]
#[path = "artwork_reuse_tests.rs"]
mod reuse_tests;

#[cfg(test)]
#[path = "artwork_work_tests.rs"]
mod work_tests;

#[cfg(test)]
#[path = "artwork_allocation_tests.rs"]
mod allocation_tests;

#[cfg(test)]
#[path = "artwork_ownership_tests.rs"]
mod ownership_tests;

#[cfg(test)]
#[path = "artwork_upload_tests.rs"]
mod upload_tests;
