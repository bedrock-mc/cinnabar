//! Culled solid cube runs must rasterise exactly the pixels of the two-sided discard path.
use crate::chunk_constants;
use crate::gpu_snapshot;
use crate::material_shader;
use crate::shader_source;

use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu, RasterState};
use meshing::{CubeQuadLayout, Face, PackedQuad, sub_chunk_facing_faces};

const ORIGIN: [i32; 3] = [16, 64, -16];
const TEXTURE_SIDE: u32 = 16;

/// Isolated blocks expose all six faces; the sheets add greedy extents.
fn fixture_quads() -> (Vec<PackedQuad>, CubeQuadLayout) {
    let blocks = [
        [0, 0, 0],
        [2, 2, 2],
        [5, 3, 9],
        [12, 1, 4],
        [9, 10, 13],
        [3, 12, 6],
        [14, 14, 14],
        [15, 7, 1],
        [11, 5, 8],
    ];
    let mut quads = blocks
        .iter()
        .enumerate()
        .flat_map(|(index, &origin)| {
            Face::ALL.map(|face| PackedQuad::new(origin, face, 1, 1, (index % 2) as u32))
        })
        .collect::<Vec<_>>();
    quads.push(PackedQuad::new([6, 4, 0], Face::NegativeZ, 5, 3, 1));
    quads.push(PackedQuad::new([6, 4, 0], Face::PositiveZ, 5, 3, 0));
    quads.push(PackedQuad::new([10, 8, 2], Face::PositiveX, 4, 2, 1));
    quads.push(PackedQuad::new([1, 6, 3], Face::PositiveY, 7, 4, 0));
    quads.push(PackedQuad::new([1, 9, 3], Face::NegativeY, 7, 4, 1));
    let slot = |quad: &PackedQuad| {
        CubeQuadLayout::SOLID_FACE_ORDER
            .iter()
            .position(|&face| face == quad.face())
    };
    quads.sort_by_key(slot);
    let mut counts = [0; 6];
    for quad in &quads {
        counts[quad.face() as usize] += 1;
    }
    (quads, CubeQuadLayout::from_solid_counts(counts))
}

/// Distinct per-corner light, AO and face-shade bits, so interpolation shows any mismatch.
pub(crate) fn lighting_words(count: usize) -> Vec<u32> {
    (0..count)
        .flat_map(|quad| {
            let sample = |corner: usize| {
                let levels = (quad * 37 + corner * 11) & 0xff;
                let ao = (quad + corner) % 4;
                (levels | (ao << 8) | ((quad % 2) << 11)) as u32
            };
            [sample(0) | (sample(1) << 16), sample(2) | (sample(3) << 16)]
        })
        .collect()
}

pub(crate) fn pattern_texture(gpu: &Gpu) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("solid raster pattern"),
        size: wgpu::Extent3d {
            width: TEXTURE_SIDE,
            height: TEXTURE_SIDE,
            depth_or_array_layers: 2,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let texels = (0..2 * TEXTURE_SIDE * TEXTURE_SIDE)
        .flat_map(|index| {
            let (layer, texel) = (index / 256, index % 256);
            let (u, v) = (texel % TEXTURE_SIDE, texel / TEXTURE_SIDE);
            [
                (u * 16) as u8,
                (v * 16) as u8,
                (40 + layer * 150) as u8,
                255,
            ]
        })
        .collect::<Vec<_>>();
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(TEXTURE_SIDE * 4),
            rows_per_image: Some(TEXTURE_SIDE),
        },
        texture.size(),
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

#[test]
fn culled_solid_runs_match_the_two_sided_discard_path_from_every_side() {
    let Some(gpu) = Gpu::for_fixture("solid terrain raster") else {
        return;
    };
    let (quads, layout) = fixture_quads();
    let quad_words = quads.iter().flat_map(PackedQuad::words).collect::<Vec<_>>();
    let storage = wgpu::BufferUsages::STORAGE;
    let quad_buffer = gpu.words(&quad_words, storage);
    let origin = gpu.words(
        &[
            ORIGIN[0] as u32,
            ORIGIN[1] as u32,
            ORIGIN[2] as u32,
            0,
            0,
            0,
            0,
            0,
        ],
        storage,
    );
    let no_animation = u32::MAX;
    let materials = gpu.words(
        &[0, 0, no_animation, 0, 0, 0, 1, 0, no_animation, 0, 0, 0],
        storage,
    );
    let animations = gpu.words(&[0; 8], storage);
    let animation_frames = gpu.words(&[0; 4], storage);
    let clock = gpu.words(&[0; 4], wgpu::BufferUsages::UNIFORM);
    let streams = gpu.words(&lighting_words(quads.len()), storage);
    let records = gpu.buffer(&[0.0], storage);
    let tints = gpu.buffer(&[0.0; 8 + assets::SEASONAL_FOLIAGE_COUNT * 4], storage);
    let mut atmosphere = [0.0; 32];
    atmosphere[16..19].copy_from_slice(&[0.6, 0.7, 0.9]);
    atmosphere[19] = 8.0;
    atmosphere[20] = 60.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let table = render::LightmapInputs::default().build();
    let lightmap = gpu.buffer(bytemuck::cast_slice(&table), wgpu::BufferUsages::UNIFORM);
    let atlas = pattern_texture(&gpu);
    let sampler = gpu.device.create_sampler(&Default::default());

    let indices = chunk_constants::STATIC_QUAD_INDICES;
    let source = format!(
        "{}\n{}",
        shader_source::standalone(include_str!("../../src/chunk.wgsl"), &[]),
        RASTER_VERTEX.replace(
            "RASTER_INDICES",
            &indices.map(|index| format!("{index}u")).join(", ")
        ),
    )
    .replace("@group(1) @binding(0)", "@group(0) @binding(20)");

    let origin_point = Vec3::from_array(ORIGIN.map(|value| value as f32));
    let cameras = [
        (Vec3::new(24.0, 22.0, 26.0), Vec3::splat(8.0)),
        (Vec3::new(-9.0, -7.0, -10.0), Vec3::splat(8.0)),
        (Vec3::new(8.5, 6.5, 8.5), Vec3::new(2.0, 2.0, 2.0)),
        (Vec3::new(8.5, 6.5, 8.5), Vec3::new(14.0, 12.0, 15.0)),
        (Vec3::new(8.0, 20.0, -6.0), Vec3::new(8.0, 4.0, 8.0)),
        (Vec3::new(16.0, 9.0, 9.0), Vec3::new(0.0, 7.0, 7.0)),
    ];
    let mut culled_any = false;
    for (eye, target) in cameras {
        let (eye, target) = (origin_point + eye, origin_point + target);
        let clip_from_world = Mat4::perspective_infinite_reverse_rh(1.4, 1.0, 0.05)
            * Mat4::look_at_rh(eye, target, Vec3::Y);
        let view = gpu.buffer(
            &gpu_snapshot::view(clip_from_world, eye),
            wgpu::BufferUsages::UNIFORM,
        );
        let bindings = [
            (0, view.as_entire_binding()),
            (1, quad_buffer.as_entire_binding()),
            (2, origin.as_entire_binding()),
            (3, materials.as_entire_binding()),
            (4, wgpu::BindingResource::TextureView(&atlas)),
            (5, wgpu::BindingResource::TextureView(&atlas)),
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
        let draw = |fragment, quads: std::ops::Range<u32>| Draw {
            fragment,
            vertices: quads.start * 6..quads.end * 6,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        };
        let raster = |cull_mode| RasterState {
            primitive: wgpu::PrimitiveState {
                cull_mode,
                ..Default::default()
            },
            ..Default::default()
        };
        let reference = gpu.render_with_state(
            &source,
            "raster_vertex",
            &[draw("fragment", 0..quads.len() as u32)],
            raster(None),
        );
        let facing = sub_chunk_facing_faces(ORIGIN, eye.as_dvec3().to_array());
        let runs = layout.solid_runs(facing).collect::<Vec<_>>();
        let solid_draws = runs
            .iter()
            .map(|run| draw("fragment_solid", run.clone()))
            .collect::<Vec<_>>();
        let culled = gpu.render_with_state(
            &source,
            "raster_vertex",
            &solid_draws,
            raster(Some(wgpu::Face::Back)),
        );
        let background = &reference[..4];
        assert!(
            reference.chunks_exact(4).any(|pixel| pixel != background),
            "camera {eye} sees terrain"
        );
        let mismatched = reference
            .chunks_exact(4)
            .zip(culled.chunks_exact(4))
            .filter(|(left, right)| left != right)
            .count();
        assert_eq!(mismatched, 0, "camera {eye} with runs {runs:?}");
        culled_any |= runs.iter().map(|run| run.len()).sum::<usize>() < quads.len();
    }
    assert!(culled_any, "some camera must skip back-facing runs");
}

const RASTER_VERTEX: &str = r#"
@vertex fn raster_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    var indices = array<u32, 6>(RASTER_INDICES);
    return cube_vertex(indices[index % 6u], index / 6u);
}
"#;
