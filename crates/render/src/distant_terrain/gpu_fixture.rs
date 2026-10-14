//! A native device with the renderer's production kernels and pipelines, drawing uploaded
//! sections into a small target and reading back its pixels and counters, for the GPU tests.

use std::sync::{Arc, atomic::AtomicBool};

use bevy::{
    camera::primitives::Frustum,
    math::{Mat4, Vec3},
    prelude::Entity,
};
use lod::{
    colour::{FaceColour, MaterialColours, TintClass},
    draw::NeighbourLevels,
    fixtures::blocks,
    geometry::SectionPos,
    select::shown::{DrawUpdate, DrawnSection},
};
use render_scene::distant_terrain::{DistantTerrainColourTables, ResidentColumnMask};

use super::{
    gpu::{LodGpu, TargetKey},
    layout::{Capacity, LodView, Target, WATER_ALPHA, WORKGROUP, shade_uniform},
    node::{encode, finish},
    readback::{SizeBins, decode_counters},
    renderer::{LodFrame, LodPyramid},
    staging::Staging,
    table::SectionTable,
    test_meshes::ground,
    translucent::TranslucentSorter,
    view::PackedTables,
};
use crate::gpu_snapshot::Gpu;

/// Side of the square target in pixels.
pub(super) const SIDE: u32 = 64;
const COLOUR: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
const DEPTH: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
/// The sRGB colour of every fixture material but water.
pub(super) const STONE: [u8; 3] = [200, 100, 50];
/// The sRGB colour of fixture water.
pub(super) const WATER: [u8; 3] = [10, 20, 240];
/// The eye most tests look from.
pub(super) const EYE: [i32; 3] = [16, 20, -30];

/// Where the eye looks at the origin section's ground.
pub(super) fn target() -> Vec3 {
    Vec3::new(16.0, 4.0, 16.0)
}

/// A section at `pos` of stone below `height` and water below `water`, drawn with no
/// neighbours.
pub(super) fn section(pos: SectionPos, height: usize, water: usize) -> DrawnSection {
    DrawnSection {
        pos,
        mesh: ground(pos, height, water),
        neighbours: NeighbourLevels::NONE,
    }
}

/// An update adding `upserts`.
pub(super) fn update(upserts: Vec<DrawnSection>) -> DrawUpdate {
    DrawUpdate {
        upserts,
        removed: Vec::new(),
        relinked: Vec::new(),
        shown: Arc::from([]),
    }
}

/// A native device with the production renderer objects and a small target.
pub(super) struct Fixture {
    gpu: Gpu,
    lod: LodGpu,
    table: SectionTable,
    sorter: TranslucentSorter,
    colour: wgpu::TextureView,
    colour_texture: wgpu::Texture,
    depth: wgpu::TextureView,
    lightmap: wgpu::Buffer,
    tables: wgpu::Buffer,
    packed: PackedTables,
    first: bool,
    /// Built from the depth target after pass A when Hi-Z is on.
    pyramid: Option<LodPyramid>,
    /// Fog colour, start and end in blocks.
    pub(super) fog: ([f32; 3], f32, f32),
    /// The reverse-Z depth the target clears to before each frame, 0 for the far plane.
    pub(super) depth_clear: f32,
}

/// One frame's readout: the target's pixels, the culling counters and the statistics.
pub(super) struct Frame {
    pub(super) pixels: Vec<u8>,
    /// Visible opaque clusters, newly visible ones, visible translucent ones, dropped entries.
    pub(super) counters: [u32; 4],
    pub(super) bins: SizeBins,
}

impl Frame {
    /// The colour of the target's centre pixel.
    pub(super) fn centre(&self) -> [u8; 3] {
        self.pixel(SIDE / 2, SIDE / 2)
    }

    /// The colour of pixel `x`, `y`.
    pub(super) fn pixel(&self, x: u32, y: u32) -> [u8; 3] {
        let at = ((y * SIDE + x) * 4) as usize;
        [self.pixels[at], self.pixels[at + 1], self.pixels[at + 2]]
    }
}

/// True when every channel of `actual` lies within 2 of `expected`.
pub(super) fn close(actual: [u8; 3], expected: [u8; 3]) -> bool {
    actual
        .iter()
        .zip(expected)
        .all(|(&got, want)| got.abs_diff(want) <= 2)
}

impl Fixture {
    /// Tests clusters against a pyramid of the depth target from now on.
    pub(super) fn enable_hiz(&mut self) {
        let pyramid = crate::chunk::HizPyramid::new(&self.gpu.device, [SIDE, SIDE]);
        let bindings = self
            .lod
            .pyramid
            .bindings(&self.gpu.device, &self.depth, false, &pyramid);
        self.pyramid = Some(LodPyramid::new(pyramid, bindings));
    }

    /// The farthest depth of level-0 texel (0, 0) of the Hi-Z pyramid, as the last frame
    /// left it; Hi-Z must be on.
    pub(super) fn pyramid_corner(&self) -> f32 {
        let prepared = self.pyramid.as_ref().expect("hi-z is on");
        let device = &self.gpu.device;
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("distant terrain fixture pyramid texel"),
            size: 4,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &prepared.pyramid.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout::default(),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
        );
        self.gpu.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |result| {
            result.expect("readback maps");
        });
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the copy completes");
        let bytes = buffer
            .slice(..)
            .get_mapped_range()
            .expect("mapped")
            .to_vec();
        f32::from_le_bytes(bytes.try_into().expect("one texel"))
    }

    /// A fixture with a 16 MiB pool; `None`, after a named skip message, without a native
    /// adapter.
    pub(super) fn new(name: &str) -> Option<Self> {
        Self::with_pool(name, 16 << 20)
    }

    /// A fixture with a `pool_bytes` pool.
    pub(super) fn with_pool(name: &str, pool_bytes: u64) -> Option<Self> {
        let gpu = Gpu::for_fixture(name)?;
        let capacity = Capacity::new(pool_bytes, &gpu.device.limits()).expect("the pool fits");
        let key = TargetKey {
            colour: COLOUR,
            depth: DEPTH,
            samples: 1,
        };
        let lod = LodGpu::new(&gpu.device, capacity, key, false);
        let texture = |format, usage| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("distant terrain fixture target"),
                size: wgpu::Extent3d {
                    width: SIDE,
                    height: SIDE,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let colour_texture = texture(
            COLOUR,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        );
        let depth = texture(
            DEPTH,
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        )
        .create_view(&Default::default());
        let lightmap = gpu.buffer(&[1.0; 256 * 4], wgpu::BufferUsages::UNIFORM);
        let mut materials = MaterialColours::new();
        for id in 1..=blocks::SAND {
            let rgb = if id == blocks::WATER { WATER } else { STONE };
            assert!(materials.push([FaceColour::from_srgb(rgb, TintClass::None, 0); 6]));
        }
        let packed = PackedTables::new(&DistantTerrainColourTables {
            materials: Arc::new(materials),
            biomes: None,
        });
        let tables = gpu.words(&packed.words, wgpu::BufferUsages::STORAGE);
        let fog = ([0.0; 3], 1.0e6, 2.0e6);
        Some(Self {
            table: SectionTable::new(capacity),
            sorter: TranslucentSorter::default(),
            colour: colour_texture.create_view(&Default::default()),
            colour_texture,
            depth,
            lightmap,
            tables,
            packed,
            gpu,
            lod,
            first: true,
            pyramid: None,
            fog,
            depth_clear: 0.0,
        })
    }

    /// Cluster slots, which is also each draw list's room.
    pub(super) fn capacity_clusters(&self) -> u32 {
        self.lod.capacity.clusters
    }

    /// Draws with `colours` from now on.
    pub(super) fn set_colours(&mut self, colours: &DistantTerrainColourTables) {
        self.packed = PackedTables::new(colours);
        self.tables = self
            .gpu
            .words(&self.packed.words, wgpu::BufferUsages::STORAGE);
    }

    /// Forgets everything drawn, as a visible-set reset does: the next frame zeroes every
    /// slot, flag and list first.
    pub(super) fn reset(&mut self) {
        self.table = SectionTable::new(self.lod.capacity);
        self.sorter.reset();
        self.first = true;
    }

    /// The view from integer `eye` toward `target`.
    pub(super) fn view(
        &self,
        eye: [i32; 3],
        target: Vec3,
        resident: Option<&ResidentColumnMask>,
    ) -> LodView {
        let eye_f = Vec3::new(eye[0] as f32, eye[1] as f32, eye[2] as f32);
        let clip_from_rel: Mat4 =
            glam::camera::rh::proj::directx::perspective_infinite_reverse(1.2, 1.0, 0.05)
                * glam::camera::rh::view::look_at_mat4(Vec3::ZERO, target - eye_f, Vec3::Y);
        let frustum = Frustum(bevy::shape::ViewFrustum::from_clip_from_world(
            &clip_from_rel,
        ));
        LodView {
            clip_from_rel: clip_from_rel.to_cols_array_2d(),
            planes: std::array::from_fn(|index| frustum.half_spaces[index].normal_d().to_array()),
            camera: [eye[0], eye[1], eye[2], 0],
            camera_frac: [0.0; 4],
            viewport: [0.0, 0.0, SIDE as f32, SIDE as f32],
            depth: self
                .pyramid
                .as_ref()
                .map_or([SIDE, SIDE, 0, 0], |prepared| {
                    [SIDE, SIDE, prepared.pyramid.mip_count(), 1]
                }),
            counts: [
                self.table.section_slots(),
                self.table.cluster_slots(),
                self.sorter.order().len() as u32,
                self.lod.capacity.clusters,
            ],
            resident: resident.map_or([0; 4], |mask| {
                [
                    mask.centre[0],
                    mask.centre[1],
                    ResidentColumnMask::RADIUS,
                    1,
                ]
            }),
            fog: [self.fog.0[0], self.fog.0[1], self.fog.0[2], self.fog.1],
            fog_end: [self.fog.2, WATER_ALPHA, 0.0, 0.0],
            shade: shade_uniform(),
            tables: self.packed.info,
            tables_extra: [self.packed.records, 0, 0, 0],
        }
    }

    /// Uploads `update`, then encodes and reads back one frame seen from `eye`.
    pub(super) fn frame(
        &mut self,
        update: Option<DrawUpdate>,
        eye: [i32; 3],
        target: Vec3,
        resident: Option<&ResidentColumnMask>,
    ) -> Frame {
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let mut staging = Staging::new(self.lod.capacity.staging_words());
        self.table.begin_frame();
        if let Some(update) = update {
            self.table.start(update);
        }
        assert!(self.table.step(&mut staging, u64::MAX).committed || !self.first);
        let changed = self.table.take_translucent_changed();
        let eye_f = eye.map(f64::from);
        if let Some(order) = self
            .sorter
            .update(self.table.translucent(), eye_f, 4096, changed)
        {
            assert!(staging.copy(Target::Order, 0, order));
        }
        if let Some(mask) = resident {
            assert!(staging.copy(
                Target::Sections,
                self.lod.capacity.resident_word(),
                &mask.words
            ));
        }
        let view = self.view(eye, target, resident);
        queue.write_buffer(&self.lod.view, 0, bytemuck::bytes_of(&view));
        let mut bytes = vec![0_u8; staging.written_bytes() as usize];
        for (offset, part) in staging.parts() {
            bytes[offset..offset + part.len()].copy_from_slice(part);
        }
        queue.write_buffer(&self.lod.staging, 0, &bytes);
        let groups = |count: u32| count.div_ceil(WORKGROUP);
        let frame = LodFrame {
            view: Entity::PLACEHOLDER,
            clear: std::mem::take(&mut self.first),
            scatter_groups: staging.command_count(),
            section_groups: groups(view.counts[0]),
            cluster_groups: groups(view.counts[1]),
            order_groups: groups(view.counts[2]),
            cull_group: self.lod.cull_group(
                device,
                self.pyramid.as_ref().map(|prepared| &prepared.pyramid.view),
            ),
            draw_group: self.lod.draw_group(device, &self.tables, &self.lightmap),
            readback: Some(0),
            encoded: AtomicBool::new(false),
        };
        let mut encoder = device.create_command_encoder(&Default::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("distant terrain fixture clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &self.colour,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &self.depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(self.depth_clear),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        let colour = || wgpu::RenderPassColorAttachment {
            view: &self.colour,
            depth_slice: None,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        };
        let depth = || wgpu::RenderPassDepthStencilAttachment {
            view: &self.depth,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        };
        encode(
            &mut encoder,
            &self.lod,
            &frame,
            self.pyramid.as_ref(),
            &colour,
            &depth,
            None,
        );
        finish(&mut encoder, &self.lod, &frame);
        let pixels = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIDE * SIDE * 4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture: &self.colour_texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &pixels,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(SIDE * 4),
                    rows_per_image: Some(SIDE),
                },
            },
            wgpu::Extent3d {
                width: SIDE,
                height: SIDE,
                depth_or_array_layers: 1,
            },
        );
        queue.submit([encoder.finish()]);
        let stats = &self.lod.readbacks[0].stats;
        for buffer in [&pixels, stats] {
            buffer.slice(..).map_async(wgpu::MapMode::Read, |result| {
                result.expect("readback maps");
            });
        }
        device
            .poll(wgpu::PollType::wait_indefinitely())
            .expect("the frame completes");
        let read = |buffer: &wgpu::Buffer| {
            let bytes = buffer
                .slice(..)
                .get_mapped_range()
                .expect("mapped")
                .to_vec();
            buffer.unmap();
            bytes
        };
        let words = read(stats);
        let words: &[u32] = bytemuck::cast_slice(&words);
        Frame {
            pixels: read(&pixels),
            counters: decode_counters(words),
            bins: SizeBins::decode(words),
        }
    }
}
