//! Ordinary indexed unit draws must keep the exact sealed direct-path pixels and depth ties.
use super::*;
use render_model::terrain_units::{
    IndexedUnitPage, QUADS_PER_UNIT, TerrainUnit, UNITS_PER_INSTANCE, VisibleTerrainUnit, shards,
};

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
#[allow(
    clippy::too_many_arguments,
    reason = "native draw fixture varies page capacity and the optional GPU mask independently"
)]
fn draw_units(
    gpu: &Gpu,
    raster: &Raster,
    target: &Target,
    stream: CullStream,
    units: &[VisibleTerrainUnit],
    clear: bool,
    page_units: u64,
    mask: Option<&wgpu::Buffer>,
    depth_only: bool,
) {
    let (pipeline, group) = match stream {
        CullStream::Solid => (&raster.solid, &raster.solid_group),
        _ => (&raster.cutout, &raster.cutout_group),
    };
    let (pipeline, group) = if depth_only {
        (
            raster.depth_only.as_ref().expect("unit depth pipeline"),
            raster.depth_group.as_ref().expect("unit depth bindings"),
        )
    } else {
        (pipeline, group)
    };
    let indices = (0..QUADS_PER_UNIT * UNITS_PER_INSTANCE)
        .flat_map(|quad| chunk_constants::STATIC_QUAD_INDICES.map(|index| quad * 4 + index))
        .collect::<Vec<_>>();
    let index_buffer = gpu.words(&indices, wgpu::BufferUsages::INDEX);
    let empty = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(1),
        entries: &[],
    });
    let mut mask_encoder = gpu.device.create_command_encoder(&Default::default());
    let masks = super::unit_mask::UnitMasks::new(&gpu.device);
    let pages = shards(
        units.len(),
        page_units * size_of::<VisibleTerrainUnit>() as u64,
    )
    .map(|range| {
        // Reverse physical slots, then recover the original draw order through references.
        let layout = IndexedUnitPage {
            capacity: range.len(),
        };
        let mut words = vec![0; layout.byte_size() as usize / size_of::<u32>()];
        words[..4].copy_from_slice(&layout.header(range.len() as u32));
        let descriptors = units[range.clone()]
            .iter()
            .rev()
            .copied()
            .collect::<Vec<_>>();
        let descriptor_words: &[u32] = bytemuck::cast_slice(&descriptors);
        words[4..4 + descriptor_words.len()].copy_from_slice(descriptor_words);
        let start = layout.reference_offset() as usize / size_of::<u32>();
        for (index, slot) in (0..range.len() as u32).rev().enumerate() {
            words[start + index] = slot;
        }
        let buffer = gpu.words(&words, wgpu::BufferUsages::STORAGE);
        if mask.is_some() {
            let mut bindings = None;
            masks.prepare(&gpu.device, &buffer, mask, &mut bindings);
            masks.encode(
                &gpu.device,
                &mut mask_encoder,
                bindings.as_ref().unwrap(),
                true,
                range.len() as u32,
            );
        }
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
    gpu.queue.submit([mask_encoder.finish()]);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = if depth_only {
            drop(target.pass(&mut encoder, clear));
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("unit depth prepass"),
                color_attachments: &[],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &target.depth.create_view(&Default::default()),
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
        } else {
            target.pass(&mut encoder, clear)
        };
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, group, &[]);
        pass.set_bind_group(1, &empty, &[]);
        pass.set_index_buffer(index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        for (page, count) in &pages {
            pass.set_bind_group(2, page, &[]);
            pass.draw_indexed(
                0..indices.len() as u32,
                0,
                0..count.div_ceil(UNITS_PER_INSTANCE),
            );
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
        for page_units in [1, 2, 3, u64::from(u32::MAX)] {
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
                    None,
                    false,
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

/// Current-frame bits, rather than stale visibility, drive the vertex rejection path.
#[test]
fn unit_visibility_mask_rejects_geometry_before_rasterization() {
    let Some(gpu) = Gpu::for_fixture_with_limits(
        "ordinary unit visibility mask",
        wgpu::Features::empty(),
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: render::required_vertex_storage_buffers(),
            ..Default::default()
        },
    ) else {
        return;
    };
    let terrain = fixture();
    let camera = camera(Vec3::new(8.25, 72.5, 8.75), Vec3::new(10.0, 70.0, -40.0));
    let format = wgpu::TextureFormat::Rgba8Unorm;
    let raster = Raster::through(&gpu, &terrain, &camera, true, format, "vertex_unit");
    let draws = (0..terrain.chunks.len())
        .flat_map(|slot| {
            slot_draws(
                &terrain,
                slot,
                camera.eye.as_dvec3().to_array(),
                CullStream::Solid,
            )
        })
        .collect::<Vec<_>>();
    let units = visible_units(&draws);
    let bits = gpu.words(
        &vec![u32::MAX; terrain.chunks.len().div_ceil(32)],
        wgpu::BufferUsages::STORAGE,
    );
    let target = Target::new(&gpu, format, 1);
    draw_units(
        &gpu,
        &raster,
        &target,
        CullStream::Solid,
        &units,
        true,
        3,
        Some(&bits),
        false,
    );
    let pixels = read_texture(&gpu, &target.color, 0);
    assert!(
        pixels.as_chunks::<4>().0.iter().all(|p| p == &pixels[..4]),
        "masked units reached rasterization"
    );
}

/// Rebuilds depth and visibility on each camera move, retaining strict depth ties and page tails.
#[test]
fn same_frame_unit_visibility_preserves_pixels_after_disocclusion() {
    let Some(gpu) = Gpu::for_fixture_with_limits(
        "same-frame ordinary terrain visibility",
        wgpu::Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
        wgpu::Limits {
            max_storage_buffers_per_shader_stage: render::required_vertex_storage_buffers(),
            ..Default::default()
        },
    ) else {
        return;
    };
    let terrain = fixture();
    let kernels = CullKernels::new(&gpu.device);
    let storage = storage::OcclusionStorage::new(&gpu.device, terrain.records.len() as u32);
    gpu.queue
        .write_buffer(&storage.records, 0, bytemuck::cast_slice(&terrain.records));
    for samples in [1, 2]
        .into_iter()
        .filter(|&samples| samples == 1 || gpu.backend == wgpu::Backend::Metal)
    {
        for eye in [
            Vec3::new(8.25, 72.5, 8.75),
            Vec3::new(-14.5, 78.0, 4.0),
            Vec3::new(12.0, 75.0, -40.0),
        ] {
            let camera = camera(eye, Vec3::new(8.0, 70.0, -16.0));
            let format = wgpu::TextureFormat::Rgba8Unorm;
            let legacy =
                Raster::through_samples(&gpu, &terrain, &camera, true, format, "vertex", samples);
            let batched = Raster::through_samples(
                &gpu,
                &terrain,
                &camera,
                true,
                format,
                "vertex_unit",
                samples,
            );
            let order = (0..terrain.chunks.len()).rev().collect::<Vec<_>>();
            let draws = [CullStream::Solid, CullStream::Cutout].map(|stream| {
                order
                    .iter()
                    .flat_map(|&slot| slot_draws(&terrain, slot, eye.as_dvec3().to_array(), stream))
                    .collect::<Vec<_>>()
            });
            let expected = Target::new(&gpu, format, samples);
            for (stream, draws) in [CullStream::Solid, CullStream::Cutout]
                .into_iter()
                .zip(&draws)
            {
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                {
                    let mut pass = expected.pass(&mut encoder, stream == CullStream::Solid);
                    legacy.draw(&mut pass, stream, draws);
                }
                gpu.queue.submit([encoder.finish()]);
            }
            let pixels = read_texture(
                &gpu,
                expected.resolved.as_ref().unwrap_or(&expected.color),
                0,
            );
            for page_capacity in [1, 2, 3, u64::from(u32::MAX)] {
                let target = Target::new(&gpu, format, samples);
                // A solid prefix is a conservative depth source. Its colors are overwritten by the ordered final pass.
                let occluders = visible_units(&draws[0]);
                draw_units(
                    &gpu,
                    &batched,
                    &target,
                    CullStream::Solid,
                    &occluders[..occluders.len().min(7)],
                    true,
                    page_capacity,
                    None,
                    true,
                );
                let pyramid = HizPyramid::new(&gpu.device, [SNAPSHOT_SIDE; 2]);
                let bindings = kernels.pyramid_bindings(
                    &gpu.device,
                    &target.depth.create_view(&Default::default()),
                    samples > 1,
                    &pyramid,
                );
                let input = direct::sized_input(&camera, pyramid.mip_count(), [SNAPSHOT_SIDE; 2]);
                let uniform = CullViewUniform::new(
                    &input,
                    CullPhase::Late,
                    terrain.records.len() as u32,
                    storage.capacity,
                    kernels.workgroup,
                );
                gpu.queue
                    .write_buffer(&storage.uniform, 0, bytemuck::bytes_of(&uniform));
                let group = kernels.occlusion_bind_group(&gpu.device, &storage, &pyramid);
                let mut encoder = gpu.device.create_command_encoder(&Default::default());
                kernels.encode_pyramid(&mut encoder, &pyramid, &bindings, None);
                kernels.encode_occlusion(&mut encoder, &group, terrain.records.len() as u32);
                gpu.queue.submit([encoder.finish()]);
                for (stream, draws) in [CullStream::Solid, CullStream::Cutout]
                    .into_iter()
                    .zip(&draws)
                {
                    let mut units = visible_units(draws);
                    // Seed stale hidden verdicts, including newly disoccluded units.
                    for unit in &mut units {
                        unit.hidden = 1;
                    }
                    draw_units(
                        &gpu,
                        &batched,
                        &target,
                        stream,
                        &units,
                        false,
                        page_capacity,
                        Some(&storage.occluded),
                        false,
                    );
                }
                let actual =
                    read_texture(&gpu, target.resolved.as_ref().unwrap_or(&target.color), 0);
                assert_eq!(
                    pixels, actual,
                    "camera {eye:?}, samples {samples}, page capacity {page_capacity}"
                );
            }
        }
    }
}
