//! Greedy-merged cube quads must leave no seam pixels uncovered, and sealing
//! those seams must keep silhouettes and face interiors where they were.
use crate::{chunk_constants, gpu_snapshot, material_shader, shader_source};
use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu, RasterState, SNAPSHOT_SIDE};
use meshing::{Face, PackedQuad};

const SIDE: u8 = world::SUB_CHUNK_SIDE as u8;
/// Brightest sky and block light, no AO, so every terrain pixel stays distinct from the clear colour.
pub(crate) const FULL_LIGHT: u32 = 0xff | (0xff << 16);
/// Jittered camera positions rendered per seam scene.
const FRAMES: u32 = 48;

/// One sub-chunk's world origin and the cube quads it emits.
struct Section {
    origin: [i32; 3],
    quads: Vec<PackedQuad>,
}

/// Alternates unit and merged floor rows, creating fifteen T-junctions per merged edge.
fn flat_section(origin: [i32; 3], along_x: bool) -> Section {
    let mut quads = Vec::new();
    for row in 0..SIDE {
        let at = |along: u8| {
            if along_x {
                [along, 0, row]
            } else {
                [row, 0, along]
            }
        };
        if row % 2 == 0 {
            let (width, height) = if along_x { (SIDE, 1) } else { (1, SIDE) };
            quads.push(PackedQuad::new(at(0), Face::PositiveY, width, height, 0));
        } else {
            quads.extend(
                (0..SIDE).map(|along| PackedQuad::new(at(along), Face::PositiveY, 1, 1, 0)),
            );
        }
    }
    Section { origin, quads }
}

/// Builds terraces rising every two rows, with merged fronts or treads at convex and concave edges.
fn terrace_section(origin: [i32; 3], merged_fronts: bool) -> Section {
    let mut quads = Vec::new();
    for z in 0..SIDE {
        let y = z / 2;
        if merged_fronts {
            quads.extend((0..SIDE).map(|x| PackedQuad::new([x, y, z], Face::PositiveY, 1, 1, 0)));
        } else {
            quads.push(PackedQuad::new([0, y, z], Face::PositiveY, SIDE, 1, 0));
        }
        if z % 2 == 0 && z > 0 {
            if merged_fronts {
                quads.push(PackedQuad::new([0, y, z], Face::NegativeZ, SIDE, 1, 0));
            } else {
                quads.extend(
                    (0..SIDE).map(|x| PackedQuad::new([x, y, z], Face::NegativeZ, 1, 1, 0)),
                );
            }
        }
    }
    Section { origin, quads }
}

/// The six merged faces of a box of blocks from `min` with `size` blocks per axis.
fn merged_box(min: [u8; 3], size: [u8; 3]) -> [PackedQuad; 6] {
    let [x, y, z] = min;
    let [sx, sy, sz] = size;
    [
        PackedQuad::new(min, Face::NegativeX, sz, sy, 0),
        PackedQuad::new([x + sx - 1, y, z], Face::PositiveX, sz, sy, 0),
        PackedQuad::new(min, Face::NegativeY, sx, sz, 0),
        PackedQuad::new([x, y + sy - 1, z], Face::PositiveY, sx, sz, 0),
        PackedQuad::new(min, Face::NegativeZ, sx, sy, 0),
        PackedQuad::new([x, y, z + sz - 1], Face::PositiveZ, sx, sy, 0),
    ]
}

/// Separate single blocks and merged boxes. Their faces meet only at shared
/// corners, never at T-junctions, so each silhouette is closed without help.
fn box_section(origin: [i32; 3]) -> Section {
    let boxes = [
        ([1, 1, 1], [1, 1, 1]),
        ([4, 2, 6], [1, 1, 1]),
        ([12, 5, 11], [1, 1, 1]),
        ([6, 9, 9], [1, 1, 1]),
        ([9, 1, 2], [5, 3, 1]),
        ([1, 6, 2], [1, 4, 6]),
        ([3, 12, 10], [7, 1, 4]),
        ([12, 10, 3], [2, 2, 2]),
    ];
    Section {
        origin,
        quads: boxes
            .iter()
            .flat_map(|&(min, size)| merged_box(min, size))
            .collect(),
    }
}

/// Sections spanning `columns` x `rows` sub-chunks from `corner`, alternating layouts in a checkerboard.
fn terrain(
    corner: [i32; 3],
    columns: i32,
    rows: i32,
    section: fn([i32; 3], bool) -> Section,
) -> Vec<Section> {
    let side = i32::from(SIDE);
    (0..rows)
        .flat_map(|row| (0..columns).map(move |column| (column, row)))
        .map(|(column, row)| {
            section(
                [corner[0] + column * side, corner[1], corner[2] + row * side],
                (column + row) % 2 == 0,
            )
        })
        .collect()
}

/// Stores quads, origins and lighting, prefixing geometry with per-quad section indices.
/// Lighting bases skip that prefix, which is padded to whole records.
struct Streams {
    quads: wgpu::Buffer,
    origins: wgpu::Buffer,
    geometry: wgpu::Buffer,
    quad_count: u32,
}

impl Streams {
    /// Uploads packed quads, origins and lighting for the terrain fixture.
    fn new(gpu: &Gpu, terrain: &[Section]) -> Self {
        let quad_count = terrain
            .iter()
            .map(|section| section.quads.len())
            .sum::<usize>();
        let skipped_records = quad_count.div_ceil(2) as u32;
        let (mut quads, mut origins) = (Vec::new(), Vec::new());
        let mut geometry = Vec::with_capacity(4 * quad_count);
        for (index, section) in terrain.iter().enumerate() {
            let first = (quads.len() / 2) as u32;
            origins.extend(section.origin.map(|value| value as u32));
            origins.extend([0, first, skipped_records + first, 0, 0]);
            quads.extend(section.quads.iter().flat_map(PackedQuad::words));
            geometry.extend(std::iter::repeat_n(index as u32, section.quads.len()));
        }
        geometry.resize(2 * skipped_records as usize, 0);
        geometry.resize(geometry.len() + 2 * quad_count, FULL_LIGHT);
        let storage = wgpu::BufferUsages::STORAGE;
        Self {
            quads: gpu.words(&quads, storage),
            origins: gpu.words(&origins, storage),
            geometry: gpu.words(&geometry, storage),
            quad_count: quad_count as u32,
        }
    }
}

/// A two-layer gradient texture with a box-filtered mip chain, so distant
/// faces sample the mip level production terrain would.
fn mipmapped_pattern(gpu: &Gpu) -> wgpu::TextureView {
    const TEXELS: u32 = 16;
    const LEVELS: u32 = 5;
    const LAYERS: u32 = 2;
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("seam pattern"),
        size: wgpu::Extent3d {
            width: TEXELS,
            height: TEXELS,
            depth_or_array_layers: LAYERS,
        },
        mip_level_count: LEVELS,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for level in 0..LEVELS {
        let side = TEXELS >> level;
        let span = 1 << level;
        // Mean of the base texels `u * 16` for u in [x * span, (x + 1) * span).
        let mean = |x: u32| (16 * (x * span) + 8 * (span - 1)) as u8;
        let texels = (0..LAYERS * side * side)
            .flat_map(|index| {
                let (layer, texel) = (index / (side * side), index % (side * side));
                [
                    mean(texel % side),
                    mean(texel / side),
                    (40 + layer * 150) as u8,
                    255,
                ]
            })
            .collect::<Vec<_>>();
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: level,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &texels,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(side * 4),
                rows_per_image: Some(side),
            },
            wgpu::Extent3d {
                width: side,
                height: side,
                depth_or_array_layers: LAYERS,
            },
        );
    }
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// Shared material, texture and atmosphere fixtures; fog is disabled so far terrain keeps its texels.
struct Fixture {
    gpu: Gpu,
    materials: wgpu::Buffer,
    animations: wgpu::Buffer,
    animation_frames: wgpu::Buffer,
    clock: wgpu::Buffer,
    records: wgpu::Buffer,
    tints: wgpu::Buffer,
    query_tables: wgpu::Buffer,
    atmosphere: wgpu::Buffer,
    lightmap: wgpu::Buffer,
    atlas: wgpu::TextureView,
    sampler: wgpu::Sampler,
    source: String,
}

impl Fixture {
    /// Creates production seam resources, skipping an absent native GPU adapter.
    fn new(name: &str) -> Option<Self> {
        let gpu = Gpu::for_fixture(name)?;
        let storage = wgpu::BufferUsages::STORAGE;
        let mut atmosphere = [0.0; 32];
        atmosphere[16..19].copy_from_slice(&[0.6, 0.7, 0.9]);
        atmosphere[19] = 1.0e6;
        atmosphere[20] = 2.0e6;
        let table = render::LightmapInputs::default().build();
        let source = format!(
            "{}\n{}",
            shader_source::standalone(include_str!("../../src/chunk.wgsl"), &[]),
            SEAM_SHADER.replace(
                "INDICES",
                &chunk_constants::STATIC_QUAD_INDICES
                    .map(|index| format!("{index}u"))
                    .join(", "),
            ),
        )
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)");
        Some(Self {
            materials: gpu.words(&[0, 0, u32::MAX, 0, 0, 0], storage),
            animations: gpu.words(&[0; 8], storage),
            animation_frames: gpu.words(&[0; 4], storage),
            clock: gpu.words(&[0; 4], wgpu::BufferUsages::UNIFORM),
            records: gpu.buffer(&[0.0], storage),
            tints: gpu.buffer(&[0.0; 8 + assets::SEASONAL_FOLIAGE_COUNT * 4], storage),
            query_tables: gpu.words(
                &meshing::biome_lattice::query_table_words(),
                wgpu::BufferUsages::UNIFORM,
            ),
            atmosphere: gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM),
            lightmap: gpu.buffer(bytemuck::cast_slice(&table), wgpu::BufferUsages::UNIFORM),
            atlas: mipmapped_pattern(&gpu),
            sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                address_mode_w: wgpu::AddressMode::Repeat,
                ..material_shader::native_leaf_sampler_descriptor()
            }),
            source,
            gpu,
        })
    }

    /// Renders `streams` through the test's `vertex` and `fragment` entry points.
    fn render(&self, streams: &Streams, view: &View, vertex: &str, fragment: &str) -> Vec<u8> {
        let uniform = self.gpu.buffer(
            &gpu_snapshot::view(view.clip_from_world, view.eye),
            wgpu::BufferUsages::UNIFORM,
        );
        let bindings = [
            (0, uniform.as_entire_binding()),
            (1, streams.quads.as_entire_binding()),
            (2, streams.origins.as_entire_binding()),
            (3, self.materials.as_entire_binding()),
            (6, wgpu::BindingResource::Sampler(&self.sampler)),
            (7, self.records.as_entire_binding()),
            (8, self.tints.as_entire_binding()),
            (
                material_shader::BIOME_QUERY_TABLES_BINDING,
                self.query_tables.as_entire_binding(),
            ),
            (9, self.animations.as_entire_binding()),
            (10, self.animation_frames.as_entire_binding()),
            (11, self.clock.as_entire_binding()),
            (13, streams.geometry.as_entire_binding()),
            (15, self.atmosphere.as_entire_binding()),
            (20, self.lightmap.as_entire_binding()),
            (
                material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0],
                wgpu::BindingResource::TextureView(&self.atlas),
            ),
            (
                material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1],
                wgpu::BindingResource::TextureView(&self.atlas),
            ),
            (
                material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                wgpu::BindingResource::Sampler(&self.sampler),
            ),
        ]
        .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource });
        let draw = Draw {
            fragment,
            vertices: 0..streams.quad_count * 6,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        };
        let state = RasterState {
            primitive: wgpu::PrimitiveState {
                cull_mode: Some(wgpu::Face::Back),
                ..Default::default()
            },
            ..Default::default()
        };
        self.gpu
            .render_with_state(&self.source, vertex, &[draw], state)
    }

    /// The clear colour, read from a view that looks straight up past all terrain.
    fn background(&self, streams: &Streams, eye: Vec3) -> Vec<u8> {
        let sky = View::new(eye, 0.0, -1.5, 0.6);
        let pixels = self.render(streams, &sky, "seam_vertex", "fragment_solid");
        let background = pixels[..4].to_vec();
        assert!(
            pixels
                .as_chunks::<4>()
                .0
                .iter()
                .all(|pixel| pixel.as_slice() == background),
            "the sky view must miss every quad"
        );
        background
    }
}

/// A perspective camera for one snapshot.
pub(crate) struct View {
    pub(crate) eye: Vec3,
    pub(crate) clip_from_world: Mat4,
}

impl View {
    /// Looks from `eye`, turned by `yaw` from +Z and pitched down by `pitch` radians.
    pub(crate) fn new(eye: Vec3, yaw: f32, pitch: f32, fov_y: f32) -> Self {
        let direction = Vec3::new(
            yaw.sin() * pitch.cos(),
            -pitch.sin(),
            yaw.cos() * pitch.cos(),
        );
        Self::toward(eye, eye + direction, fov_y)
    }

    /// Creates a perspective view aimed from eye toward target.
    fn toward(eye: Vec3, target: Vec3, fov_y: f32) -> Self {
        Self {
            eye,
            clip_from_world: Mat4::perspective_infinite_reverse_rh(fov_y, 1.0, 0.05)
                * Mat4::look_at_rh(eye, target, Vec3::Y),
        }
    }
}

/// Low-discrepancy camera jitter: sub-block translation plus a small yaw and pitch change.
pub(crate) fn jitter(frame: u32) -> (Vec3, f32, f32) {
    let fraction = |scale: f64| ((f64::from(frame) + 1.0) * scale).fract() as f32;
    (
        Vec3::new(
            fraction(0.618_033_988_7),
            fraction(0.414_213_562_4) * 0.5,
            fraction(0.732_050_807_6),
        ),
        (fraction(0.236_067_977_5) - 0.5) * 0.2,
        (fraction(0.302_775_637_7) - 0.5) * 0.1,
    )
}

/// Reads one RGBA pixel of a snapshot.
pub(crate) fn pixel(pixels: &[u8], x: usize, y: usize) -> &[u8] {
    &pixels[4 * (y * SNAPSHOT_SIDE as usize + x)..][..4]
}

/// Counts uncovered pixels: everywhere in full-frame views, or below each column's horizon.
pub(crate) fn uncovered_pixels(pixels: &[u8], background: &[u8], full_frame: bool) -> usize {
    let side = SNAPSHOT_SIDE as usize;
    let is_background = |x: usize, y: usize| pixel(pixels, x, y) == background;
    (0..side)
        .map(|x| {
            let first = if full_frame {
                0
            } else {
                (0..side).find(|&y| !is_background(x, y)).unwrap_or(side)
            };
            (first..side).filter(|&y| is_background(x, y)).count()
        })
        .sum()
}

/// Terrain sections with the camera that looks across them.
struct Scene {
    name: &'static str,
    terrain: Vec<Section>,
    eye: Vec3,
    yaw: f32,
    pitch: f32,
    fov_y: f32,
    /// Scales the jittered eye translation.
    jitter: f32,
    /// Scales the jittered yaw and pitch.
    turn: f32,
    full_frame: bool,
}

impl Scene {
    /// Lays 12 x 14 sections north of world column `centre`, viewed from 67
    /// blocks above the floor so the terrain fills the frame.
    fn distant(
        name: &'static str,
        section: fn([i32; 3], bool) -> Section,
        centre: [i32; 2],
    ) -> Self {
        Self {
            name,
            terrain: terrain([centre[0] - 96, 0, centre[1]], 12, 14, section),
            eye: Vec3::new(centre[0] as f32, 68.0, centre[1] as f32 - 20.0),
            yaw: 0.0,
            pitch: 0.66,
            fov_y: 0.6,
            jitter: 1.0,
            turn: 1.0,
            full_frame: true,
        }
    }

    /// A wide view along the floor toward the horizon, where the in-plane
    /// axes of far faces project nearly parallel.
    fn horizon(
        name: &'static str,
        section: fn([i32; 3], bool) -> Section,
        centre: [i32; 2],
    ) -> Self {
        Self {
            name,
            terrain: terrain([centre[0] - 96, 0, centre[1]], 12, 14, section),
            eye: Vec3::new(centre[0] as f32, 12.0, centre[1] as f32 + 8.0),
            yaw: 0.0,
            pitch: 0.12,
            fov_y: 1.2,
            jitter: 1.0,
            turn: 1.0,
            full_frame: false,
        }
    }

    /// A steep view from two blocks above the floor, where seams span many pixels.
    fn near(name: &'static str, section: fn([i32; 3], bool) -> Section, centre: [i32; 2]) -> Self {
        Self {
            name,
            terrain: terrain([centre[0] - 48, 0, centre[1]], 6, 8, section),
            eye: Vec3::new(centre[0] as f32, 3.0, centre[1] as f32 + 32.0),
            yaw: 0.0,
            pitch: 0.85,
            fov_y: 1.2,
            jitter: 0.25,
            turn: 1.0,
            full_frame: true,
        }
    }

    /// Builds a narrow diagonal view 26–300 blocks away, where floor quads project as thin slivers.
    fn grazing(
        name: &'static str,
        section: fn([i32; 3], bool) -> Section,
        centre: [i32; 2],
    ) -> Self {
        Self {
            name,
            terrain: terrain([centre[0], 0, centre[1]], 14, 14, section),
            eye: Vec3::new(centre[0] as f32 + 2.0, 2.62, centre[1] as f32 + 2.0),
            yaw: std::f32::consts::FRAC_PI_4,
            pitch: (1.62_f32 / 150.0).atan(),
            fov_y: 0.1,
            jitter: 1.0,
            turn: 0.05,
            full_frame: false,
        }
    }

    /// Frames, among jittered views drawn through `vertex`, that leave
    /// terrain pixels uncovered, with their uncovered pixel counts.
    fn cracks(
        &self,
        fixture: &Fixture,
        streams: &Streams,
        background: &[u8],
        vertex: &str,
    ) -> Vec<(u32, usize)> {
        let mut cracked = Vec::new();
        for frame in 0..FRAMES {
            let (offset, yaw, pitch) = jitter(frame);
            let view = View::new(
                self.eye + offset * self.jitter,
                self.yaw + yaw * self.turn,
                self.pitch + pitch * self.turn,
                self.fov_y,
            );
            let pixels = fixture.render(streams, &view, vertex, "fragment_solid");
            let count = uncovered_pixels(&pixels, background, self.full_frame);
            if count > 0 {
                gpu_snapshot::save(&format!("{}-{vertex}-{frame}", self.name), &pixels);
                cracked.push((frame, count));
            }
        }
        cracked
    }

    /// Fails on any frame that leaves terrain pixels uncovered, after the
    /// same views drawn unsealed show that this adapter opens the cracks.
    fn assert_sealed(&self) {
        let Some(fixture) = Fixture::new(self.name) else {
            return;
        };
        let streams = Streams::new(&fixture.gpu, &self.terrain);
        let background = fixture.background(&streams, self.eye);
        if self
            .cracks(&fixture, &streams, &background, "unsealed_vertex")
            .is_empty()
        {
            eprintln!(
                "skipping {}: missing crack-reproducing adapter fixture (unsealed T-junctions left no gaps)",
                self.name
            );
            return;
        }
        let cracked = self.cracks(&fixture, &streams, &background, "seam_vertex");
        assert!(
            cracked.is_empty(),
            "{}: the clear colour shows through terrain seams (frame, pixels): {cracked:?}",
            self.name
        );
    }
}

#[test]
fn distant_merged_floors_leave_no_seam_pixels() {
    Scene::distant("merged floor seams", flat_section, [0, 0]).assert_sealed();
}

#[test]
fn distant_merged_terraces_leave_no_seam_pixels() {
    Scene::distant("merged terrace seams", terrace_section, [0, 0]).assert_sealed();
}

#[test]
fn horizon_merged_floors_leave_no_seam_pixels() {
    Scene::horizon("horizon floor seams", flat_section, [0, 0]).assert_sealed();
}

#[test]
fn grazing_diagonal_merged_floors_leave_no_seam_pixels() {
    Scene::grazing("grazing floor seams", flat_section, [0, 0]).assert_sealed();
}

#[test]
fn near_merged_floors_far_from_origin_leave_no_seam_pixels() {
    Scene::near("far merged floor seams", flat_section, [20_000, -20_000]).assert_sealed();
}

/// How sealing changed one view against the same quads drawn unsealed.
#[derive(Debug, Default)]
struct SealChange {
    /// Unsealed terrain pixels with a background 4-neighbour.
    silhouette: usize,
    /// Background pixels that became terrain.
    grown: usize,
    /// Grown pixels with no unsealed terrain among their 8 neighbours.
    stray: usize,
    /// Terrain pixels that became background.
    lost: usize,
    /// Pixels that show the same quad in both frames.
    same_face: usize,
    /// Same-face pixels whose colour changed.
    recoloured: usize,
}

impl SealChange {
    /// Compares quad-id and colour snapshots, unsealed first.
    fn new(ids: [&[u8]; 2], colours: [&[u8]; 2], background: &[u8]) -> Self {
        let side = SNAPSHOT_SIDE as i32;
        let covered = |pixels: &[u8], x: i32, y: i32| {
            (0..side).contains(&x)
                && (0..side).contains(&y)
                && pixel(pixels, x as usize, y as usize) != background
        };
        let mut change = Self::default();
        for y in 0..side {
            for x in 0..side {
                let (before, after) = (covered(ids[0], x, y), covered(ids[1], x, y));
                let (column, row) = (x as usize, y as usize);
                change.lost += usize::from(before && !after);
                if !before && after {
                    change.grown += 1;
                    let near =
                        (-1..=1).any(|dy| (-1..=1).any(|dx| covered(ids[0], x + dx, y + dy)));
                    change.stray += usize::from(!near);
                }
                if before && after && pixel(ids[0], column, row) == pixel(ids[1], column, row) {
                    change.same_face += 1;
                    change.recoloured += usize::from(
                        pixel(colours[0], column, row) != pixel(colours[1], column, row),
                    );
                }
                let edge = [(1, 0), (-1, 0), (0, 1), (0, -1)]
                    .iter()
                    .any(|&(dx, dy)| !covered(ids[0], x + dx, y + dy));
                change.silhouette += usize::from(before && edge);
            }
        }
        change
    }
}

/// Seal changes for jittered copies of each `(eye, target, fov_y)` view of `terrain`.
fn seal_changes(name: &str, terrain: &[Section], views: &[(Vec3, Vec3, f32)]) -> Vec<SealChange> {
    let Some(fixture) = Fixture::new(name) else {
        return Vec::new();
    };
    let streams = Streams::new(&fixture.gpu, terrain);
    let background = fixture.background(&streams, views[0].1 + Vec3::Y * 100.0);
    let mut changes = Vec::new();
    for (index, &(eye, target, fov_y)) in views.iter().enumerate() {
        for frame in 0..8 {
            let view = View::toward(eye + jitter(frame).0 * 0.1, target, fov_y);
            let render = |vertex, fragment| fixture.render(&streams, &view, vertex, fragment);
            let ids = [
                render("unsealed_id_vertex", "quad_id_fragment"),
                render("seam_id_vertex", "quad_id_fragment"),
            ];
            let colours = [
                render("unsealed_vertex", "fragment_solid"),
                render("seam_vertex", "fragment_solid"),
            ];
            if frame == 0 {
                gpu_snapshot::save(&format!("{name}-{index}-unsealed"), &colours[0]);
                gpu_snapshot::save(&format!("{name}-{index}-sealed"), &colours[1]);
            }
            changes.push(SealChange::new(
                [&ids[0], &ids[1]],
                [&colours[0], &colours[1]],
                &background,
            ));
        }
    }
    changes
}

#[test]
fn sealing_grows_silhouettes_by_less_than_a_pixel() {
    let centre = Vec3::new(24.0, 72.0, -8.0);
    let views = [
        (centre + Vec3::new(-14.0, 18.0, -22.0), centre, 1.0),
        (centre + Vec3::new(30.0, -12.0, -26.0), centre, 1.0),
        (centre + Vec3::new(-60.0, 45.0, -80.0), centre, 0.6),
        (centre + Vec3::new(2.0, 1.5, -40.0), centre, 0.6),
        (centre + Vec3::new(160.0, 90.0, -200.0), centre, 0.3),
    ];
    let terrain = [box_section([16, 64, -16])];
    for (snapshot, change) in seal_changes("sealed silhouettes", &terrain, &views)
        .iter()
        .enumerate()
    {
        assert!(
            change.silhouette > 200,
            "snapshot {snapshot} shows the boxes: {change:?}"
        );
        assert_eq!(
            change.lost, 0,
            "snapshot {snapshot} uncovered terrain: {change:?}"
        );
        assert_eq!(
            change.stray, 0,
            "snapshot {snapshot} grew by a whole pixel: {change:?}"
        );
        assert!(
            change.grown * 16 <= change.silhouette,
            "snapshot {snapshot} grew more than a sliver of its outline: {change:?}"
        );
    }
}

#[test]
fn sealing_keeps_face_interiors_in_place() {
    let terrain = terrain([-48, 0, 0], 6, 8, terrace_section);
    let views = [
        (Vec3::new(0.0, 14.0, 10.0), Vec3::new(3.0, 4.0, 30.0), 1.2),
        (Vec3::new(0.0, 40.0, -20.0), Vec3::new(0.0, 4.0, 60.0), 0.8),
        (
            Vec3::new(-20.0, 10.0, 40.0),
            Vec3::new(10.0, 6.0, 44.0),
            0.5,
        ),
    ];
    for (snapshot, change) in seal_changes("sealed interiors", &terrain, &views)
        .iter()
        .enumerate()
    {
        assert!(
            change.same_face > 30_000,
            "snapshot {snapshot} shows terraces: {change:?}"
        );
        // Rounding alone recolours about one pixel in a hundred; stretching
        // the texture over the grown quad instead recolours about one in six.
        assert!(
            change.recoloured * 25 <= change.same_face,
            "snapshot {snapshot} moved texels inside faces: {change:?}"
        );
    }
}

/// Wraps production cube vertices with section indices, unsealed controls and encoded quad ids.
const SEAM_SHADER: &str = r#"
/// Maps witness vertices through the production static quad indices.
fn seam_corner(index: u32) -> u32 {
    var indices = array<u32, 6>(INDICES);
    return geometry_streams[index / 6u] * 4u + indices[index % 6u];
}

/// Draws sealed production cube vertices using each quad's section index.
@vertex fn seam_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    return cube_vertex(seam_corner(index), index / 6u);
}

/// Draws the same cube vertices with seam growth disabled.
@vertex fn unsealed_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    return sealed_cube_vertex(seam_corner(index), index / 6u, 0.0);
}

/// Draws sealed vertices with a unique identifier for each quad.
@vertex fn seam_id_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var out = cube_vertex(seam_corner(index), index / 6u);
    out.biome_record = index / 6u + 1u;
    return out;
}

/// Draws unsealed vertices with a unique identifier for each quad.
@vertex fn unsealed_id_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var out = sealed_cube_vertex(seam_corner(index), index / 6u, 0.0);
    out.biome_record = index / 6u + 1u;
    return out;
}

// Shades as usual so the bindings match, then reports the quad id instead.
/// Encodes the quad identifier in pixels while retaining the color shader as a witness.
@fragment fn quad_id_fragment(in: VertexOutput) -> @location(0) vec4<f32> {
    let colour = shade_cube(in, sample_cube_texture(in, dpdx(in.uv), dpdy(in.uv)));
    return select(unpack4x8unorm(in.biome_record | 0xff000000u), colour, colour.a < -1.0);
}
"#;
