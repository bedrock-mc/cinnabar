//! Cube, model and liquid passes must project a shared world corner alike, so
//! floors that mix cubes with slabs and water stay closed far from the origin.
use crate::terrain_seams::{FULL_LIGHT, View, jitter, uncovered_pixels};
use crate::{chunk_constants, gpu_snapshot, material_shader, shader_source, solid_terrain_raster};
use bevy::math::Vec3;
use gpu_snapshot::{Draw, DrawPipeline, Gpu, RasterState};
use meshing::liquid::{LIQUID_DEPTH_WRITE_BIT, LIQUID_TWO_SIDED_BIT};
use meshing::{Face, PackedLiquidQuad, PackedModelDrawRef, PackedModelRef, PackedQuad};

const SIDE: u8 = world::SUB_CHUNK_SIDE as u8;
const FRAMES: u32 = 24;

/// Builds a y=1 floor alternating merged rows with cube, top-slab and water unit faces.
/// Their shared corners create T-junctions across the three render passes.
#[derive(Default)]
struct Floor {
    origins: Vec<[i32; 3]>,
    cubes: Vec<(u32, PackedQuad)>,
    slabs: Vec<(u32, [u8; 3])>,
    water: Vec<(u32, [u8; 3])>,
}

impl Floor {
    /// Lays `columns` x `rows` sections from `corner`.
    fn new(corner: [i32; 3], columns: i32, rows: i32) -> Self {
        let mut floor = Self::default();
        let side = i32::from(SIDE);
        for row in 0..rows {
            for column in 0..columns {
                let section = floor.origins.len() as u32;
                floor
                    .origins
                    .push([corner[0] + column * side, corner[1], corner[2] + row * side]);
                for z in 0..SIDE {
                    if z % 2 == 0 {
                        let quad = PackedQuad::new([0, 0, z], Face::PositiveY, SIDE, 1, 0);
                        floor.cubes.push((section, quad));
                        continue;
                    }
                    for x in 0..SIDE {
                        match x % 4 {
                            1 => floor.slabs.push((section, [x, 0, z])),
                            3 => floor.water.push((section, [x, 0, z])),
                            _ => floor.cubes.push((
                                section,
                                PackedQuad::new([x, 0, z], Face::PositiveY, 1, 1, 0),
                            )),
                        }
                    }
                }
            }
        }
        floor
    }
}

/// Words for a storage binding that a pass may not read but must still bind.
const UNUSED: [u32; 64] = [0; 64];

/// GPU buffers for the three passes, with each quad's section stored after
/// its pass's own geometry stream words.
struct Streams {
    origins: wgpu::Buffer,
    cube_quads: wgpu::Buffer,
    cube_geometry: wgpu::Buffer,
    model_templates: wgpu::Buffer,
    model_geometry: wgpu::Buffer,
    liquid_geometry: wgpu::Buffer,
    /// First section-map word of the model and liquid geometry streams.
    model_sections: u32,
    liquid_sections: u32,
}

impl Streams {
    /// Uploads mixed-floor cube, model and liquid streams with section indices.
    fn new(gpu: &Gpu, floor: &Floor) -> Self {
        let storage = wgpu::BufferUsages::STORAGE;
        let cube_count = floor.cubes.len();
        let skipped = cube_count.div_ceil(2) as u32;
        let origins = floor
            .origins
            .iter()
            .flat_map(|origin| {
                [
                    origin[0] as u32,
                    origin[1] as u32,
                    origin[2] as u32,
                    0,
                    0,
                    skipped,
                    0,
                    0,
                ]
            })
            .collect::<Vec<_>>();
        let mut cube_geometry = floor
            .cubes
            .iter()
            .map(|&(section, _)| section)
            .collect::<Vec<_>>();
        cube_geometry.resize(2 * skipped as usize, 0);
        cube_geometry.resize(cube_geometry.len() + 2 * cube_count, FULL_LIGHT);

        // Draw refs, then model refs, lighting records and the section map.
        let slabs = floor.slabs.len();
        let ref_word = (2 * slabs).next_multiple_of(4);
        let light_word = ref_word + 4 * slabs;
        let mut model_geometry = (0..slabs)
            .flat_map(|index| PackedModelDrawRef::new((ref_word / 4 + index) as u32, 0).words())
            .collect::<Vec<_>>();
        model_geometry.resize(ref_word, 0);
        for (index, &(_, [x, y, z])) in floor.slabs.iter().enumerate() {
            let transform = u32::from(x) | (u32::from(y) << 4) | (u32::from(z) << 8);
            model_geometry.extend(
                PackedModelRef::new(transform, 0, (light_word / 2 + index) as u32, 1).words(),
            );
        }
        model_geometry.resize(light_word + 2 * slabs, FULL_LIGHT);
        let model_sections = model_geometry.len() as u32;
        model_geometry.extend(floor.slabs.iter().map(|&(section, _)| section));

        // Liquid records, then lighting records and the section map.
        let water = floor.water.len();
        let mut liquid_geometry = Vec::with_capacity(7 * water);
        for (index, &(_, origin)) in floor.water.iter().enumerate() {
            let mut words = PackedLiquidQuad::try_pack(
                origin,
                Face::PositiveY,
                [255; 4],
                0,
                (2 * water + index) as u32,
                [0; 2],
                false,
            )
            .expect("full water top")
            .words();
            words[2] |= LIQUID_DEPTH_WRITE_BIT | LIQUID_TWO_SIDED_BIT;
            liquid_geometry.extend(words);
        }
        liquid_geometry.resize(6 * water, FULL_LIGHT);
        let liquid_sections = liquid_geometry.len() as u32;
        liquid_geometry.extend(floor.water.iter().map(|&(section, _)| section));

        Self {
            origins: gpu.words(&origins, storage),
            cube_quads: gpu.words(
                &floor
                    .cubes
                    .iter()
                    .flat_map(|(_, quad)| quad.words())
                    .collect::<Vec<_>>(),
                storage,
            ),
            cube_geometry: gpu.words(&cube_geometry, storage),
            model_templates: gpu.words(&slab_top_template(), storage),
            model_geometry: gpu.words(&model_geometry, storage),
            liquid_geometry: gpu.words(&liquid_geometry, storage),
            model_sections,
            liquid_sections,
        }
    }
}

/// One model template holding a top slab's two-sided upward face at y = 1.
fn slab_top_template() -> Vec<u32> {
    let positions: [i16; 12] = [0, 256, 0, 0, 256, 256, 256, 256, 256, 256, 256, 0];
    let uvs: [u16; 8] = [0, 0, 0, 4096, 4096, 4096, 4096, 0];
    let mut words = vec![1, 0, 1, 0];
    words.extend(
        positions
            .chunks_exact(2)
            .map(|pair| u32::from(pair[0] as u16) | (u32::from(pair[1] as u16) << 16)),
    );
    words.extend(
        uvs.chunks_exact(2)
            .map(|pair| u32::from(pair[0]) | (u32::from(pair[1]) << 16)),
    );
    // Material zero; upward normal, two-sided.
    words.extend([0, 2 | 8]);
    words
}

/// Global bindings each named entry point actually reads, found with naga so
/// a pass's bind group lists exactly what its derived layout expects.
fn used_bindings(source: &str, entries: [&str; 2]) -> Vec<u32> {
    let module = naga::front::wgsl::parse_str(source)
        .unwrap_or_else(|error| panic!("{}", error.emit_to_string(source)));
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .expect("world seam shader validates");
    let mut used = Vec::new();
    for (index, entry) in module.entry_points.iter().enumerate() {
        if !entries.contains(&entry.name.as_str()) {
            continue;
        }
        let function = info.get_entry_point(index);
        for (handle, variable) in module.global_variables.iter() {
            if let Some(binding) = &variable.binding
                && !function[handle].is_empty()
                && !used.contains(&binding.binding)
            {
                used.push(binding.binding);
            }
        }
    }
    used
}

/// Production world shaders with the test's section-mapped entry points.
struct Shaders {
    cube: String,
    model: String,
    liquid: String,
}

impl Shaders {
    /// Composes production shaders with section-mapped witness entry points.
    fn new(streams: &Streams) -> Self {
        let indices = chunk_constants::STATIC_QUAD_INDICES
            .map(|index| format!("{index}u"))
            .join(", ");
        let prepare = |source: &str, entries: &str| {
            format!(
                "{}\n{}",
                shader_source::standalone(source, &[]),
                entries.replace("INDICES", &indices)
            )
            .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
        };
        Self {
            cube: prepare(include_str!("../../src/chunk.wgsl"), CUBE_ENTRIES),
            model: prepare(
                include_str!("../../src/model.wgsl"),
                &MODEL_ENTRIES.replace("SECTIONS", &format!("{}u", streams.model_sections)),
            ),
            liquid: prepare(
                include_str!("../../src/liquid.wgsl"),
                &LIQUID_ENTRIES.replace("SECTIONS", &format!("{}u", streams.liquid_sections)),
            ),
        }
    }
}

/// Shared textures, materials and per-frame uniforms.
struct Fixture {
    gpu: Gpu,
    materials: wgpu::Buffer,
    zeros: wgpu::Buffer,
    clock: wgpu::Buffer,
    tints: wgpu::Buffer,
    query_tables: wgpu::Buffer,
    atmosphere: wgpu::Buffer,
    lightmap: wgpu::Buffer,
    atlas: wgpu::TextureView,
    sampler: wgpu::Sampler,
}

impl Fixture {
    /// Creates production mixed-floor resources, skipping an absent native GPU adapter.
    fn new(name: &str) -> Option<Self> {
        let gpu = Gpu::for_fixture(name)?;
        let storage = wgpu::BufferUsages::STORAGE;
        let uniform = wgpu::BufferUsages::UNIFORM;
        let frame = render::AtmosphereFrame::default();
        Some(Self {
            materials: gpu.words(&[0, 0, assets::NO_ANIMATION, 0, 0, 0], storage),
            zeros: gpu.words(&UNUSED, storage),
            clock: gpu.words(&[0; 4], uniform),
            tints: gpu.buffer(&[0.0; 8 + assets::SEASONAL_FOLIAGE_COUNT * 4], storage),
            query_tables: gpu.words(
                &meshing::biome_lattice::query_table_words(),
                wgpu::BufferUsages::UNIFORM,
            ),
            atmosphere: gpu.buffer(bytemuck::cast_slice(std::slice::from_ref(&frame)), uniform),
            lightmap: gpu.buffer(
                bytemuck::cast_slice(&render::LightmapInputs::default().build()),
                uniform,
            ),
            atlas: solid_terrain_raster::pattern_texture(&gpu),
            sampler: gpu.device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::Repeat,
                ..material_shader::native_leaf_sampler_descriptor()
            }),
            gpu,
        })
    }

    /// Every resource a world pass may bind, by binding number.
    fn resources<'a>(
        &'a self,
        streams: &'a Streams,
        uniform: &'a wgpu::Buffer,
        geometry: &'a wgpu::Buffer,
    ) -> [(u32, wgpu::BindingResource<'a>); 21] {
        [
            (0, uniform.as_entire_binding()),
            (1, streams.cube_quads.as_entire_binding()),
            (2, streams.origins.as_entire_binding()),
            (3, self.materials.as_entire_binding()),
            (4, wgpu::BindingResource::TextureView(&self.atlas)),
            (5, wgpu::BindingResource::TextureView(&self.atlas)),
            (6, wgpu::BindingResource::Sampler(&self.sampler)),
            (7, self.zeros.as_entire_binding()),
            (8, self.tints.as_entire_binding()),
            (
                material_shader::BIOME_QUERY_TABLES_BINDING,
                self.query_tables.as_entire_binding(),
            ),
            (9, self.zeros.as_entire_binding()),
            (10, self.zeros.as_entire_binding()),
            (11, self.clock.as_entire_binding()),
            (12, streams.model_templates.as_entire_binding()),
            (13, geometry.as_entire_binding()),
            (14, self.zeros.as_entire_binding()),
            (15, self.atmosphere.as_entire_binding()),
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
            (20, self.lightmap.as_entire_binding()),
        ]
    }

    /// Draws cube, slab and water passes together; pass_vertices selects model/liquid entry points.
    /// Controls can substitute the former world-coordinate projection.
    fn render(
        &self,
        streams: &Streams,
        shaders: &Shaders,
        floor: &Floor,
        view: &View,
        pass_vertices: [&str; 2],
    ) -> Vec<u8> {
        let uniform = self.gpu.buffer(
            &gpu_snapshot::view(view.clip_from_world, view.eye),
            wgpu::BufferUsages::UNIFORM,
        );
        let passes = [
            (
                &shaders.cube,
                "seam_cube_vertex",
                "fragment_solid",
                &streams.cube_geometry,
                floor.cubes.len(),
            ),
            (
                &shaders.model,
                pass_vertices[0],
                "fragment",
                &streams.model_geometry,
                floor.slabs.len(),
            ),
            (
                &shaders.liquid,
                pass_vertices[1],
                "fragment_depth",
                &streams.liquid_geometry,
                floor.water.len(),
            ),
        ];
        let bindings = passes
            .iter()
            .map(|&(source, vertex, fragment, geometry, _)| {
                let used = used_bindings(source, [vertex, fragment]);
                self.resources(streams, &uniform, geometry)
                    .into_iter()
                    .filter(|(binding, _)| used.contains(binding))
                    .map(|(binding, resource)| wgpu::BindGroupEntry { binding, resource })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let draws = passes
            .iter()
            .zip(&bindings)
            .map(|((_, _, fragment, _, quads), bindings)| Draw {
                fragment,
                vertices: 0..*quads as u32 * 6,
                bindings,
                blend: None,
                write_depth: true,
            })
            .collect::<Vec<_>>();
        let pipelines = passes
            .iter()
            .map(|(_, vertex, ..)| DrawPipeline {
                vertex,
                topology: wgpu::PrimitiveTopology::TriangleList,
            })
            .collect::<Vec<_>>();
        let sources = passes
            .iter()
            .map(|(source, ..)| source.as_str())
            .collect::<Vec<_>>();
        // Liquids wind their faces opposite to cubes; nothing here faces away.
        let state = RasterState {
            primitive: wgpu::PrimitiveState {
                cull_mode: None,
                ..Default::default()
            },
            ..Default::default()
        };
        self.gpu.render_sources(&sources, &draws, &pipelines, state)
    }
}

/// Uncovered floor pixels per jittered frame of steep views onto a mixed floor
/// centred on world column `centre`, rendered with `pass_vertices`.
fn floor_cracks(
    fixture: &Fixture,
    centre: [i32; 2],
    pass_vertices: [&str; 2],
) -> Vec<(u32, usize)> {
    let floor = Floor::new([centre[0] - 48, 0, centre[1]], 6, 8);
    let streams = Streams::new(&fixture.gpu, &floor);
    let shaders = Shaders::new(&streams);
    let eye = Vec3::new(centre[0] as f32, 3.0, centre[1] as f32 + 32.0);
    let sky = fixture.render(
        &streams,
        &shaders,
        &floor,
        &View::new(eye, 0.0, -1.5, 0.6),
        pass_vertices,
    );
    let background = sky[..4].to_vec();
    assert!(
        sky.chunks_exact(4).all(|pixel| pixel == background),
        "the sky view must miss every quad"
    );
    let mut cracked = Vec::new();
    for frame in 0..FRAMES {
        let (offset, yaw, pitch) = jitter(frame);
        let view = View::new(eye + offset * 0.25, yaw, 0.85 + pitch, 1.2);
        let pixels = fixture.render(&streams, &shaders, &floor, &view, pass_vertices);
        let count = uncovered_pixels(&pixels, &background, true);
        if count > 0 {
            gpu_snapshot::save(&format!("mixed floor {centre:?}-{frame}"), &pixels);
            cracked.push((frame, count));
        }
    }
    cracked
}

#[test]
fn far_from_origin_floors_of_cubes_slabs_and_water_leave_no_seam_pixels() {
    let Some(fixture) = Fixture::new("mixed world seams") else {
        return;
    };
    let centres = [[5_000, -5_000], [-12_000, 9_000], [20_000, -20_000]];
    let control = centres.map(|centre| {
        floor_cracks(
            &fixture,
            centre,
            ["absolute_model_vertex", "absolute_liquid_vertex"],
        )
        .len()
    });
    eprintln!(
        "mixed world seams: world-coordinate control frames with cracks per centre {control:?}"
    );
    if control.iter().sum::<usize>() == 0 {
        eprintln!(
            "skipping mixed world seams: missing seam-reproducing adapter fixture (world-coordinate slabs and water left no cracks)"
        );
        return;
    }
    for centre in centres {
        let cracked = floor_cracks(
            &fixture,
            centre,
            ["seam_model_vertex", "seam_liquid_vertex"],
        );
        assert!(
            cracked.is_empty(),
            "floor at {centre:?}: the clear colour shows between cubes, slabs and water (frame, pixels): {cracked:?}"
        );
    }
}

/// Production cube vertices with each quad's section from the geometry stream.
const CUBE_ENTRIES: &str = r#"
/// Draws sealed cube vertices with section indices from the fixture stream.
@vertex fn seam_cube_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var indices = array<u32, 6>(INDICES);
    return cube_vertex(geometry_streams[index / 6u] * 4u + indices[index % 6u], index / 6u);
}
"#;

/// Production model vertices, and a control that projects their world
/// position directly, as the pass did before it shared terrain's projection.
const MODEL_ENTRIES: &str = r#"
/// Maps fixture model instances to their section before running production geometry.
fn sectioned_model_vertex(index: u32) -> VertexOutput {
    var indices = array<u32, 6>(INDICES);
    let slab = index / 6u;
    return model_vertex(geometry_streams[SECTIONS + slab] * 4u + indices[index % 6u], slab);
}

/// Uses production camera-relative model projection in the shared-floor witness.
@vertex fn seam_model_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    return sectioned_model_vertex(index);
}

/// Projects the same model geometry through the former absolute-coordinate control.
@vertex fn absolute_model_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var out = sectioned_model_vertex(index);
    out.clip_position = view.clip_from_world * vec4(out.world_position, 1.0);
    return out;
}
"#;

/// Production liquid vertices, and the matching world-coordinate control.
const LIQUID_ENTRIES: &str = r#"
/// Maps fixture liquid instances to their section before running production geometry.
fn sectioned_liquid_vertex(index: u32) -> VertexOutput {
    var indices = array<u32, 6>(INDICES);
    let water = index / 6u;
    let section = geometry_streams[SECTIONS + water];
    return vertex_for_ref(TransparentDrawRef(water, section), section * 4u + indices[index % 6u]);
}

/// Uses production camera-relative liquid projection in the shared-floor witness.
@vertex fn seam_liquid_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    return sectioned_liquid_vertex(index);
}

/// Projects the same liquid geometry through the former absolute-coordinate control.
@vertex fn absolute_liquid_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var out = sectioned_liquid_vertex(index);
    out.clip_position = view.clip_from_world * vec4(out.world_position, 1.0);
    return out;
}
"#;
