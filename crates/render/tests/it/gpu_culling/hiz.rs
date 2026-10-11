//! The depth pyramid keeps every footprint's farthest depth, and Hi-Z culling stays conservative.

use super::*;

/// Boxes write their id; rectangles are screen-aligned NDC quads at a fixed depth.
pub(super) const SCENE_SHADER: &str = r#"
struct View { clip_from_world: mat4x4<f32> }
struct Item { low: vec4<f32>, high: vec4<f32> }
@group(0) @binding(0) var<uniform> view: View;
@group(0) @binding(1) var<storage, read> items: array<Item>;
struct Out { @builtin(position) position: vec4<f32>, @location(0) @interpolate(flat) id: u32 }
@vertex fn box_vertex(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> Out {
    var corners = array<u32, 36>(0u,2u,3u, 0u,3u,1u, 4u,5u,7u, 4u,7u,6u, 0u,1u,5u, 0u,5u,4u,
        2u,6u,7u, 2u,7u,3u, 0u,4u,6u, 0u,6u,2u, 1u,3u,7u, 1u,7u,5u);
    let corner = corners[index];
    let pick = vec3((corner & 1u) != 0u, (corner & 2u) != 0u, (corner & 4u) != 0u);
    let item = items[instance];
    var out: Out;
    out.position = view.clip_from_world * vec4(select(item.low.xyz, item.high.xyz, pick), 1.0);
    out.id = u32(item.low.w);
    return out;
}
@vertex fn rect_vertex(@builtin(vertex_index) index: u32, @builtin(instance_index) instance: u32) -> Out {
    var corners = array<vec2<u32>, 6>(vec2(0u, 0u), vec2(1u, 0u), vec2(1u, 1u), vec2(0u, 0u), vec2(1u, 1u), vec2(0u, 1u));
    let item = items[instance];
    let corner = corners[index];
    var out: Out;
    out.position = vec4(select(item.low.xy, item.high.xy, corner == vec2(1u)), item.low.z, 1.0);
    out.id = instance + 1u;
    return out;
}
@fragment fn id_fragment(in: Out) -> @location(0) vec4<u32> { return vec4(in.id, 0u, 0u, 1u); }
@fragment fn colour_fragment(in: Out) -> @location(0) vec4<f32> { return vec4(f32(in.id) / 8.0); }
"#;

pub(super) fn render_scene(
    gpu: &Gpu,
    target: &Target,
    entries: (&str, &str),
    view: &[f32],
    items: &[[f32; 8]],
) {
    let module = gpu
        .device
        .create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(SCENE_SHADER.into()),
        });
    let pipeline = gpu
        .device
        .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: None,
            layout: None,
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some(entries.0),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::GreaterEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: wgpu::MultisampleState {
                count: target.samples,
                ..Default::default()
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some(entries.1),
                compilation_options: Default::default(),
                targets: &[Some(target.color.format().into())],
            }),
            multiview_mask: None,
            cache: None,
        });
    let view = gpu.buffer(view, wgpu::BufferUsages::UNIFORM);
    let items_buffer = gpu.buffer(items.as_flattened(), wgpu::BufferUsages::STORAGE);
    let layout = pipeline.get_bind_group_layout(0);
    let mut entries_list = vec![wgpu::BindGroupEntry {
        binding: 1,
        resource: items_buffer.as_entire_binding(),
    }];
    if entries.0 == "box_vertex" {
        entries_list.push(wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        });
    }
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &entries_list,
    });
    let vertices = if entries.0 == "box_vertex" { 36 } else { 6 };
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    {
        let mut pass = target.pass(&mut encoder, true);
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..vertices, 0..items.len() as u32);
    }
    gpu.queue.submit([encoder.finish()]);
}

/// Every pyramid texel holds the farthest depth of the pixels it covers, single- or multi-sampled.
#[test]
fn hi_z_pyramid_keeps_the_farthest_depth_of_every_footprint() {
    let Some(gpu) = Gpu::for_fixture("terrain hi-z pyramid") else {
        return;
    };
    let kernels = CullKernels::new(&gpu.device);
    let camera = camera(Vec3::ZERO, Vec3::NEG_Z);
    let boxes = [
        [-6.0, -6.0, -30.0, 1.0, 6.0, 6.0, -29.0, 0.0],
        [-1.5, -0.25, -9.0, 2.0, 2.5, 3.0, -8.0, 0.0],
        [0.7, -3.0, -5.0, 3.0, 1.4, -1.0, -4.0, 0.0],
    ];
    let single = Target::new(&gpu, wgpu::TextureFormat::R32Uint, 1);
    render_scene(
        &gpu,
        &single,
        ("box_vertex", "id_fragment"),
        &camera.clip_from_world.to_cols_array(),
        &boxes,
    );
    let depth = floats(&read_texture(&gpu, &single.depth, 0));
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let pyramid = single.pyramid(&gpu, &kernels, &mut encoder);
    gpu.queue.submit([encoder.finish()]);
    assert_eq!(pyramid.depth_size, [SNAPSHOT_SIDE; 2]);
    let side = SNAPSHOT_SIDE as usize;
    for level in 0..pyramid.mip_count() {
        let texels = floats(&read_texture(&gpu, &pyramid.texture, level));
        let width = pyramid.size(level)[0] as usize;
        let mut expected = vec![1.0_f32; texels.len()];
        for (pixel, &value) in depth.iter().enumerate() {
            let shift = level + 1;
            let texel = ((pixel / side) >> shift) * width + ((pixel % side) >> shift);
            expected[texel] = expected[texel].min(value);
        }
        assert_eq!(texels, expected, "level {level}");
    }
    assert!(depth.iter().any(|&value| value > 0.0) && depth.contains(&0.0));

    // Pixel-aligned rectangles give every sample its pixel's depth, so both seeds agree.
    let ndc = |pixel: u32| pixel as f32 / SNAPSHOT_SIDE as f32 * 2.0 - 1.0;
    let rects = [
        [ndc(10), ndc(20), 0.25, 0.0, ndc(200), ndc(140), 0.0, 0.0],
        [ndc(64), ndc(33), 0.75, 0.0, ndc(129), ndc(255), 0.0, 0.0],
        [ndc(3), ndc(150), 0.5, 0.0, ndc(77), ndc(151), 0.0, 0.0],
    ];
    let pyramid_of = |target: &Target| {
        render_scene(
            &gpu,
            target,
            ("rect_vertex", "colour_fragment"),
            &[0.0; 16],
            &rects,
        );
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        let pyramid = target.pyramid(&gpu, &kernels, &mut encoder);
        gpu.queue.submit([encoder.finish()]);
        (0..pyramid.mip_count())
            .map(|level| floats(&read_texture(&gpu, &pyramid.texture, level)))
            .collect::<Vec<_>>()
    };
    let single = pyramid_of(&Target::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, 1));
    let multi = pyramid_of(&Target::new(&gpu, wgpu::TextureFormat::Rgba8Unorm, 4));
    assert_eq!(single, multi);
    assert!(single[0].contains(&0.75));
}

/// Display-sized pyramids build, and odd trailing rows and columns still reach every level.
/// Only texels covering a depth pixel are built; edges nearer than an unbuilt texel prove no
/// covered texel reads one.
#[test]
fn hi_z_pyramid_covers_every_pixel_of_display_sized_targets() {
    let Some(gpu) = Gpu::for_fixture("terrain hi-z display sizes") else {
        return;
    };
    let kernels = CullKernels::new(&gpu.device);
    for size in [[1920, 1080], [2560, 1440], [3024, 1834], [1366, 767]] {
        let sizes = kernels::pyramid_sizes(size);
        assert!(sizes[0][0] * 2 >= size[0] && sizes[0][1] * 2 >= size[1]);
        assert!(
            sizes
                .windows(2)
                .all(|pair| pair[1] == pair[0].map(|side| (side / 2).max(1)))
        );
        assert_eq!(*sizes.last().unwrap(), [1, 1]);
        let extents = kernels::pyramid_extents(size);

        let [width, height] = size;
        let edge_x = 1.0 - 2.0 / width as f32;
        let edge_y = -1.0 + 2.0 / height as f32;
        for covered_edges in [false, true] {
            // Without the cover, only the last column and row keep the cleared, farthest depth.
            let cover = [-1.0, -1.0, 0.25, 0.0, 1.0, 1.0, 0.0, 0.0];
            let rects = [
                [-1.0, edge_y, 0.5, 0.0, edge_x, 1.0, 0.0, 0.0],
                [-0.5, 0.0, 0.75, 0.0, 0.25, 0.5, 0.0, 0.0],
            ];
            let scene = if covered_edges {
                [&[cover][..], &rects].concat()
            } else {
                rects.to_vec()
            };
            let target = Target::sized(&gpu, wgpu::TextureFormat::Rgba8Unorm, 1, size);
            render_scene(
                &gpu,
                &target,
                ("rect_vertex", "colour_fragment"),
                &[0.0; 16],
                &scene,
            );
            let depth = floats(&read_texture(&gpu, &target.depth, 0));
            let (w, h) = (width as usize, height as usize);
            let edge = if covered_edges { 0.25 } else { 0.0 };
            assert_eq!(depth[0], 0.5, "{size:?} interior");
            assert_eq!(depth[w - 1], edge, "{size:?} last column");
            assert_eq!(depth[(h - 1) * w], edge, "{size:?} last row");
            let mut encoder = gpu.device.create_command_encoder(&Default::default());
            let pyramid = target.pyramid(&gpu, &kernels, &mut encoder);
            gpu.queue.submit([encoder.finish()]);
            assert_eq!(pyramid.mip_count() as usize, sizes.len());
            for level in 0..pyramid.mip_count() {
                let shift = level + 1;
                assert_eq!(
                    extents[level as usize],
                    [((w - 1) >> shift) as u32 + 1, ((h - 1) >> shift) as u32 + 1],
                    "{size:?} level {level} covered extent"
                );
                let texels = floats(&read_texture(&gpu, &pyramid.texture, level));
                let level_width = pyramid.size(level)[0] as usize;
                let mut expected = vec![None::<f32>; texels.len()];
                for (pixel, &value) in depth.iter().enumerate() {
                    let texel = ((pixel / w) >> shift) * level_width + ((pixel % w) >> shift);
                    expected[texel] =
                        Some(expected[texel].map_or(value, |old: f32| old.min(value)));
                }
                for (texel, (actual, expected)) in texels.iter().zip(&expected).enumerate() {
                    if let Some(expected) = expected {
                        assert_eq!(
                            actual, expected,
                            "{size:?} edges {covered_edges} level {level} texel {texel}"
                        );
                    }
                }
            }
        }
    }
}

/// Behind a wall, only sub-chunks whose box shows a pixel survive; none that shows one is lost.
#[test]
fn hi_z_never_culls_a_visible_sub_chunk_and_culls_hidden_ones() {
    let Some(gpu) = Gpu::for_fixture("terrain hi-z conservative") else {
        return;
    };
    let camera = camera(Vec3::new(8.25, 72.5, 8.75), Vec3::new(10.0, 70.0, -40.0));
    let mut random = Lcg(11);
    let mut records = Vec::new();
    let mut boxes = vec![[-40.0, 60.0, -24.0, 0.0, 56.0, 84.0, -23.0, 0.0]];
    for x in -3..=3 {
        for y in 3..=5 {
            for z in -5..=-2 {
                let slot = records.len() as u32;
                let low = [1 + random.next(8), random.next(10), 1 + random.next(8)];
                let high = low.map(|value| value as i32 + 2 + random.next(5) as i32);
                let origin = [x * 16, y * 16, z * 16];
                records.push(
                    CullRecord::new(&CullRecordSource {
                        origin,
                        base_vertex: slot as i32 * 4,
                        bounds: [low.map(|value| value as i32), high],
                        cube: slot * 8..slot * 8 + 6,
                        solid_ends: [1, 2, 3, 4, 5, 6],
                        ..Default::default()
                    })
                    .unwrap(),
                );
                let world = |local: [i32; 3]| {
                    std::array::from_fn::<f32, 3, _>(|axis| (origin[axis] + local[axis]) as f32)
                };
                let (lo, hi) = (world(low.map(|value| value as i32)), world(high));
                boxes.push([
                    lo[0],
                    lo[1],
                    lo[2],
                    (slot + 1) as f32,
                    hi[0],
                    hi[1],
                    hi[2],
                    0.0,
                ]);
            }
        }
    }
    let target = Target::new(&gpu, wgpu::TextureFormat::R32Uint, 1);
    render_scene(
        &gpu,
        &target,
        ("box_vertex", "id_fragment"),
        &camera.clip_from_world.to_cols_array(),
        &boxes,
    );
    let ids = read_texture(&gpu, &target.color, 0);
    let shown = bytemuck::cast_slice::<u8, u32>(&ids)
        .iter()
        .filter(|&&id| id != 0)
        .map(|&id| id as usize - 1)
        .collect::<BTreeSet<_>>();

    let enabled = enabled_words(|_| true, records.len());
    let culler = Culler::new(&gpu, &records, &enabled);
    culler.set_history(&vec![0; records.len()]);
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let pyramid = target.pyramid(&gpu, &culler.kernels, &mut encoder);
    let input = view_input(&camera, pyramid.mip_count());
    culler.encode(&mut encoder, &input, CullPhase::Late, Some(&pyramid));
    gpu.queue.submit([encoder.finish()]);
    let kept = culler.history();
    let in_frustum = (0..records.len())
        .filter(|&slot| bevy_visible(&camera.frustum, records[slot].origin))
        .collect::<BTreeSet<_>>();
    assert!(!shown.is_empty());
    assert!(
        shown.is_subset(&kept),
        "Hi-Z culled a sub-chunk that shows pixels"
    );
    assert!(
        kept.difference(&in_frustum)
            .all(|&slot| on_frustum_boundary(&camera.frustum, records[slot].origin))
    );
    assert!(
        kept.len() < in_frustum.len(),
        "the wall must hide some sub-chunks"
    );
}
