//! Ordinary indexed unit draws must keep the exact sealed direct-path pixels and depth ties.
use super::*;
use render_model::terrain_units::{QUADS_PER_UNIT, TerrainUnit, VisibleTerrainUnit, shards};

/// Adds T junctions, alpha-tested two-sided faces, a long stream and coincident depth ties.
fn fixture() -> Terrain {
    let mut terrain = terrain();
    terrain.alpha_cutout = true;
    let mut cutout = (0..131)
        .map(|quad| {
            PackedQuad::new(
                [2 + quad % 3, 2, 7],
                Face::PositiveZ,
                8,
                8,
                if quad % 2 == 0 { 2 } else { 0 },
            )
        })
        .collect::<Vec<_>>();
    cutout.extend([
        PackedQuad::new([0, 0, 1], Face::PositiveZ, 8, 16, 1),
        PackedQuad::new([8, 0, 1], Face::PositiveZ, 8, 8, 0),
        PackedQuad::new([8, 8, 1], Face::PositiveZ, 8, 8, 1),
    ]);
    terrain.add([0, 4, -1], Vec::new(), cutout);
    terrain.add(
        [0, 4, -1],
        Vec::new(),
        vec![PackedQuad::new([2, 2, 7], Face::PositiveZ, 8, 8, 2)],
    );
    terrain
}

/// Converts validated direct commands to the shader-facing descriptors, preserving every quad.
fn visible_units(draws: &[[u32; 5]]) -> Vec<VisibleTerrainUnit> {
    let units = draws
        .iter()
        .flat_map(|&[_, count, _, base, first]| {
            TerrainUnit::build(first..first + count, base / 4, [[0; 3], [16; 3]], None, 1)
        })
        .collect::<Vec<_>>();
    let mut visible = Vec::new();
    TerrainUnit::compact(&units, |_| true, &mut visible);
    visible
}

/// Draws consecutive pages with zero base vertex and first instance, as production does.
fn draw_units(
    gpu: &Gpu,
    raster: &Raster,
    target: &Target,
    stream: CullStream,
    units: &[VisibleTerrainUnit],
    clear: bool,
    page_units: u64,
) {
    let (pipeline, group) = match stream {
        CullStream::Solid => (&raster.solid, &raster.solid_group),
        _ => (&raster.cutout, &raster.cutout_group),
    };
    let indices = (0..QUADS_PER_UNIT)
        .flat_map(|quad| chunk_constants::STATIC_QUAD_INDICES.map(|index| quad * 4 + index))
        .collect::<Vec<_>>();
    let index_buffer = gpu.words(&indices, wgpu::BufferUsages::INDEX);
    let empty = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[],
    });
    let pages = shards(
        units.len(),
        page_units * size_of::<VisibleTerrainUnit>() as u64,
    )
    .map(|range| {
        let buffer = gpu.words(
            bytemuck::cast_slice(&units[range.clone()]),
            wgpu::BufferUsages::STORAGE,
        );
        (
            gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &pipeline.get_bind_group_layout(2),
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            }),
            range.len() as u32,
        )
    })
    .collect::<Vec<_>>();
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = target.pass(&mut encoder, clear);
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.set_bind_group(1, &empty, &[]);
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for (page, count) in &pages {
            pass.set_bind_group(2, page, &[]);
            pass.draw_indexed(0..indices.len() as u32, 0, 0..*count);
        }
    }
    gpu.queue.submit([encoder.finish()]);
}

#[test]
fn batched_direct_units_preserve_seams_cutout_tails_and_depth_precedence() {
    let Some(gpu) = Gpu::for_fixture_with_limits(
        "direct terrain unit pixel identity",
        wgpu::Features::empty(),
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: render::required_vertex_storage_buffers(),
            ..Default::default()
        },
    ) else {
        return;
    };
    let terrain = fixture();
    for (index, camera) in [
        camera(Vec3::new(8.25, 72.5, 8.75), Vec3::new(10.0, 70.0, -40.0)),
        camera(Vec3::new(-14.5, 78.0, 4.0), Vec3::new(6.0, 72.0, -16.0)),
        camera(Vec3::new(12.0, 75.0, -40.0), Vec3::new(8.0, 72.0, 0.0)),
    ]
    .iter()
    .enumerate()
    {
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let legacy = Raster::through(&gpu, &terrain, camera, true, format, "vertex");
        let batched = Raster::through(&gpu, &terrain, camera, true, format, "vertex_unit");
        // Reverse order deliberately makes coplanar precedence depend on allocation order.
        let order = (0..terrain.chunks.len()).rev().collect::<Vec<_>>();
        let eye = camera.eye.as_dvec3().to_array();
        let draws = [CullStream::Solid, CullStream::Cutout].map(|stream| {
            order
                .iter()
                .flat_map(|&slot| slot_draws(&terrain, slot, eye, stream))
                .collect::<Vec<_>>()
        });
        let expected_target = Target::new(&gpu, format, 1);
        for (stream, draws) in [CullStream::Solid, CullStream::Cutout]
            .into_iter()
            .zip(&draws)
        {
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            {
                let mut pass = expected_target.pass(&mut encoder, stream == CullStream::Solid);
                legacy.draw(&mut pass, stream, draws);
            }
            gpu.queue.submit([encoder.finish()]);
        }
        let expected = read_texture(&gpu, &expected_target.color, 0);
        assert!(
            expected
                .as_chunks::<4>()
                .0
                .iter()
                .any(|pixel| pixel != &expected[..4])
        );
        for page_units in [2, u64::from(u32::MAX)] {
            let actual_target = Target::new(&gpu, format, 1);
            for (stream, draws) in [CullStream::Solid, CullStream::Cutout]
                .into_iter()
                .zip(&draws)
            {
                draw_units(
                    &gpu,
                    &batched,
                    &actual_target,
                    stream,
                    &visible_units(draws),
                    stream == CullStream::Solid,
                    page_units,
                );
            }
            let actual = read_texture(&gpu, &actual_target.color, 0);
            let differences = expected
                .as_chunks::<4>()
                .0
                .iter()
                .zip(actual.as_chunks::<4>().0)
                .filter(|(a, b)| a != b)
                .count();
            gpu_snapshot::save(&format!("terrain_units_before_{index}"), &expected);
            gpu_snapshot::save(&format!("terrain_units_after_{index}"), &actual);
            assert_eq!(
                differences, 0,
                "{:?}, camera {index}, page capacity {page_units}",
                gpu.backend
            );
        }
    }
}
