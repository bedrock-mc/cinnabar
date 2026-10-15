//! Resident far-terrain tiles: quads in one storage buffer, cell colours in one texture array.

use crate::queue_writes::ProfiledQueue as _;
use std::{collections::HashMap, ops::Range, sync::Arc};

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
use far_terrain::{NearMask, TileKey, TileMesh, config::TILE_CELLS};
use render_scene::FarTerrainScene;

use super::SHADER;

const QUAD_BYTES: u64 = 8;
const TILE_RECORD_BYTES: u64 = 16;
const MASK_HEADER_WORDS: usize = 4;

struct Resident {
    mesh: Arc<TileMesh>,
    slot: u32,
    quads: Range<u32>,
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
    mask: Buffer,
    mask_words: u64,
    resident: HashMap<TileKey, Resident>,
    applied_tiles: Option<Arc<[Arc<TileMesh>]>>,
    applied_mask: Option<Arc<NearMask>>,
    pub(super) draws: Vec<FarDraw>,
    layout: BindGroupLayoutDescriptor,
    pipelines: Vec<((TextureFormat, u32), CachedRenderPipelineId)>,
}

impl FarTerrainGpu {
    pub(super) fn new(device: &RenderDevice) -> Self {
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
            mask: storage_buffer(
                device,
                "far terrain near columns",
                4 * MASK_HEADER_WORDS as u64,
            ),
            mask_words: MASK_HEADER_WORDS as u64,
            resident: HashMap::new(),
            applied_tiles: None,
            applied_mask: None,
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

    /// Makes the resident set match `tiles`, uploading only new or rebuilt tiles.
    fn apply_tiles(&mut self, device: &RenderDevice, queue: &RenderQueue, tiles: &[Arc<TileMesh>]) {
        let wanted: HashMap<TileKey, &Arc<TileMesh>> =
            tiles.iter().map(|mesh| (mesh.key, mesh)).collect();
        let stale: Vec<TileKey> = self
            .resident
            .iter()
            .filter(|(key, resident)| {
                wanted
                    .get(key)
                    .is_none_or(|mesh| mesh.revision != resident.mesh.revision)
            })
            .map(|(key, _)| *key)
            .collect();
        for key in stale {
            let resident = self.resident.remove(&key).expect("listed above");
            self.ranges.release(resident.quads);
            self.free_slots.push(resident.slot);
        }
        // Finer tiles first, retained fallbacks included; when space runs out, coarser
        // residents make room, and only what nothing coarser can free is left out.
        let order = upload_order(tiles.iter().map(|mesh| mesh.key));
        let priority: HashMap<TileKey, (u8, usize)> = order
            .iter()
            .map(|&(index, rank)| (tiles[index].key, rank))
            .collect();
        for (index, rank) in order {
            let mesh = &tiles[index];
            if self.resident.contains_key(&mesh.key) {
                continue;
            }
            let (slot, quads) = loop {
                if let Some(slot) = self.allocate_slot(device, queue) {
                    match self.allocate_quads(device, queue, mesh.quads.len() as u32) {
                        Some(quads) => break (Some(slot), quads),
                        None => self.free_slots.push(slot),
                    }
                }
                let Some(evicted) = self
                    .resident
                    .keys()
                    .copied()
                    .filter(|key| priority.get(key).is_some_and(|&other| other > rank))
                    .max_by_key(|key| priority[key])
                else {
                    break (None, 0..0);
                };
                let resident = self.resident.remove(&evicted).expect("listed above");
                self.ranges.release(resident.quads);
                self.free_slots.push(resident.slot);
            };
            let Some(slot) = slot else {
                break;
            };
            self.upload(queue, mesh, slot, &quads);
            self.resident.insert(
                mesh.key,
                Resident {
                    mesh: Arc::clone(mesh),
                    slot,
                    quads,
                },
            );
        }
        self.draws = tiles
            .iter()
            .filter(|mesh| !mesh.quads.is_empty() && self.resident.contains_key(&mesh.key))
            .map(|mesh| {
                let resident = &self.resident[&mesh.key];
                FarDraw {
                    slot: resident.slot,
                    quads: resident.quads.clone(),
                    bounds: tile_bounds(mesh),
                }
            })
            .collect();
    }

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

    /// A free texture layer and record slot, doubling both up to the device's array-layer
    /// limit and re-uploading residents; `None` once that limit is full.
    fn allocate_slot(&mut self, device: &RenderDevice, queue: &RenderQueue) -> Option<u32> {
        if let Some(slot) = self.free_slots.pop() {
            return Some(slot);
        }
        let old = self.slots;
        let limit = device.limits().max_texture_array_layers;
        if old >= limit {
            return None;
        }
        self.slots = (old * 2).min(limit);
        (self.colours, self.colours_view) = colour_texture(device, self.slots);
        self.records = storage_buffer(
            device,
            "far terrain tiles",
            u64::from(self.slots) * TILE_RECORD_BYTES,
        );
        self.free_slots.extend((old + 1..self.slots).rev());
        self.reupload_residents(queue);
        Some(old)
    }

    /// A quad range, doubling the buffer and re-uploading residents when it outgrows it;
    /// `None` once the buffer would pass what the device can bind.
    fn allocate_quads(
        &mut self,
        device: &RenderDevice,
        queue: &RenderQueue,
        count: u32,
    ) -> Option<Range<u32>> {
        let limits = device.limits();
        let max_quads = (limits
            .max_storage_buffer_binding_size
            .min(limits.max_buffer_size)
            / QUAD_BYTES)
            .min(u64::from(u32::MAX)) as u32;
        let range = self.ranges.allocate(count);
        if range.end > max_quads {
            self.ranges.release(range);
            return None;
        }
        if range.end > self.quad_capacity {
            self.quad_capacity = range.end.next_power_of_two().min(max_quads);
            self.quads = storage_buffer(
                device,
                "far terrain quads",
                u64::from(self.quad_capacity) * QUAD_BYTES,
            );
            self.reupload_residents(queue);
        }
        Some(range)
    }

    fn reupload_residents(&mut self, queue: &RenderQueue) {
        let residents: Vec<(Arc<TileMesh>, u32, Range<u32>)> = self
            .resident
            .values()
            .map(|resident| {
                (
                    Arc::clone(&resident.mesh),
                    resident.slot,
                    resident.quads.clone(),
                )
            })
            .collect();
        for (mesh, slot, quads) in residents {
            self.upload(queue, &mesh, slot, &quads);
        }
    }

    fn apply_mask(&mut self, device: &RenderDevice, queue: &RenderQueue, mask: &NearMask) {
        let [x, z] = mask.origin();
        let mut words = vec![x as u32, z as u32, mask.side(), 0];
        words.extend_from_slice(mask.words());
        if words.len() as u64 > self.mask_words {
            self.mask_words = (words.len() as u64).next_power_of_two();
            self.mask = storage_buffer(device, "far terrain near columns", self.mask_words * 4);
        }
        queue.profiled_write_buffer(&self.mask, 0, bytemuck::cast_slice(&words));
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

fn storage_buffer(device: &RenderDevice, label: &'static str, size: u64) -> Buffer {
    device.create_buffer(&BufferDescriptor {
        label: Some(label),
        size,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

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
        usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let view = texture.create_view(&TextureViewDescriptor {
        dimension: Some(TextureViewDimension::D2Array),
        ..Default::default()
    });
    (texture, view)
}

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
        .applied_tiles
        .as_ref()
        .is_some_and(|applied| Arc::ptr_eq(applied, &scene.tiles))
    {
        gpu.apply_tiles(&device, &queue, &scene.tiles);
        gpu.applied_tiles = Some(Arc::clone(&scene.tiles));
    }
    if !gpu
        .applied_mask
        .as_ref()
        .is_some_and(|applied| Arc::ptr_eq(applied, &scene.near))
    {
        gpu.apply_mask(&device, &queue, &scene.near);
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

/// Culls tiles to each view's frustum and keeps its pipeline and bind group current.
pub(super) fn prepare_far_terrain_views(
    mut commands: Commands,
    device: Res<RenderDevice>,
    cache: Res<PipelineCache>,
    view_uniforms: Res<ViewUniforms>,
    mut gpu: ResMut<FarTerrainGpu>,
    mut views: FarViews,
) {
    // With nothing to draw no pipeline is compiled and no bind group is made.
    if gpu.draws.is_empty() {
        for (.., state) in &mut views {
            if let Some(mut state) = state {
                state.visible.clear();
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
mod upload_order_tests {
    use super::*;

    /// A fine retained fallback, listed after the selected tiles, still outranks coarse ones.
    #[test]
    fn finer_tiles_rank_first_wherever_they_are_listed() {
        let key = |level, x| TileKey { level, x, z: 0 };
        let order: Vec<usize> = upload_order([key(0, 0), key(2, 1), key(0, 5)].into_iter())
            .into_iter()
            .map(|(index, _)| index)
            .collect();
        assert_eq!(order, [0, 2, 1]);
    }
}
