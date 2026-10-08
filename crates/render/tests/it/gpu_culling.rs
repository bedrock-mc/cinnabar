//! The GPU terrain cull keeps the CPU path's visible set and pixels, and its Hi-Z is conservative.
mod direct;
mod hiz;
#[path = "../../src/chunk/gpu_cull/kernels.rs"]
mod kernels;
#[path = "../../src/chunk/gpu_cull/model.rs"]
mod model;
#[path = "../../src/chunk/gpu_cull/occlusion.rs"]
mod occlusion;

use std::collections::BTreeSet;

use bevy::{
    camera::primitives::{Aabb, Frustum, Sphere},
    math::{Mat4, Vec3, Vec3A},
    transform::components::{GlobalTransform, Transform},
};
use meshing::{CubeQuadLayout, Face, PackedQuad};

use crate::{chunk_constants, gpu_snapshot, material_shader, shader_source, solid_terrain_raster};
use gpu_snapshot::{Gpu, SNAPSHOT_SIDE};
use kernels::{CullKernels, CullStorage, HizPyramid};
use model::{
    ARGS_WORDS, CullCamera, CullPhase, CullRecord, CullRecordSource, CullStream, CullViewInput,
    CullViewUniform, STREAM_COUNT, args_region, count_index, frustum_slack, reference_args,
};

const SIDE: f32 = world::SUB_CHUNK_SIDE as f32;
const INDEX_COUNTS: [u32; STREAM_COUNT] = [6; STREAM_COUNT];
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

type Args = [Vec<[u32; ARGS_WORDS as usize]>; STREAM_COUNT];

struct Camera {
    eye: Vec3,
    clip_from_view: Mat4,
    clip_from_world: Mat4,
    frustum: Frustum,
}

/// A square perspective view, built exactly as Bevy builds a camera frustum.
fn camera(eye: Vec3, target: Vec3) -> Camera {
    camera_with_aspect(eye, target, 1.0)
}

fn camera_with_aspect(eye: Vec3, target: Vec3, aspect: f32) -> Camera {
    let global =
        GlobalTransform::from(Transform::from_translation(eye).looking_at(target, Vec3::Y));
    let projection = Mat4::perspective_infinite_reverse_rh(1.2, aspect, 0.05);
    let clip_from_world = projection * Mat4::from(global.affine().inverse());
    let frustum = Frustum::from_clip_from_world_custom_far(
        &clip_from_world,
        &global.translation(),
        &global.back().as_vec3(),
        1000.0,
    );
    Camera {
        eye,
        clip_from_view: projection,
        clip_from_world,
        frustum,
    }
}

fn view_input(camera: &Camera, hiz_mips: u32) -> CullViewInput {
    CullViewInput {
        planes: std::array::from_fn(|index| {
            camera.frustum.half_spaces[index].normal_d().to_array()
        }),
        clip_from_world: camera.clip_from_world.as_dmat4().to_cols_array_2d(),
        camera: CullCamera::new(Some(camera.eye.as_dvec3().to_array())),
        viewport: [0.0, 0.0, SNAPSHOT_SIDE as f32, SNAPSHOT_SIDE as f32],
        depth_size: [SNAPSHOT_SIDE; 2],
        hiz_mips,
        index_counts: INDEX_COUNTS,
    }
}

/// Bevy's own sub-chunk visibility test, as `check_visibility` runs it.
fn bevy_visible(frustum: &Frustum, origin: [i32; 3]) -> bool {
    let transform = GlobalTransform::from_translation(Vec3::from_array(origin.map(|v| v as f32)));
    let aabb = Aabb {
        center: Vec3A::splat(SIDE / 2.0),
        half_extents: Vec3A::splat(SIDE / 2.0),
    };
    let world_from_local = transform.affine();
    let sphere = Sphere {
        center: world_from_local.transform_point3a(aabb.center),
        radius: transform.radius_vec3a(aabb.half_extents),
    };
    frustum.intersects_sphere(&sphere, false)
        && frustum.intersects_obb(&aabb, &world_from_local, true, false)
}

/// Whether float rounding may legitimately flip the kernel's frustum decision.
fn on_frustum_boundary(frustum: &Frustum, origin: [i32; 3]) -> bool {
    let center = origin.map(|value| value as f32 + SIDE / 2.0);
    frustum.half_spaces[..5].iter().any(|half_space| {
        let plane = half_space.normal_d().to_array();
        let distance: f32 = (0..3).map(|axis| plane[axis] * center[axis]).sum::<f32>() + plane[3];
        let extent: f32 = (0..3).map(|axis| plane[axis].abs() * SIDE / 2.0).sum();
        let radius = Vec3::splat(SIDE / 2.0).length();
        let slack = 2.0 * frustum_slack(plane, center);
        [distance + extent, distance + radius]
            .iter()
            .any(|margin| margin.abs() <= slack)
    })
}

/// Deterministic pseudo-random words for fixture variety.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, bound: u32) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        ((self.0 >> 33) % u64::from(bound)) as u32
    }
}

fn enabled_words(enabled: impl Fn(usize) -> bool, slots: usize) -> Vec<u32> {
    (0..slots.div_ceil(32))
        .map(|word| {
            (0..32)
                .filter(|bit| word * 32 + bit < slots && enabled(word * 32 + bit))
                .fold(0, |bits, bit| bits | 1 << bit)
        })
        .collect()
}

fn bits_of(set: &BTreeSet<usize>, slots: usize) -> Vec<u32> {
    (0..slots)
        .map(|slot| u32::from(set.contains(&slot)))
        .collect()
}

struct Culler<'a> {
    gpu: &'a Gpu,
    kernels: CullKernels,
    storage: CullStorage,
    slots: u32,
}

impl<'a> Culler<'a> {
    fn new(gpu: &'a Gpu, records: &[CullRecord], enabled: &[u32]) -> Self {
        let storage = CullStorage::new(&gpu.device, records.len().next_power_of_two() as u32, true);
        gpu.queue
            .write_buffer(&storage.records, 0, bytemuck::cast_slice(records));
        gpu.queue
            .write_buffer(&storage.enabled, 0, bytemuck::cast_slice(enabled));
        Self {
            gpu,
            kernels: CullKernels::new(&gpu.device),
            storage,
            slots: records.len() as u32,
        }
    }

    fn set_history(&self, history: &[u32]) {
        self.gpu
            .queue
            .write_buffer(&self.storage.history, 0, bytemuck::cast_slice(history));
    }

    fn history(&self) -> BTreeSet<usize> {
        let bytes = read_buffer(self.gpu, &self.storage.history, u64::from(self.slots) * 4);
        bytemuck::cast_slice::<u8, u32>(&bytes)
            .iter()
            .enumerate()
            .filter(|(_, bit)| **bit != 0)
            .map(|(slot, _)| slot)
            .collect()
    }

    /// Encodes one phase into `encoder`; the late phase tests `pyramid` when given one.
    fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        input: &CullViewInput,
        phase: CullPhase,
        pyramid: Option<&HizPyramid>,
    ) {
        let uniform = CullViewUniform::new(input, phase, self.slots, self.storage.capacity);
        self.storage.write_uniform(&self.gpu.queue, phase, &uniform);
        let groups = self
            .kernels
            .bind_groups(&self.gpu.device, &self.storage, pyramid);
        self.kernels
            .encode_cull(encoder, &groups[phase as usize], self.slots);
    }

    fn run(&self, input: &CullViewInput, phase: CullPhase, pyramid: Option<&HizPyramid>) -> Args {
        let mut encoder = self.gpu.device.create_command_encoder(&Default::default());
        self.encode(&mut encoder, input, phase, pyramid);
        self.gpu.queue.submit([encoder.finish()]);
        self.args(phase)
    }

    fn args(&self, phase: CullPhase) -> Args {
        let counts = read_buffer(self.gpu, &self.storage.draw_counts, 32);
        let counts = bytemuck::cast_slice::<u8, u32>(&counts).to_vec();
        let words = read_buffer(self.gpu, &self.storage.args, self.storage.args.size());
        let words = bytemuck::cast_slice::<u8, u32>(&words);
        CullStream::ALL.map(|stream| {
            let start = args_region(self.storage.capacity, phase, stream) as usize;
            let count = counts[count_index(phase, stream) as usize] as usize;
            assert!(count as u32 <= stream.draws_per_record() * self.slots);
            words[start..start + count * ARGS_WORDS as usize]
                .chunks_exact(ARGS_WORDS as usize)
                .map(|chunk| chunk.try_into().unwrap())
                .collect()
        })
    }
}

fn read_buffer(gpu: &Gpu, buffer: &wgpu::Buffer, size: u64) -> Vec<u8> {
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, size);
    gpu.queue.submit([encoder.finish()]);
    map(gpu, &staging)
}

fn map(gpu: &Gpu, staging: &wgpu::Buffer) -> Vec<u8> {
    let (tx, rx) = std::sync::mpsc::channel();
    staging
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    rx.recv().unwrap().unwrap();
    staging.slice(..).get_mapped_range().to_vec()
}

/// Mixed records over a 9x4x9 sub-chunk grid, with dead and cave-hidden slots.
fn mixed_records() -> (Vec<CullRecord>, Vec<u32>) {
    let mut random = Lcg(7);
    let mut records = Vec::new();
    for x in -4..=4 {
        for y in 2..=5 {
            for z in -4..=4 {
                let slot = records.len() as u32;
                if slot % 13 == 5 {
                    records.push(CullRecord::default());
                    continue;
                }
                let cube = if random.next(5) == 0 {
                    0
                } else {
                    8 + random.next(40)
                };
                let mut end = 0;
                let solid_ends = std::array::from_fn(|_| {
                    end += random.next(cube / 6 + 1);
                    end.min(cube)
                });
                let range = |random: &mut Lcg, start: u32| {
                    let count = if random.next(3) == 0 {
                        0
                    } else {
                        1 + random.next(9)
                    };
                    start..start + count
                };
                records.push(
                    CullRecord::new(&CullRecordSource {
                        origin: [x * 16, y * 16, z * 16],
                        base_vertex: slot as i32 * 4,
                        bounds: [[0; 3], [16; 3]],
                        cube: slot * 64..slot * 64 + cube,
                        solid_ends,
                        model: range(&mut random, 40_000 + slot * 16),
                        liquid: range(&mut random, 80_000 + slot * 16),
                    })
                    .unwrap(),
                );
            }
        }
    }
    let enabled = enabled_words(|slot| slot % 7 != 3, records.len());
    (records, enabled)
}

/// Both kernels parse and validate without a GPU, so CI without an adapter still checks them.
#[test]
fn cull_and_pyramid_kernels_validate() {
    for source in [
        kernels::cull_shader_source(),
        kernels::pyramid_shader_source().to_owned(),
    ] {
        let module = naga::front::wgsl::parse_str(&source).unwrap();
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }
}

/// Frustum and cave decisions match Bevy's CPU culling; compaction, facing runs and the
/// two-phase history match the CPU reference draw for draw.
#[test]
fn gpu_cull_matches_cpu_visibility_and_compacts_every_stream() {
    let Some(gpu) = Gpu::for_fixture("gpu terrain cull") else {
        return;
    };
    let (records, enabled) = mixed_records();
    let culler = Culler::new(&gpu, &records, &enabled);
    let cameras = [
        camera(Vec3::new(8.0, 70.0, 8.0), Vec3::new(40.0, 60.0, 60.0)),
        camera(Vec3::new(-20.5, 50.25, 3.0), Vec3::new(-80.0, 40.0, -10.0)),
        camera(Vec3::new(0.0, 120.0, 0.0), Vec3::new(1.0, 0.0, 0.5)),
        camera(Vec3::new(30.0, 64.0, -30.0), Vec3::new(0.0, 64.0, 0.0)),
    ];
    let mut checked_boundary_free = 0;
    for camera in &cameras {
        let input = view_input(camera, 0);
        let must = (0..records.len())
            .filter(|&slot| {
                records[slot].is_live()
                    && model::slot_enabled(&enabled, slot)
                    && bevy_visible(&camera.frustum, records[slot].origin)
            })
            .collect::<BTreeSet<_>>();
        let boundary = (0..records.len())
            .filter(|&slot| on_frustum_boundary(&camera.frustum, records[slot].origin))
            .collect::<BTreeSet<_>>();

        culler.set_history(&vec![0; records.len()]);
        let late = culler.run(&input, CullPhase::Late, None);
        let visible = culler.history();
        assert!(
            must.is_subset(&visible),
            "GPU culled a CPU-visible sub-chunk"
        );
        assert!(
            visible
                .difference(&must)
                .all(|slot| boundary.contains(slot)),
            "GPU kept a sub-chunk away from every frustum plane"
        );
        checked_boundary_free += usize::from(visible == must);
        let camera_split = CullCamera::new(Some(camera.eye.as_dvec3().to_array()));
        let expected = reference_args(&records, camera_split, INDEX_COUNTS, |slot| {
            visible.contains(&slot)
        });
        assert_eq!(late, expected, "late compaction");

        // Next frame draws everything early and nothing late.
        assert_eq!(
            culler.run(&input, CullPhase::Early, None),
            expected,
            "early compaction"
        );
        let empty: Args = Default::default();
        assert_eq!(culler.run(&input, CullPhase::Late, None), empty);
        assert_eq!(culler.history(), visible);

        // A stale history draws only its visible part early; late adds the rest exactly once.
        let stale = (0..records.len())
            .filter(|slot| slot % 3 == 0)
            .collect::<BTreeSet<_>>();
        culler.set_history(&bits_of(&stale, records.len()));
        let early = culler.run(&input, CullPhase::Early, None);
        let late = culler.run(&input, CullPhase::Late, None);
        let drawn_early = visible
            .intersection(&stale)
            .copied()
            .collect::<BTreeSet<_>>();
        assert_eq!(
            early,
            reference_args(&records, camera_split, INDEX_COUNTS, |s| drawn_early
                .contains(&s))
        );
        assert_eq!(
            late,
            reference_args(&records, camera_split, INDEX_COUNTS, |s| {
                visible.contains(&s) && !stale.contains(&s)
            })
        );
        assert_eq!(culler.history(), visible);
    }
    assert!(
        checked_boundary_free > 0,
        "some camera must compare identical sets"
    );
}

struct Target {
    color: wgpu::Texture,
    depth: wgpu::Texture,
    samples: u32,
}

impl Target {
    fn new(gpu: &Gpu, color: wgpu::TextureFormat, samples: u32) -> Self {
        Self::sized(gpu, color, samples, [SNAPSHOT_SIDE; 2])
    }

    fn sized(gpu: &Gpu, color: wgpu::TextureFormat, samples: u32, size: [u32; 2]) -> Self {
        let copy = if samples == 1 {
            wgpu::TextureUsages::COPY_SRC
        } else {
            wgpu::TextureUsages::empty()
        };
        let texture = |format, usage| {
            gpu.device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: samples,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: usage | wgpu::TextureUsages::RENDER_ATTACHMENT | copy,
                view_formats: &[],
            })
        };
        Self {
            color: texture(color, wgpu::TextureUsages::empty()),
            depth: texture(DEPTH_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING),
            samples,
        }
    }

    fn pass<'e>(&self, encoder: &'e mut wgpu::CommandEncoder, clear: bool) -> wgpu::RenderPass<'e> {
        let color = self.color.create_view(&Default::default());
        let depth = self.depth.create_view(&Default::default());
        let sky = wgpu::Color {
            r: 0.12,
            g: 0.18,
            b: 0.25,
            a: 1.0,
        };
        let (color_load, depth_load) = match clear {
            true => (wgpu::LoadOp::Clear(sky), wgpu::LoadOp::Clear(0.0)),
            false => (wgpu::LoadOp::Load, wgpu::LoadOp::Load),
        };
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &color,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: color_load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth,
                depth_ops: Some(wgpu::Operations {
                    load: depth_load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        })
    }

    fn pyramid(
        &self,
        gpu: &Gpu,
        kernels: &CullKernels,
        encoder: &mut wgpu::CommandEncoder,
    ) -> HizPyramid {
        let size = self.depth.size();
        let pyramid = HizPyramid::new(&gpu.device, [size.width, size.height]);
        let depth = self.depth.create_view(&Default::default());
        let bindings = kernels.pyramid_bindings(&gpu.device, &depth, self.samples > 1, &pyramid);
        kernels.encode_pyramid(encoder, &pyramid, &bindings);
        pyramid
    }
}

/// Reads mip `level` of a 4-byte-texel texture.
fn read_texture(gpu: &Gpu, texture: &wgpu::Texture, level: u32) -> Vec<u8> {
    let size = texture
        .size()
        .mip_level_size(level, wgpu::TextureDimension::D2);
    let row = (size.width * 4).next_multiple_of(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT);
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: u64::from(row * size.height),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: level,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &staging,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(size.height),
            },
        },
        size,
    );
    gpu.queue.submit([encoder.finish()]);
    let padded = map(gpu, &staging);
    padded
        .chunks_exact(row as usize)
        .flat_map(|line| line[..size.width as usize * 4].to_vec())
        .collect()
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytemuck::cast_slice::<u8, f32>(bytes).to_vec()
}

/// The six faces of a local cuboid, as greedy quads.
fn cuboid(low: [u8; 3], size: [u8; 3], material: u32) -> [PackedQuad; 6] {
    let [x, y, z] = low;
    let [w, h, d] = size;
    [
        PackedQuad::new(low, Face::NegativeX, d, h, material),
        PackedQuad::new([x + w - 1, y, z], Face::PositiveX, d, h, material),
        PackedQuad::new(low, Face::NegativeY, w, d, material),
        PackedQuad::new([x, y + h - 1, z], Face::PositiveY, w, d, material),
        PackedQuad::new(low, Face::NegativeZ, w, h, material),
        PackedQuad::new([x, y, z + d - 1], Face::PositiveZ, w, h, material),
    ]
}

#[derive(Default)]
struct Terrain {
    quads: Vec<PackedQuad>,
    origins: Vec<u32>,
    records: Vec<CullRecord>,
    /// Per slot: origin, cube range and layout, as the CPU path draws them.
    chunks: Vec<([i32; 3], std::ops::Range<u32>, CubeQuadLayout)>,
}

impl Terrain {
    /// Appends sub-chunk `key` as the next slot.
    fn add(&mut self, key: [i32; 3], solids: Vec<PackedQuad>, cutout: Vec<PackedQuad>) {
        let slot = self.records.len() as u32;
        let origin = key.map(|value| value * 16);
        let mut solids = solids;
        let order = |quad: &PackedQuad| {
            CubeQuadLayout::SOLID_FACE_ORDER
                .iter()
                .position(|&face| face == quad.face())
        };
        solids.sort_by_key(order);
        let mut counts = [0; 6];
        for quad in &solids {
            counts[quad.face() as usize] += 1;
        }
        let layout = CubeQuadLayout::from_solid_counts(counts);
        let start = self.quads.len() as u32;
        let mut bounds = [[16; 3], [0; 3]];
        for quad in solids.iter().chain(&cutout) {
            let low = quad.origin().map(i32::from);
            let extent = i32::from(quad.width().max(quad.height()));
            for axis in 0..3 {
                bounds[0][axis] = bounds[0][axis].min(low[axis]);
                bounds[1][axis] = bounds[1][axis].max((low[axis] + extent).min(16));
            }
        }
        self.quads.extend(solids);
        self.quads.extend(cutout);
        let end = self.quads.len() as u32;
        self.origins.extend([
            origin[0] as u32,
            origin[1] as u32,
            origin[2] as u32,
            0,
            start,
            start,
            0,
            0,
        ]);
        let solid_ends = CubeQuadLayout::SOLID_FACE_ORDER.map(|face| layout.solid_range(face).end);
        self.records.push(
            CullRecord::new(&CullRecordSource {
                origin,
                base_vertex: slot as i32 * 4,
                bounds,
                cube: start..end,
                solid_ends,
                ..Default::default()
            })
            .unwrap(),
        );
        self.chunks.push((origin, start..end, layout));
    }
}

/// A wall of slabs hiding block-filled sub-chunks, with open sides, a floor and cutout sheets.
fn terrain() -> Terrain {
    let mut terrain = Terrain::default();
    for x in -2..=2 {
        let slab = cuboid([0, 0, 8], [16, 16, 1], (x & 1) as u32);
        if x != 2 {
            terrain.add([x, 4, -2], slab.to_vec(), Vec::new());
        }
        for z in [-4, -5] {
            for y in [4, 5] {
                let blocks = [
                    cuboid([5, 5, 5], [3, 3, 3], 0),
                    cuboid([10, 12, 9], [2, 2, 2], 1),
                ];
                terrain.add([x, y, z], blocks.concat(), Vec::new());
            }
        }
    }
    for x in -1..=1 {
        let floor = cuboid([1, 13, 1], [14, 2, 14], 1);
        let sheet = vec![PackedQuad::new([3, 15, 3], Face::PositiveY, 4, 4, 0)];
        terrain.add([x, 3, -1], floor.to_vec(), sheet);
    }
    terrain
}

/// The CPU direct path's draws of one slot's stream: facing solid runs, or the cutout tail.
fn slot_draws(terrain: &Terrain, slot: usize, eye: [f64; 3], stream: CullStream) -> Vec<[u32; 5]> {
    let (origin, cube, layout) = &terrain.chunks[slot];
    let ranges = match stream {
        CullStream::Solid => layout
            .solid_runs(meshing::sub_chunk_facing_faces(*origin, eye))
            .map(|run| cube.start + run.start..cube.start + run.end)
            .collect::<Vec<_>>(),
        _ => std::iter::once(cube.start + layout.solid_len()..cube.end).collect(),
    };
    ranges
        .into_iter()
        .filter(|range| !range.is_empty())
        .map(|range| [6, range.end - range.start, 0, slot as u32 * 4, range.start])
        .collect()
}

struct Raster {
    solid: wgpu::RenderPipeline,
    cutout: wgpu::RenderPipeline,
    solid_group: wgpu::BindGroup,
    cutout_group: wgpu::BindGroup,
    indices: wgpu::Buffer,
    view: wgpu::Buffer,
}

impl Raster {
    fn new(gpu: &Gpu, terrain: &Terrain, camera: &Camera) -> Self {
        Self::with_depth(gpu, terrain, camera, true, wgpu::TextureFormat::Rgba8Unorm)
    }

    /// `write_depth` off draws against a finished depth, for per-slot visibility queries.
    fn with_depth(
        gpu: &Gpu,
        terrain: &Terrain,
        camera: &Camera,
        write_depth: bool,
        color: wgpu::TextureFormat,
    ) -> Self {
        let source = shader_source::standalone(include_str!("../../src/chunk.wgsl"), &[])
            .replace("@group(1) @binding(0)", "@group(0) @binding(20)");
        let module = gpu
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: None,
                source: wgpu::ShaderSource::Wgsl(source.into()),
            });
        let pipeline = |fragment, cull_mode| {
            gpu.device
                .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: None,
                    layout: None,
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some("vertex"),
                        compilation_options: Default::default(),
                        buffers: &[],
                    },
                    primitive: wgpu::PrimitiveState {
                        cull_mode,
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: write_depth,
                        depth_compare: wgpu::CompareFunction::GreaterEqual,
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some(fragment),
                        compilation_options: Default::default(),
                        targets: &[Some(wgpu::ColorTargetState {
                            format: color,
                            blend: None,
                            write_mask: if write_depth {
                                wgpu::ColorWrites::ALL
                            } else {
                                wgpu::ColorWrites::empty()
                            },
                        })],
                    }),
                    multiview: None,
                    cache: None,
                })
        };
        let solid = pipeline("fragment_solid", Some(wgpu::Face::Back));
        let cutout = pipeline("fragment", None);
        let storage = wgpu::BufferUsages::STORAGE;
        let quad_words = terrain
            .quads
            .iter()
            .flat_map(PackedQuad::words)
            .collect::<Vec<_>>();
        let quads = gpu.words(&quad_words, storage);
        let origins = gpu.words(&terrain.origins, storage);
        let no_animation = u32::MAX;
        let materials = gpu.words(
            &[0, 0, no_animation, 0, 0, 0, 1, 0, no_animation, 0, 0, 0],
            storage,
        );
        let animations = gpu.words(&[0; 8], storage);
        let animation_frames = gpu.words(&[0; 4], storage);
        let clock = gpu.words(&[0; 4], wgpu::BufferUsages::UNIFORM);
        let streams = gpu.words(
            &solid_terrain_raster::lighting_words(terrain.quads.len()),
            storage,
        );
        let records = gpu.buffer(&[0.0], storage);
        let tints = gpu.buffer(&[0.0; 8 + assets::SEASONAL_FOLIAGE_COUNT * 4], storage);
        let mut atmosphere = [0.0; 32];
        atmosphere[16..19].copy_from_slice(&[0.6, 0.7, 0.9]);
        atmosphere[19] = 8.0;
        atmosphere[20] = 60.0;
        let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
        let table = render::LightmapInputs::default().build();
        let lightmap = gpu.buffer(bytemuck::cast_slice(&table), wgpu::BufferUsages::UNIFORM);
        let atlas = solid_terrain_raster::pattern_texture(gpu);
        let sampler = gpu.device.create_sampler(&Default::default());
        let view = gpu.buffer(
            &gpu_snapshot::view(camera.clip_from_world, camera.eye),
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        );
        let group = |pipeline: &wgpu::RenderPipeline| {
            let entries = [
                (0, view.as_entire_binding()),
                (1, quads.as_entire_binding()),
                (2, origins.as_entire_binding()),
                (3, materials.as_entire_binding()),
                (6, wgpu::BindingResource::Sampler(&sampler)),
                (7, records.as_entire_binding()),
                (8, tints.as_entire_binding()),
                (9, animations.as_entire_binding()),
                (10, animation_frames.as_entire_binding()),
                (11, clock.as_entire_binding()),
                (13, streams.as_entire_binding()),
                (15, atmosphere.as_entire_binding()),
                (20, lightmap.as_entire_binding()),
                (
                    material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                    wgpu::BindingResource::TextureView(&atlas),
                ),
                (
                    material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                    wgpu::BindingResource::TextureView(&atlas),
                ),
                (
                    material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                    wgpu::BindingResource::Sampler(&sampler),
                ),
            ]
            .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(0),
                entries: &entries,
            })
        };
        Self {
            solid_group: group(&solid),
            cutout_group: group(&cutout),
            solid,
            cutout,
            indices: gpu.words(
                &chunk_constants::STATIC_QUAD_INDICES,
                wgpu::BufferUsages::INDEX,
            ),
            view,
        }
    }

    fn set_camera(&self, gpu: &Gpu, camera: &Camera) {
        let words = gpu_snapshot::view(camera.clip_from_world, camera.eye);
        gpu.queue
            .write_buffer(&self.view, 0, bytemuck::cast_slice(&words));
    }

    fn bind(&self, pass: &mut wgpu::RenderPass<'_>, stream: CullStream) {
        let (pipeline, group) = match stream {
            CullStream::Solid => (&self.solid, &self.solid_group),
            _ => (&self.cutout, &self.cutout_group),
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.set_index_buffer(self.indices.slice(..), wgpu::IndexFormat::Uint32);
    }

    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, stream: CullStream, args: &[[u32; 5]]) {
        self.bind(pass, stream);
        for &[count, instances, first_index, base_vertex, first_instance] in args {
            pass.draw_indexed(
                first_index..first_index + count,
                base_vertex as i32,
                first_instance..first_instance + instances,
            );
        }
    }
}

/// The two-phase GPU path, from any stale history, draws exactly the CPU path's pixels.
#[test]
fn gpu_culled_terrain_rasterises_exactly_like_the_cpu_culled_path() {
    // The args address quads through a non-zero `first_instance`, as production does.
    let features =
        wgpu::Features::MULTI_DRAW_INDIRECT_COUNT | wgpu::Features::INDIRECT_FIRST_INSTANCE;
    let Some(gpu) = Gpu::for_fixture_with("gpu culled terrain raster", features) else {
        return;
    };
    // Replay args on backends whose count draws cannot preserve the shader's base offsets.
    let indirect = gpu.device.features().contains(features)
        && model::count_draw_offsets_supported(gpu.backend);
    let terrain = terrain();
    let slots = terrain.records.len();
    let enabled = enabled_words(|slot| slot != 7, slots);
    let culler = Culler::new(&gpu, &terrain.records, &enabled);
    let mut random = Lcg(3);
    let mut history = (0..slots)
        .filter(|_| random.next(2) == 0)
        .collect::<BTreeSet<_>>();
    let cameras = [
        camera(Vec3::new(8.25, 72.5, 8.75), Vec3::new(10.0, 70.0, -40.0)),
        camera(Vec3::new(20.5, 75.0, 2.0), Vec3::new(0.0, 68.0, -60.0)),
        camera(Vec3::new(-30.0, 90.0, -8.0), Vec3::new(10.0, 60.0, -70.0)),
    ];
    let mut occluded_any = false;
    for (index, camera) in cameras.iter().enumerate() {
        let raster = Raster::new(&gpu, &terrain, camera);
        let eye = camera.eye.as_dvec3().to_array();
        let visible = (0..slots)
            .filter(|&slot| {
                model::slot_enabled(&enabled, slot)
                    && bevy_visible(&camera.frustum, terrain.chunks[slot].0)
            })
            .collect::<Vec<_>>();
        let mut front_to_back = visible.clone();
        let depth = |slot: usize| {
            (Vec3::from_array(terrain.chunks[slot].0.map(|v| v as f32)) - camera.eye).length()
        };
        front_to_back.sort_by(|&a, &b| depth(a).total_cmp(&depth(b)));

        // CPU path: facing solid runs front to back, then the cutout tails.
        let cpu = Target::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, 1);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let mut cpu_draws = 0;
        {
            let mut pass = cpu.pass(&mut encoder, true);
            for (stream, draws) in [CullStream::Solid, CullStream::Cutout].map(|stream| {
                let draws = front_to_back
                    .iter()
                    .flat_map(|&slot| slot_draws(&terrain, slot, eye, stream))
                    .collect::<Vec<_>>();
                (stream, draws)
            }) {
                cpu_draws += draws.len();
                raster.draw(&mut pass, stream, &draws);
            }
        }
        gpu.queue.submit([encoder.finish()]);

        // GPU path: early from the stale history, Hi-Z from that depth, then late.
        culler.set_history(&bits_of(&history, slots));
        let gpu_target = Target::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, 1);
        let mut gpu_draws = 0;
        let mut pyramid_mips = 0;
        for phase in CullPhase::ALL {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let pyramid = (phase == CullPhase::Late)
                .then(|| gpu_target.pyramid(&gpu, &culler.kernels, &mut encoder));
            if let Some(pyramid) = &pyramid {
                pyramid_mips = pyramid.mip_count();
            }
            let input = view_input(camera, pyramid_mips);
            culler.encode(&mut encoder, &input, phase, pyramid.as_ref());
            if indirect {
                let mut pass = gpu_target.pass(&mut encoder, phase == CullPhase::Early);
                for stream in [CullStream::Solid, CullStream::Cutout] {
                    raster.bind(&mut pass, stream);
                    pass.multi_draw_indexed_indirect_count(
                        &culler.storage.args,
                        u64::from(args_region(culler.storage.capacity, phase, stream)) * 4,
                        &culler.storage.draw_counts,
                        u64::from(count_index(phase, stream)) * 4,
                        culler.storage.capacity * stream.draws_per_record(),
                    );
                }
                drop(pass);
                gpu.queue.submit([encoder.finish()]);
                let args = culler.args(phase);
                gpu_draws += args.iter().map(Vec::len).sum::<usize>();
            } else {
                gpu.queue.submit([encoder.finish()]);
                let args = culler.args(phase);
                gpu_draws += args.iter().map(Vec::len).sum::<usize>();
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = gpu_target.pass(&mut encoder, phase == CullPhase::Early);
                    for stream in [CullStream::Solid, CullStream::Cutout] {
                        raster.draw(&mut pass, stream, &args[stream as usize]);
                    }
                }
                gpu.queue.submit([encoder.finish()]);
            }
        }
        let next = culler.history();
        assert!(next.iter().all(|slot| visible.contains(slot)));
        occluded_any |= next.len() < visible.len();
        history = next;

        let expected = read_texture(&gpu, &cpu.color, 0);
        let actual = read_texture(&gpu, &gpu_target.color, 0);
        gpu_snapshot::save(&format!("gpu_cull_cpu_{index}"), &expected);
        gpu_snapshot::save(&format!("gpu_cull_gpu_{index}"), &actual);
        let background = &expected[..4];
        assert!(
            expected.chunks_exact(4).any(|pixel| pixel != background),
            "camera {index} sees terrain"
        );
        let mismatched = expected
            .chunks_exact(4)
            .zip(actual.chunks_exact(4))
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(mismatched, 0, "camera {index}");
        eprintln!(
            "gpu cull camera {index}: cpu path {cpu_draws} draws over {} sub-chunks, gpu path {gpu_draws} draws ({} sub-chunks pass Hi-Z)",
            visible.len(),
            history.len()
        );
    }
    assert!(occluded_any, "the wall must occlude some sub-chunks");
}
