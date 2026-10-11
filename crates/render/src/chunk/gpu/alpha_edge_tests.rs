//! Multisampled coverage must not fetch texels beyond an alpha-tested quad.

use crate::gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE, save};
use wgpu::util::DeviceExt;

const QUAD: &str = r#"
fn edge_corner(index: u32) -> vec2<f32> {
    return array(vec2(0.0, 0.0), vec2(0.0, 1.0), vec2(1.0, 1.0),
        vec2(0.0, 0.0), vec2(1.0, 1.0), vec2(1.0, 0.0))[index];
}
fn edge_position(uv: vec2<f32>) -> vec4<f32> {
    let pixel = vec2(30.2 + uv.x * 190.0, 60.2 - uv.x * 36.0 + uv.y * 150.0);
    return vec4(pixel / EDGE_TARGET_SIDE * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.5, 1.0);
}
"#;

/// Makes transparent sprite borders and opaque texels immediately outside the top edge.
fn raster(gpu: &Gpu, held: bool) -> wgpu::TextureView {
    let side = if held { 64 } else { 16 };
    let mut pixels = vec![0; side * side * 4];
    for y in 0..side {
        for x in 0..side {
            let neighbour = if held {
                y == 15 && (16..32).contains(&x)
            } else {
                y == 15
            };
            let interior = if held {
                (20..28).contains(&x) && (20..28).contains(&y)
            } else {
                (4..12).contains(&x) && (4..12).contains(&y)
            };
            if neighbour || interior {
                pixels[(y * side + x) * 4..(y * side + x + 1) * 4]
                    .copy_from_slice(&[0, 255, 0, 255]);
            }
        }
    }
    gpu.device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("transparent border and opaque adjacent texels"),
                size: wgpu::Extent3d {
                    width: side as u32,
                    height: side as u32,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            &pixels,
        )
        .create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        })
}

/// Binds a retained fixture buffer without copying its contents.
fn buffer_entry(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
    wgpu::BindGroupEntry {
        binding,
        resource: buffer.as_entire_binding(),
    }
}

/// Exercises production fragments and their interpolation interface on a diagonal quad.
fn render_edge(gpu: &Gpu, held: bool, samples: u32, back: bool) -> Vec<u8> {
    let source = if held {
        let shader = crate::shader_safety::from_actor_wesl(
            include_str!("../../hand_rig.wesl"),
            "hand",
            crate::actor::ACTOR_GPU_INSTANCE_WORDS,
            render_model::ACTOR_RIG_VERTEX_WORDS,
        );
        let bevy::shader::Source::Wesl(source) = shader.source else {
            unreachable!()
        };
        format!(
            "{}\n{QUAD}\n{}",
            crate::shader_source::standalone(&source, &[]),
            r#"
@vertex fn edge_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = edge_corner(index);
    var out: VertexOutput;
    out.position = edge_position(uv);
    out.uv = vec2(0.25) + uv * 0.25;
    out.back_uv = out.uv;
    out.skin_layer = HAND_FIXTURE_LAYER;
    out.valid = 1u;
    out.shade = 1.0;
    return out;
}"#
        )
    } else {
        format!(
            "{}\n{QUAD}\n{}",
            crate::shader_source::standalone(include_str!("../../model.wesl"), &[]),
            r#"
@vertex fn edge_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = edge_corner(index);
    var out: VertexOutput;
    out.clip_position = edge_position(uv);
    out.uv = uv;
    out.material_flags = MODEL_FIXTURE_CUTOUT;
    out.visibility.x = 1u;
    out.visibility.y = 1u;
    out.native_light_levels = vec2(15.0);
    out.native_ao_face = 1.0;
    out.tint_gamma = vec3(1.0);
    return out;
}"#
        )
    };
    // Relocate the lightmap into the fixture's single bind group.
    let item_flag = crate::HAND_ITEM_LAYER_FLAG;
    let offhand_flag = crate::HAND_OFFHAND_LAYER_FLAG;
    let blend_flag = crate::HandItemAlphaMode::Blend.texture_layer_flag();
    let cutout_flag = crate::HandItemAlphaMode::Cutout.texture_layer_flag();
    let source = source
        .replace("EDGE_TARGET_SIDE", &format!("{SNAPSHOT_SIDE}.0"))
        .replace(
            "MODEL_FIXTURE_CUTOUT",
            &format!("{}u", assets::MATERIAL_FLAG_ALPHA_CUTOUT),
        )
        .replace(
            "HAND_FIXTURE_LAYER",
            &format!("{}u", item_flag | cutout_flag),
        )
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
        .replace("@group(1) @binding(1)", "@group(0) @binding(21)");
    let source = if back {
        source.replace("edge_corner(index)", "edge_corner(5u - index)")
    } else {
        source
    };
    let view = raster(gpu, held);
    let lightmap = gpu.buffer(&[1.0; 1024], wgpu::BufferUsages::UNIFORM);
    let sampler = gpu.device.create_sampler(&if held {
        wgpu::SamplerDescriptor::default()
    } else {
        super::bind_groups::chunk_sampler_descriptor()
    });
    let bounded_sampler = gpu
        .device
        .create_sampler(&crate::material_shader::native_leaf_sampler_descriptor());
    let mut atmosphere = [0.0; 36];
    atmosphere[19] = 100.0;
    atmosphere[20] = 200.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let camera = gpu.buffer(&[0.0; 104], wgpu::BufferUsages::UNIFORM);
    let material = gpu.words(
        &[
            item_flag,
            offhand_flag,
            blend_flag,
            cutout_flag,
            !(item_flag | offhand_flag | blend_flag | cutout_flag),
            0,
            0,
            0,
        ],
        wgpu::BufferUsages::UNIFORM,
    );
    let hand_light = gpu.buffer(&[0.0; 24], wgpu::BufferUsages::UNIFORM);
    let texture_entry = |binding| wgpu::BindGroupEntry {
        binding,
        resource: wgpu::BindingResource::TextureView(&view),
    };
    let entries = if held {
        vec![
            texture_entry(6),
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            buffer_entry(8, &material),
            buffer_entry(9, &hand_light),
            texture_entry(10),
            texture_entry(11),
            buffer_entry(20, &lightmap),
        ]
    } else {
        vec![
            buffer_entry(0, &camera),
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            buffer_entry(15, &atmosphere),
            texture_entry(crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[0]),
            texture_entry(crate::material_shader::NATIVE_LEAF_TEXTURE_BINDINGS[1]),
            wgpu::BindGroupEntry {
                binding: crate::material_shader::NATIVE_LEAF_SAMPLER_BINDING,
                resource: wgpu::BindingResource::Sampler(&bounded_sampler),
            },
            buffer_entry(20, &lightmap),
        ]
    };
    gpu.render_with_samples(
        &source,
        "edge_vertex",
        &[Draw {
            fragment: if held { "hand_fragment" } else { "fragment" },
            vertices: 0..6,
            bindings: &entries,
            blend: None,
            write_depth: true,
        }],
        samples,
    )
}

/// Checks transparent top rows at both full and partial geometric pixel coverage.
fn check_edge(held: bool) {
    let Some(gpu) = Gpu::for_fixture("multisampled alpha-tested top edges") else {
        return;
    };
    for (samples, back) in [(1, false), (1, true), (4, false), (4, true)] {
        let pixels = render_edge(&gpu, held, samples, back);
        save(
            &format!(
                "{}-edge-{samples}-{}",
                if held { "held-atlas" } else { "wheat-repeat" },
                if back { "back" } else { "front" }
            ),
            &pixels,
        );
        assert!(
            pixels.as_chunks::<4>().0.iter().any(|pixel| pixel[1] > 200),
            "opaque interior must render"
        );
        let mut stray = 0;
        for x in 40..210 {
            let top = 60.2 - (x as f32 + 0.5 - 30.2) * 36.0 / 190.0;
            for y in (top as usize - 1)..=(top as usize + 2) {
                let offset = (y * SNAPSHOT_SIDE as usize + x) * 4;
                stray += usize::from(pixels[offset + 1] > 50);
            }
        }
        assert_eq!(
            stray, 0,
            "transparent top edge fetched exterior texels with {samples} samples (held={held}, back={back})"
        );
    }
}

#[test]
fn wheat_top_edge_does_not_repeat_opaque_bottom_texels() {
    check_edge(false);
}

#[test]
fn held_flower_top_edge_does_not_sample_the_neighbouring_sprite() {
    check_edge(true);
}
