//! Resident far-terrain tiles: quads in one storage buffer, cell colours in one texture array.

use crate::gpu_allocation::Requirement;
use crate::queue_writes::ProfiledQueue as _;
use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    sync::Arc,
};

use bevy::{
    camera::primitives::{Aabb, Frustum},
    math::{Affine3A, Vec3},
    prelude::{Camera3d, Commands, Component, Entity, Query, Res, ResMut, Resource, With},
    render::{
        render_resource::{
            BindGroup, BindGroupEntry, BindGroupLayoutDescriptor, BindGroupLayoutEntry,
            BindingResource, BindingType, Buffer, BufferBindingType, BufferDescriptor, BufferId,
            BufferUsages, CachedRenderPipelineId, ColorTargetState, ColorWrites, CompareFunction,
            DepthStencilState, Extent3d, Face, FragmentState, MultisampleState, Origin3d,
            PipelineCache, PrimitiveState, RenderPipelineDescriptor, ShaderStages, ShaderType,
            TexelCopyBufferLayout, TexelCopyTextureInfo, Texture, TextureAspect, TextureDescriptor,
            TextureDimension, TextureFormat, TextureSampleType, TextureUsages, TextureView,
            TextureViewDescriptor, TextureViewDimension, TextureViewId, VertexState,
        },
        renderer::{RenderDevice, RenderQueue},
        view::{ExtractedView, Msaa, ViewUniform, ViewUniforms},
    },
};
use far_terrain::{
    ColumnPos, NearMask, TileKey, TileMesh,
    config::{LEVEL_COUNT, TILE_CELLS},
};
use render_scene::{FarTerrainResidency, FarTerrainScene};

use super::SHADER;

#[path = "gpu_coverage.rs"]
mod coverage;
#[path = "gpu_upload.rs"]
mod upload;

const QUAD_BYTES: u64 = 8;
const TILE_RECORD_BYTES: u64 = 16;
/// One RGBA8 colour layer of a tile.
const TILE_LAYER_BYTES: u64 = (TILE_CELLS * TILE_CELLS * 4) as u64;
const MASK_HEADER_WORDS: usize = 4;
const UPLOAD_TILES_PER_FRAME: usize = 8;
const UPLOAD_BYTES_PER_FRAME: u64 = 1024 * 1024;

struct Resident {
    mesh: Arc<TileMesh>,
    slot: u32,
    quads: Range<u32>,
    visible: bool,
}

/// One tile drawn this frame: its slot is the instance index the shader reads its record by.
#[derive(Clone)]
pub(super) struct FarDraw {
    pub(super) slot: u32,
    pub(super) quads: Range<u32>,
    pub(super) bounds: Aabb,
}

#[derive(Resource)]
pub(crate) struct FarTerrainGpu {
    quads: Buffer,
    quad_capacity: u32,
    ranges: super::ranges::QuadRanges,
    records: Buffer,
    colours: Texture,
    colours_view: TextureView,
    slots: u32,
    free_slots: Vec<u32>,
    /// Optional growth waits until resident storage is released before trying again.
    growth_blocked: bool,
    mask: Buffer,
    mask_words: u64,
    resident: HashMap<TileKey, Resident>,
    applied_tiles: Option<Arc<[Arc<TileMesh>]>>,
    applied_mask: Option<Arc<NearMask>>,
    applied_world: Option<FarTerrainResidency>,
    protected: HashSet<ColumnPos>,
    empty_far: HashSet<ColumnPos>,
    publication_revision: u64,
    pub(super) draws: Vec<FarDraw>,
    layout: BindGroupLayoutDescriptor,
    pipelines: Vec<((TextureFormat, u32), CachedRenderPipelineId)>,
}

impl FarTerrainGpu {
    pub(crate) fn new(device: &RenderDevice) -> Self {
        let storage = |binding| BindGroupLayoutEntry {
            binding,
            visibility: ShaderStages::VERTEX,
            ty: BindingType::Buffer {
                ty: BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = BindGroupLayoutDescriptor::new(
            "far terrain bind group layout",
            &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: Some(ViewUniform::min_size()),
                    },
                    count: None,
                },
                storage(1),
                storage(2),
                BindGroupLayoutEntry {
                    binding: 3,
                    visibility: ShaderStages::FRAGMENT,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: false },
                        view_dimension: TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    visibility: ShaderStages::VERTEX_FRAGMENT,
                    ..storage(4)
                },
            ],
        );
        let (colours, colours_view) = colour_texture(device, 1);
        Self {
            quads: storage_buffer(device, "far terrain quads", QUAD_BYTES),
            quad_capacity: 1,
            ranges: Default::default(),
            records: storage_buffer(device, "far terrain tiles", TILE_RECORD_BYTES),
            colours,
            colours_view,
            slots: 1,
            free_slots: vec![0],
            growth_blocked: false,
            mask: storage_buffer(
                device,
                "far terrain near columns",
                4 * MASK_HEADER_WORDS as u64,
            ),
            mask_words: MASK_HEADER_WORDS as u64,
            resident: HashMap::new(),
            applied_tiles: None,
            applied_mask: None,
            applied_world: None,
            protected: HashSet::new(),
            empty_far: HashSet::new(),
            publication_revision: 0,
            draws: Vec::new(),
            layout,
            pipelines: Vec::new(),
        }
    }

    /// Reuses pipelines until the attachment format or sample count changes.
    fn pipeline(
        &mut self,
        cache: &PipelineCache,
        format: TextureFormat,
        samples: u32,
    ) -> CachedRenderPipelineId {
        let key = (format, samples);
        if let Some((_, id)) = self.pipelines.iter().find(|(known, _)| *known == key) {
            return *id;
        }
        let id =
            cache.queue_render_pipeline(pipeline_descriptor(self.layout.clone(), format, samples));
        self.pipelines.push((key, id));
        id
    }

    /// Uploads one bounded batch, retaining covering draws until their replacements are admitted.
    /// Returns false for deferred uploads; only required allocation failures invalidate terrain.
    pub(crate) fn apply_tiles(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        tiles: &[Arc<TileMesh>],
    ) -> bool {
        self.publication_revision = self.publication_revision.wrapping_add(1);
        let wanted: HashMap<TileKey, &Arc<TileMesh>> =
            tiles.iter().map(|mesh| (mesh.key, mesh)).collect();
        let stale: Vec<TileKey> = self
            .resident
            .iter()
            .filter(|(key, resident)| {
                !wanted.contains_key(key)
                    && (!resident.visible
                        || (!self.protects(**key)
                            && !tiles.iter().any(|mesh| mesh.key.overlaps(**key))))
            })
            .map(|(key, _)| *key)
            .collect();
        for key in stale {
            let resident = self.resident.remove(&key).expect("listed above");
            self.ranges.release(resident.quads);
            self.free_slots.push(resident.slot);
            self.growth_blocked = false;
        }
        let fallbacks = self
            .resident
            .keys()
            .filter(|key| !wanted.contains_key(key))
            .count();
        self.grow_slots(
            device,
            queue,
            (tiles.len() + fallbacks).min(u32::MAX as usize) as u32,
        );
        let quad_count = tiles.iter().fold(0_u64, |count, mesh| {
            count.saturating_add(mesh.quads.len() as u64)
        });
        let quad_count = self
            .resident
            .iter()
            .filter(|(key, _)| !wanted.contains_key(key))
            .fold(quad_count, |count, (_, resident)| {
                count.saturating_add(resident.quads.len() as u64)
            });
        self.grow_quads(
            device,
            queue,
            quad_count.min(u64::from(u32::MAX)) as u32,
            Requirement::Optional,
        );
        // Finer tiles first, retained fallbacks included; when space runs out, coarser
        // residents make room, and only what nothing coarser can free is left out.
        let order = upload_order(tiles.iter().map(|mesh| mesh.key));
        let priority: HashMap<TileKey, (u8, usize)> = order
            .iter()
            .map(|&(index, rank)| (tiles[index].key, rank))
            .collect();
        let mut uploaded_tiles = 0;
        let mut uploaded_bytes = 0;
        let mut complete = true;
        let mut allocation_failed = false;
        for (index, rank) in order {
            let mesh = &tiles[index];
            if self.current(mesh) {
                continue;
            }
            // A partial rebuild cannot overwrite the only visible evidence for another column.
            if !matches!(&mesh.coverage, far_terrain::TileCoverage::Complete)
                && self.resident.get(&mesh.key).is_some_and(|old| {
                    old.visible
                        && old
                            .mesh
                            .covered_columns()
                            .any(|column| !mesh.covers(column))
                })
            {
                continue;
            }
            let bytes = mesh.quads.len() as u64 * QUAD_BYTES
                + TILE_RECORD_BYTES
                + mesh.colours.len() as u64;
            // Admit an oversized first tile so a valid mesh can never starve.
            if uploaded_tiles >= UPLOAD_TILES_PER_FRAME
                || (uploaded_tiles > 0 && uploaded_bytes + bytes > UPLOAD_BYTES_PER_FRAME)
            {
                complete = false;
                break;
            }
            let Some((slot, quads)) =
                self.allocate_tile(device, queue, mesh, tiles, &priority, rank)
            else {
                allocation_failed = true;
                // A later coarser replacement may free space without uncovering a fallback.
                complete &= !self
                    .resident
                    .keys()
                    .any(|key| !wanted.contains_key(key) || *key == mesh.key);
                continue;
            };
            // Earlier misses need another pass after this allocation changes free
            // space or makes a retained fallback reclaimable.
            complete &= !allocation_failed;
            let upload = upload::TileUpload {
                gpu: self,
                slot,
                quads: Some(quads),
            };
            if upload.commit(device, queue, mesh, bytes).is_err() {
                return false;
            }
            uploaded_tiles += 1;
            uploaded_bytes += bytes;
        }
        self.update_coverage(tiles, &wanted);
        complete
    }

    /// Allocates a tile without retiring its coverage unless the complete allocation fits.
    fn allocate_tile(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        mesh: &TileMesh,
        tiles: &[Arc<TileMesh>],
        priority: &HashMap<TileKey, (u8, usize)>,
        rank: (u8, usize),
    ) -> Option<(u32, Range<u32>)> {
        let count = mesh.quads.len() as u32;
        if self
            .resident
            .get(&mesh.key)
            .is_some_and(|old| old.quads.len() >= count as usize)
        {
            let old = self.resident.remove(&mesh.key).expect("reusable resident");
            let quads = old.quads.start..old.quads.start + count;
            if quads.end < old.quads.end {
                self.growth_blocked = false;
            }
            self.ranges.release(quads.end..old.quads.end);
            return Some((old.slot, quads));
        }
        if !self.resident.contains_key(&mesh.key)
            && let Some(slot) = self.allocate_slot(device, queue)
        {
            if let Some(quads) = self.allocate_quads(device, queue, count) {
                return Some((slot, quads));
            }
            self.free_slots.push(slot);
        }
        let mut candidates: Vec<_> = self
            .resident
            .keys()
            .copied()
            .filter(|key| {
                *key == mesh.key
                    || match priority.get(key) {
                        Some(other) => {
                            *other > rank
                                && !self.protects(*key)
                                && !self.resident.keys().any(|fallback| {
                                    !priority.contains_key(fallback) && fallback.overlaps(*key)
                                })
                        }
                        None => {
                            // A completed fallback can free space anywhere in the batch,
                            // including when this upload does not overlap its replacements.
                            self.replaced(*key, tiles, Some(mesh.key))
                        }
                    }
            })
            .collect();
        candidates.sort_unstable_by_key(|key| {
            (
                u8::from(*key != mesh.key),
                priority.contains_key(key),
                std::cmp::Reverse(priority.get(key).copied()),
            )
        });
        let mut ranges = self.ranges.clone();
        for (index, key) in candidates.iter().enumerate() {
            ranges.release(self.resident[key].quads.clone());
            let quads = ranges.allocate(count);
            // Grow before retiring anything, so a failed growth leaves every resident drawn.
            if quads.end > max_quads(device)
                || !self.grow_quads(device, queue, quads.end, self.quad_requirement(count))
            {
                ranges.release(quads);
                continue;
            }
            for key in &candidates[..=index] {
                let old = self.resident.remove(key).expect("planned resident");
                self.free_slots.push(old.slot);
            }
            self.ranges = ranges;
            return Some((self.free_slots.pop().expect("retired slot"), quads));
        }
        None
    }

    /// Whether the requested revision has an uploaded range, even if a fallback still hides it.
    fn current(&self, mesh: &TileMesh) -> bool {
        self.resident
            .get(&mesh.key)
            .is_some_and(|resident| resident.mesh.revision == mesh.revision)
    }

    /// Keeps each fallback visible until every replacement over its area is uploaded.
    fn update_coverage(
        &mut self,
        tiles: &[Arc<TileMesh>],
        wanted: &HashMap<TileKey, &Arc<TileMesh>>,
    ) {
        let retired: Vec<_> = self
            .resident
            .keys()
            .copied()
            .filter(|key| !wanted.contains_key(key) && self.replaced(*key, tiles, None))
            .collect();
        for key in retired {
            let old = self.resident.remove(&key).expect("covered fallback");
            self.ranges.release(old.quads);
            self.free_slots.push(old.slot);
            self.growth_blocked = false;
        }
        let fallbacks: Vec<_> = self
            .resident
            .keys()
            .copied()
            .filter(|key| !wanted.contains_key(key))
            .collect();
        self.draws.clear();
        for resident in self.resident.values_mut() {
            resident.visible = false;
        }
        for key in tiles
            .iter()
            .map(|mesh| mesh.key)
            .chain(fallbacks.iter().copied())
        {
            if wanted.contains_key(&key) && fallbacks.iter().any(|old| old.overlaps(key)) {
                continue;
            }
            let Some(resident) = self.resident.get_mut(&key) else {
                continue;
            };
            resident.visible = true;
            if !resident.mesh.quads.is_empty() {
                self.draws.push(FarDraw {
                    slot: resident.slot,
                    quads: resident.quads.clone(),
                    bounds: tile_bounds(&resident.mesh),
                });
            }
        }
    }

    /// Discards the old world's coverage and acknowledgements before preparing another world.
    pub(crate) fn invalidate_publication(&mut self) {
        if !self.resident.is_empty() {
            self.growth_blocked = false;
        }
        self.resident.clear();
        self.ranges = Default::default();
        self.free_slots = (0..self.slots).rev().collect();
        self.draws.clear();
        self.applied_tiles = None;
        self.applied_mask = None;
        self.protected.clear();
        self.empty_far.clear();
    }

    /// Returns grown storage to its initial size once nothing is resident, as when far
    /// terrain is turned off or a shader pack draws the world without it.
    fn release_storage(&mut self, device: &RenderDevice) {
        if !self.resident.is_empty() || (self.quad_capacity == 1 && self.slots == 1) {
            return;
        }
        self.quads = storage_buffer(device, "far terrain quads", QUAD_BYTES);
        self.quad_capacity = 1;
        self.ranges = Default::default();
        self.records = storage_buffer(device, "far terrain tiles", TILE_RECORD_BYTES);
        (self.colours, self.colours_view) = colour_texture(device, 1);
        self.slots = 1;
        self.free_slots = vec![0];
        self.growth_blocked = false;
    }

    /// Writes only one admitted tile's geometry, record and colour layer.
    fn upload(&mut self, queue: &RenderQueue, mesh: &TileMesh, slot: u32, quads: &Range<u32>) {
        if !mesh.quads.is_empty() {
            let bytes: &[u8] = bytemuck::cast_slice(&mesh.quads);
            queue.profiled_write_buffer(&self.quads, u64::from(quads.start) * QUAD_BYTES, bytes);
        }
        let [x, z] = mesh.key.origin();
        let record = [x as u32, z as u32, u32::from(mesh.key.level), 0];
        queue.profiled_write_buffer(
            &self.records,
            u64::from(slot) * TILE_RECORD_BYTES,
            bytemuck::cast_slice(&record),
        );
        queue.profiled_write_texture(
            TexelCopyTextureInfo {
                texture: &self.colours,
                mip_level: 0,
                origin: Origin3d {
                    x: 0,
                    y: 0,
                    z: slot,
                },
                aspect: TextureAspect::All,
            },
            &mesh.colours,
            TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(TILE_CELLS as u32 * 4),
                rows_per_image: Some(TILE_CELLS as u32),
            },
            Extent3d {
                width: TILE_CELLS as u32,
                height: TILE_CELLS as u32,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Takes a free layer and record slot, growing their resources within device limits.
    fn allocate_slot(&mut self, device: &RenderDevice, queue: &RenderQueue) -> Option<u32> {
        if self.free_slots.is_empty() {
            self.grow_slots(device, queue, self.slots.saturating_add(1));
        }
        self.free_slots.pop()
    }

    /// Copies existing layers and records before publishing larger replacements.
    fn grow_slots(&mut self, device: &RenderDevice, queue: &RenderQueue, required: u32) {
        let limits = device.limits();
        let limit = limits.max_texture_array_layers.min(
            (limits
                .max_storage_buffer_binding_size
                .min(limits.max_buffer_size)
                / TILE_RECORD_BYTES) as u32,
        );
        let slots = required.min(limit).max(1).next_power_of_two().min(limit);
        if slots <= self.slots || self.growth_blocked {
            return;
        }
        let bytes = u64::from(slots) * (TILE_RECORD_BYTES + TILE_LAYER_BYTES);
        // Out of memory: no slot frees, so admission evicts lower-priority tiles instead.
        let Some(((colours, view), records)) =
            crate::gpu_allocation::checked(device, "far terrain tile slots", bytes, || {
                (
                    colour_texture(device, slots),
                    storage_buffer(
                        device,
                        "far terrain tiles",
                        u64::from(slots) * TILE_RECORD_BYTES,
                    ),
                )
            })
        else {
            self.growth_blocked = true;
            return;
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.records, 0, &records, 0, self.records.size());
        encoder.copy_texture_to_texture(
            self.colours.as_image_copy(),
            colours.as_image_copy(),
            self.colours.size(),
        );
        // Submission flushes earlier writes to the old resources before the copies.
        // Later uploads to the new resources must be queued after this submission.
        queue.submit([encoder.finish()]);
        self.colours = colours;
        self.colours_view = view;
        self.records = records;
        self.free_slots.extend((self.slots..slots).rev());
        self.slots = slots;
    }

    /// Allocates quads, copying existing geometry if the buffer must grow.
    fn allocate_quads(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        count: u32,
    ) -> Option<Range<u32>> {
        let max_quads = max_quads(device);
        let range = self.ranges.allocate(count);
        if range.end > max_quads
            || !self.grow_quads(device, queue, range.end, self.quad_requirement(count))
        {
            self.ranges.release(range);
            return None;
        }
        Some(range)
    }

    /// Growth is required only if a tile cannot fit even after evicting every other tile.
    fn quad_requirement(&self, count: u32) -> Requirement {
        if count > self.quad_capacity {
            Requirement::Required
        } else {
            Requirement::Optional
        }
    }

    /// Copies only the quad buffer when the selected geometry outgrows its capacity. False,
    /// keeping the old buffer, when the device is out of memory for the larger one.
    fn grow_quads(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        required: u32,
        requirement: Requirement,
    ) -> bool {
        let capacity = required
            .min(max_quads(device))
            .max(1)
            .next_power_of_two()
            .min(max_quads(device));
        if capacity <= self.quad_capacity {
            return true;
        }
        if requirement == Requirement::Optional && self.growth_blocked {
            return false;
        }
        let Some(quads) = checked_storage_buffer(
            device,
            "far terrain quads",
            u64::from(capacity) * QUAD_BYTES,
            requirement,
        ) else {
            self.growth_blocked |= requirement == Requirement::Optional;
            return false;
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.quads, 0, &quads, 0, self.quads.size());
        queue.submit([encoder.finish()]);
        self.quads = quads;
        self.quad_capacity = capacity;
        true
    }
}

impl crate::pipeline_warmup::PrewarmPipelines for FarTerrainGpu {
    fn prewarm(
        &mut self,
        cache: &PipelineCache,
        view: crate::pipeline_warmup::WarmView,
        ids: &mut crate::pipeline_warmup::WarmupIds,
    ) -> Result<(), bevy::prelude::BevyError> {
        let format = if view.hdr {
            render_scene::SCENE_HDR_FORMAT
        } else {
            render_scene::SCENE_COLOR_FORMAT
        };
        ids.push(self.pipeline(cache, format, view.msaa.samples()));
        Ok(())
    }
}

/// Indices of `keys` in upload priority with their ranks: finer levels first, then the given
/// order, so a fine retained fallback near the player outranks a farther coarse tile.
fn upload_order(keys: impl Iterator<Item = TileKey>) -> Vec<(usize, (u8, usize))> {
    let mut order: Vec<(usize, (u8, usize))> = keys
        .enumerate()
        .map(|(index, key)| (index, (key.level, index)))
        .collect();
    order.sort_unstable_by_key(|&(_, rank)| rank);
    order
}

/// Maximum number of packed quads that one storage binding can hold.
fn max_quads(device: &RenderDevice) -> u32 {
    let limits = device.limits();
    (limits
        .max_storage_buffer_binding_size
        .min(limits.max_buffer_size)
        / QUAD_BYTES)
        .min(u64::from(u32::MAX)) as u32
}

/// Creates storage that can be migrated without replaying its CPU payload.
fn storage_buffer(device: &RenderDevice, label: &'static str, size: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST | BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    })
}

/// [`storage_buffer`], or `None` when the device is out of memory for it.
fn checked_storage_buffer(
    device: &RenderDevice,
    label: &'static str,
    size: u64,
    requirement: Requirement,
) -> Option<Buffer> {
    requirement.checked(device, label, size, || storage_buffer(device, label, size))
}

/// Creates cell-colour layers that support both uploads and GPU migration.
fn colour_texture(device: &RenderDevice, layers: u32) -> (Texture, TextureView) {
    let texture = device.create_texture(&TextureDescriptor {
        label: Some("far terrain cell colours"),
        size: Extent3d {
            width: TILE_CELLS as u32,
            height: TILE_CELLS as u32,
            depth_or_array_layers: layers,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST | TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..Default::default()
    });
    (texture, view)
}

/// Bounds the uploaded revision, including its vertical skirts.
fn tile_bounds(mesh: &TileMesh) -> Aabb {
    let [x, z] = mesh.key.origin().map(|block| block as f32);
    let width = mesh.key.width_blocks() as f32;
    Aabb::from_min_max(
        Vec3::new(x, f32::from(mesh.min_y), z),
        Vec3::new(x + width, f32::from(mesh.max_y), z + width),
    )
}

/// Opaque, depth-tested and depth-written blocky geometry over the scene's colour samples.
fn pipeline_descriptor(
    layout: BindGroupLayoutDescriptor,
    format: TextureFormat,
    samples: u32,
) -> RenderPipelineDescriptor {
    RenderPipelineDescriptor {
        label: Some("far terrain pipeline".into()),
        layout: vec![layout, crate::lighting::layout()],
        vertex: VertexState {
            shader: SHADER,
            entry_point: Some("far_vertex".into()),
            ..Default::default()
        },
        fragment: Some(FragmentState {
            shader: SHADER,
            entry_point: Some("far_fragment".into()),
            targets: vec![Some(ColorTargetState {
                format,
                blend: None,
                write_mask: ColorWrites::ALL,
            })],
            ..Default::default()
        }),
        primitive: PrimitiveState {
            cull_mode: Some(Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(DepthStencilState {
            format: bevy::core_pipeline::core_3d::CORE_3D_DEPTH_FORMAT,
            depth_write_enabled: Some(true),
            depth_compare: Some(CompareFunction::GreaterEqual),
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: MultisampleState {
            count: samples,
            ..Default::default()
        },
        ..Default::default()
    }
}

/// Uploads changed tiles and the near mask; an unchanged scene does nothing.
pub(super) fn prepare_far_terrain(
    scene: Res<FarTerrainScene>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut gpu: ResMut<FarTerrainGpu>,
) {
    if !gpu
        .applied_world
        .as_ref()
        .is_some_and(|world| world.same_world(&scene.residency))
    {
        gpu.invalidate_publication();
        gpu.applied_world = Some(scene.residency.clone());
    }
    gpu.set_empty_coverage(
        scene
            .target
            .as_ref()
            .map(|target| target.empty_far.as_ref()),
    );
    if !gpu
        .applied_tiles
        .as_ref()
        .is_some_and(|applied| Arc::ptr_eq(applied, &scene.tiles))
    {
        gpu.applied_tiles = gpu
            .apply_tiles(&device, &queue, &scene.tiles)
            .then(|| Arc::clone(&scene.tiles));
        scene.residency.publish(
            gpu.resident
                .values()
                .filter(|resident| resident.visible && !resident.mesh.quads.is_empty())
                .map(|resident| (resident.mesh.key, resident.mesh.revision)),
        );
        if scene.tiles.is_empty() {
            gpu.release_storage(&device);
        }
    }
    if scene.target.is_none()
        && !gpu
            .applied_mask
            .as_ref()
            .is_some_and(|applied| Arc::ptr_eq(applied, &scene.near))
        && gpu.apply_mask(&device, &queue, &scene.near).is_ok()
    {
        gpu.applied_mask = Some(Arc::clone(&scene.near));
    }
}

/// Per-view pipeline, frustum-culled draws and bind group, rebuilt only when inputs change.
#[derive(Component)]
pub(crate) struct FarTerrainView {
    pub(super) pipeline: CachedRenderPipelineId,
    pub(super) visible: Vec<FarDraw>,
    pub(super) bind_group: Option<(BindGroupKey, BindGroup)>,
}

impl FarTerrainView {
    /// Drops the draws and the bind group, which would otherwise keep released storage alive.
    pub(super) fn release(&mut self) {
        self.visible.clear();
        self.bind_group = None;
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct BindGroupKey {
    view_uniforms: BufferId,
    quads: BufferId,
    records: BufferId,
    colours: TextureViewId,
    mask: BufferId,
}

type FarViews<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static ExtractedView,
        &'static Msaa,
        &'static crate::scene_target::SceneTarget,
        Option<&'static mut FarTerrainView>,
    ),
    With<Camera3d>,
>;

/// Culls tiles to each view's frustum and keeps its pipeline and bind group current. Shader
/// pack cameras whose pack draws terrain get no tiles: the pack shows its own sky and fog
/// beyond its terrain.
pub(super) fn prepare_far_terrain_views(
    mut commands: Commands,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<FarTerrainGpu>,
    mut views: FarViews,
    pack: render_scene::shaderpack::ShaderPackDrawnViews,
) {
    // With nothing to draw no pipeline is compiled and no bind group is made.
    if gpu.draws.is_empty() {
        for (.., state) in &mut views {
            if let Some(mut state) = state {
                state.release();
            }
        }
        return;
    }
    let (Some(uniforms), Some(binding)) = (
        view_uniforms.uniforms.buffer(),
        view_uniforms.uniforms.binding(),
    ) else {
        return;
    };
    let key = BindGroupKey {
        view_uniforms: uniforms.id(),
        quads: gpu.quads.id(),
        records: gpu.records.id(),
        colours: gpu.colours_view.id(),
        mask: gpu.mask.id(),
    };
    for (entity, view, msaa, scene, state) in &mut views {
        let pipeline = gpu.pipeline(&cache, scene.format(), msaa.samples());
        let mut state = match state {
            Some(state) => state,
            None => {
                commands.entity(entity).insert(FarTerrainView {
                    pipeline,
                    visible: Vec::new(),
                    bind_group: None,
                });
                continue;
            }
        };
        state.pipeline = pipeline;
        let clip_from_world = view
            .clip_from_world
            .unwrap_or_else(|| view.clip_from_view * view.world_from_view.to_matrix().inverse());
        let frustum = Frustum(bevy::shape::ViewFrustum::from_clip_from_world(
            &clip_from_world,
        ));
        state.visible.clear();
        if pack.replaces(entity, |drawn| drawn.terrain) {
            continue;
        }
        state.visible.extend(
            gpu.draws
                .iter()
                .filter(|draw| {
                    frustum.intersects_obb(&draw.bounds, &Affine3A::IDENTITY, true, false)
                })
                .cloned(),
        );
        if state.visible.is_empty()
            || state
                .bind_group
                .as_ref()
                .is_some_and(|(known, _)| *known == key)
        {
            continue;
        }
        let group = device.create_bind_group(
            "far terrain bind group",
            &cache.get_bind_group_layout(&gpu.layout),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: binding.clone(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: gpu.quads.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: gpu.records.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::TextureView(&gpu.colours_view),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: gpu.mask.as_entire_binding(),
                },
            ],
        );
        state.bind_group = Some((key, group));
    }
}

#[cfg(test)]
#[path = "gpu_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "coverage_tests.rs"]
mod coverage_tests;
