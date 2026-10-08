//! Bounded, cached resident-grid diffuse transport. Captured reflection cubes are not GI.

use crate::ChunkTextureAssets;
use bevy::shader::ShaderDefVal;
use bevy::{
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderContext, RenderDevice, RenderQueue},
    },
};
use std::{
    num::NonZeroU64,
    sync::atomic::{AtomicBool, Ordering},
};
#[path = "indirect_geometry.rs"]
mod geometry;
pub(crate) use geometry::{IndirectGeometry, collect_geometry};
#[cfg(test)]
#[path = "indirect_tests.rs"]
mod tests;

pub(crate) const GRID_SIZE: [u32; 3] = [48, 32, 48];
pub(crate) const CELL_SIZE: f32 = 1.0;
pub(crate) const PROBE_SPACING: u32 = 4;
pub(crate) const PROBE_SIZE: [u32; 3] = [
    GRID_SIZE[0] / PROBE_SPACING,
    GRID_SIZE[1] / PROBE_SPACING,
    GRID_SIZE[2] / PROBE_SPACING,
];
pub(crate) const HEADER_WORDS: usize = 4;
pub(crate) const PROBE_WORDS: usize = 26;
pub(crate) const INDIRECT_MIN_BYTES: u64 = HEADER_WORDS as u64 * 16;
pub(crate) const CELL_COUNT: usize = (GRID_SIZE[0] * GRID_SIZE[1] * GRID_SIZE[2]) as usize;
pub(crate) const PROBE_COUNT: u32 = PROBE_SIZE[0] * PROBE_SIZE[1] * PROBE_SIZE[2];
const BUFFER_WORDS: usize = HEADER_WORDS + CELL_COUNT + PROBE_COUNT as usize * PROBE_WORDS;

#[derive(Clone, Copy, PartialEq)]
struct GridKey {
    geometry: u64,
    coverage: u64,
    assets: crate::ChunkTextureAssetIdentity,
    tints: meshing::ChunkBiomeTintIdentity,
    origin: IVec3,
    dimension: Option<i32>,
}

pub(crate) struct IndirectGridGpu {
    pub buffer: Buffer,
    pub parameters: Buffer,
    update_order: Buffer,
    order: Vec<u32>,
    pub submitted: AtomicBool,
    words: Vec<[u32; 4]>,
    colors: Vec<geometry::Reflectance>,
    prepared: Option<GridKey>,
    lighting: Option<u64>,
    geometry_revision: Option<(u64, IVec3, Option<i32>, u64)>,
    coverage_revision: Option<(u64, IVec3, Option<i32>, u64)>,
    epoch: u32,
    cursor: u32,
    batch: u32,
    relight_again: bool,
    quality: super::EnhancedQuality,
    range: Option<[u32; 4]>,
    pub rebuilds: u64,
    pub uploads: u64,
}

impl IndirectGridGpu {
    pub fn new(device: &RenderDevice) -> Self {
        Self::allocate(device, false)
    }

    pub fn for_capture(device: &RenderDevice) -> Self {
        Self::allocate(device, true)
    }

    fn allocate(device: &RenderDevice, capture: bool) -> Self {
        let order = if capture {
            vec![0]
        } else {
            probe_update_order()
        };
        Self {
            update_order: device.create_buffer_with_data(&BufferInitDescriptor {
                label: Some("camera-first irradiance update order"),
                contents: bytemuck::cast_slice(&order),
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            }),
            order,
            buffer: device.create_buffer(&BufferDescriptor {
                label: Some("Enhanced spatial irradiance"),
                size: if capture {
                    INDIRECT_MIN_BYTES
                } else {
                    BUFFER_WORDS as u64 * 16
                },
                usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            parameters: device.create_buffer(&BufferDescriptor {
                label: Some("Enhanced irradiance update range"),
                size: 16,
                usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            submitted: AtomicBool::new(false),
            words: vec![[0; 4]; HEADER_WORDS + if capture { 0 } else { CELL_COUNT }],
            colors: Vec::new(),
            prepared: None,
            lighting: None,
            geometry_revision: None,
            coverage_revision: None,
            epoch: 0,
            cursor: 0,
            batch: 0,
            relight_again: false,
            quality: super::EnhancedQuality::default(),
            range: None,
            rebuilds: 0,
            uploads: 0,
        }
    }

    pub fn set_quality(&mut self, quality: super::EnhancedQuality) {
        if self.quality != quality {
            self.quality = quality;
            self.cursor = 0;
            self.batch = 0;
            self.relight_again = false;
            self.submitted.store(false, Ordering::Relaxed);
        }
    }

    pub fn prepare(
        &mut self,
        queue: &RenderQueue,
        geometry: &IndirectGeometry,
        coverage: &crate::chunk::ChunkResidentCoverage,
        assets: &ChunkTextureAssets,
        tints: &crate::ChunkBiomeTints,
        camera: Vec3,
        dimension: Option<i32>,
        lighting_signature: u64,
    ) -> bool {
        if self.submitted.swap(false, Ordering::Relaxed) {
            self.cursor = (self.cursor + self.batch).min(PROBE_COUNT);
        }
        if self.cursor == PROBE_COUNT && self.relight_again {
            self.cursor = 0;
            self.relight_again = false;
        }
        let spacing = PROBE_SPACING as f32;
        let snapped = (camera / spacing).floor().as_ivec3() * PROBE_SPACING as i32;
        let origin = snapped - IVec3::from_array(GRID_SIZE.map(|v| v as i32 / 2));
        let regional = match self.geometry_revision {
            Some((revision, previous_origin, previous_dimension, regional))
                if revision == geometry.revision
                    && previous_origin == origin
                    && previous_dimension == dimension =>
            {
                regional
            }
            _ => geometry.region_revision(origin, dimension),
        };
        self.geometry_revision = Some((geometry.revision, origin, dimension, regional));
        let coverage_revision = coverage.revision();
        let known = match self.coverage_revision {
            Some((revision, previous_origin, previous_dimension, signature))
                if revision == coverage_revision
                    && previous_origin == origin
                    && previous_dimension == dimension =>
            {
                signature
            }
            _ => geometry::coverage_signature(coverage, origin, dimension),
        };
        self.coverage_revision = Some((coverage_revision, origin, dimension, known));
        let key = GridKey {
            geometry: regional,
            coverage: known,
            assets: assets.identity(),
            tints: tints.table_identity(),
            origin,
            dimension,
        };
        let changed = self.prepared != Some(key);
        let relight = self.lighting != Some(lighting_signature);
        let retained = self.prepared.filter(|previous| {
            previous.dimension == dimension
                && previous.assets == key.assets
                && grids_overlap(previous.origin, origin)
        });
        let scroll = retained.filter(|previous| previous.origin != origin);
        if changed || relight {
            if changed && (retained.is_none() || scroll.is_some()) {
                if retained.is_none() {
                    self.epoch = self.epoch.wrapping_add(1).max(1);
                }
                self.cursor = 0;
                self.relight_again = false;
                order_scrolled_probes(&mut self.order, scroll.map(|v| v.origin), origin);
                queue.write_buffer(&self.update_order, 0, bytemuck::cast_slice(&self.order));
            } else if self.cursor == PROBE_COUNT {
                self.cursor = 0;
            } else if self.cursor > 0 {
                // Finish the current sweep before refreshing its already submitted probes.
                self.relight_again = true;
            }
            if changed {
                if self
                    .prepared
                    .is_none_or(|previous| previous.assets != key.assets)
                {
                    geometry::palette(assets, &mut self.colors);
                }
                self.words.fill([0; 4]);
                geometry::rebuild(
                    geometry,
                    coverage,
                    assets,
                    tints,
                    dimension,
                    origin,
                    &self.colors,
                    &mut self.words,
                );
                self.rebuilds += 1;
            }
            self.words[0] = [
                (origin.x as f32).to_bits(),
                (origin.y as f32).to_bits(),
                (origin.z as f32).to_bits(),
                CELL_SIZE.to_bits(),
            ];
            self.words[1] = [
                GRID_SIZE[0],
                GRID_SIZE[1],
                GRID_SIZE[2],
                (HEADER_WORDS + CELL_COUNT) as u32,
            ];
            self.words[2] = [
                PROBE_SIZE[0],
                PROBE_SIZE[1],
                PROBE_SIZE[2],
                (PROBE_SPACING as f32).to_bits(),
            ];
            self.words[3] = [
                self.epoch,
                u32::from(dimension.is_some()),
                PROBE_WORDS as u32,
                HEADER_WORDS as u32,
            ];
            let upload = if changed {
                &self.words[..]
            } else {
                &self.words[..HEADER_WORDS]
            };
            queue.write_buffer(&self.buffer, 0, bytemuck::cast_slice(upload));
            self.uploads += 1;
            self.prepared = Some(key);
            self.lighting = Some(lighting_signature);
        }
        self.batch = if dimension.is_some() {
            super::quality::budget(self.quality)
                .irradiance_updates
                .min(PROBE_COUNT - self.cursor)
        } else {
            0
        };
        if self.batch > 0 {
            let range = [
                self.cursor,
                self.batch,
                self.epoch,
                super::quality::budget(self.quality).irradiance_rays,
            ];
            if self.range != Some(range) {
                queue.write_buffer(&self.parameters, 0, bytemuck::cast_slice(&range));
                self.range = Some(range);
            }
        }
        changed || relight
    }

    pub fn pending(&self) -> bool {
        self.batch > 0
    }

    #[allow(clippy::too_many_arguments)]
    pub fn bind_group(
        &self,
        device: &RenderDevice,
        cache: &PipelineCache,
        frame: &Buffer,
        environment: &TextureView,
        sampler: &Sampler,
        locals: &Buffer,
        cloud_shadow: &TextureView,
    ) -> BindGroup {
        device.create_bind_group(
            "Enhanced resident irradiance update",
            &cache.get_bind_group_layout(&compute_layout()),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: frame.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: self.buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(environment),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: BindingResource::Sampler(sampler),
                },
                BindGroupEntry {
                    binding: 4,
                    resource: self.parameters.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 5,
                    resource: locals.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 6,
                    resource: BindingResource::TextureView(cloud_shadow),
                },
                BindGroupEntry {
                    binding: 7,
                    resource: self.update_order.as_entire_binding(),
                },
            ],
        )
    }
}

fn grids_overlap(previous: IVec3, current: IVec3) -> bool {
    (previous - current)
        .abs()
        .cmplt(IVec3::from_array(GRID_SIZE.map(|v| v as i32)))
        .all()
}

fn probe_cell(index: u32) -> IVec3 {
    IVec3::new(
        (index % PROBE_SIZE[0]) as i32,
        ((index / PROBE_SIZE[0]) % PROBE_SIZE[1]) as i32,
        (index / (PROBE_SIZE[0] * PROBE_SIZE[1])) as i32,
    )
}

fn order_scrolled_probes(order: &mut [u32], previous: Option<IVec3>, origin: IVec3) {
    let center = IVec3::from_array(PROBE_SIZE.map(|v| v as i32));
    order.sort_unstable_by_key(|&index| {
        let cell = probe_cell(index);
        let reused = previous.is_some_and(|previous| {
            let old_cell = cell + (origin - previous) / PROBE_SPACING as i32;
            old_cell.cmpge(IVec3::ZERO).all()
                && old_cell
                    .cmplt(IVec3::from_array(PROBE_SIZE.map(|v| v as i32)))
                    .all()
        });
        (
            reused,
            (cell * 2 + IVec3::ONE - center).length_squared(),
            index,
        )
    });
}

pub(crate) fn compute_layout() -> BindGroupLayoutDescriptor {
    let buffer = |size, ty| BindingType::Buffer {
        ty,
        has_dynamic_offset: false,
        min_binding_size: NonZeroU64::new(size),
    };
    let texture = |dimension| BindingType::Texture {
        sample_type: TextureSampleType::Float { filterable: true },
        view_dimension: dimension,
        multisampled: false,
    };
    let types = [
        buffer(
            std::mem::size_of::<super::frame::EnhancedFrameGpu>() as u64,
            BufferBindingType::Uniform,
        ),
        buffer(
            INDIRECT_MIN_BYTES,
            BufferBindingType::Storage { read_only: false },
        ),
        texture(TextureViewDimension::D2Array),
        BindingType::Sampler(SamplerBindingType::Filtering),
        buffer(16, BufferBindingType::Uniform),
        buffer(
            super::local_lights::LOCAL_LIGHT_BUFFER_BYTES as u64,
            BufferBindingType::Storage { read_only: true },
        ),
        texture(TextureViewDimension::D2),
        buffer(4, BufferBindingType::Storage { read_only: true }),
    ];
    let entries = types
        .into_iter()
        .enumerate()
        .map(|(binding, ty)| BindGroupLayoutEntry {
            binding: binding as u32,
            visibility: ShaderStages::COMPUTE,
            ty,
            count: None,
        })
        .collect::<Vec<_>>();
    BindGroupLayoutDescriptor::new("Enhanced irradiance trace layout", &entries)
}

fn probe_update_order() -> Vec<u32> {
    let mut order = (0..PROBE_COUNT).collect::<Vec<_>>();
    let center = Vec3::from_array(GRID_SIZE.map(|v| v as f32 * 0.5));
    order.sort_unstable_by(|&a, &b| {
        let distance = |index| {
            let p = Vec3::new(
                (index % PROBE_SIZE[0]) as f32,
                ((index / PROBE_SIZE[0]) % PROBE_SIZE[1]) as f32,
                (index / (PROBE_SIZE[0] * PROBE_SIZE[1])) as f32,
            );
            ((p + Vec3::splat(0.5)) * PROBE_SPACING as f32 + Vec3::splat(CELL_SIZE * 0.5) - center)
                .length_squared()
        };
        distance(a).total_cmp(&distance(b)).then_with(|| a.cmp(&b))
    });
    order
}

#[derive(Resource)]
pub(crate) struct IndirectPipelines {
    pipeline: CachedComputePipelineId,
}
impl FromWorld for IndirectPipelines {
    fn from_world(world: &mut World) -> Self {
        Self::new(world)
    }
}
impl IndirectPipelines {
    pub fn new(world: &World) -> Self {
        Self {
            pipeline: world.resource::<PipelineCache>().queue_compute_pipeline(
                ComputePipelineDescriptor {
                    label: Some("update_spatial_irradiance".into()),
                    layout: vec![compute_layout()],
                    shader: super::INDIRECT_COMPUTE_SHADER,
                    entry_point: Some("update_spatial_irradiance".into()),
                    shader_defs: vec![ShaderDefVal::Bool("INDIRECT_COMPUTE".into(), true)],
                    ..default()
                },
            ),
        }
    }
    pub fn dispatch(
        &self,
        context: &mut RenderContext,
        world: &World,
        grid: &IndirectGridGpu,
        group: &BindGroup,
    ) -> bool {
        if !grid.pending() {
            return false;
        }
        let Some(pipeline) = world
            .resource::<PipelineCache>()
            .get_compute_pipeline(self.pipeline)
        else {
            return false;
        };
        let mut pass = context
            .command_encoder()
            .begin_compute_pass(&ComputePassDescriptor {
                label: Some("bounded spatial diffuse transport"),
                ..default()
            });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.dispatch_workgroups(grid.batch, 1, 1);
        grid.submitted.store(true, Ordering::Relaxed);
        true
    }
}
