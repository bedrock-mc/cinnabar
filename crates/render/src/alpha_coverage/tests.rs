use super::*;
use crate::gpu_snapshot::{Draw, Gpu, RasterState, SNAPSHOT_SIDE};
use bevy::math::{Mat4, Vec3};
use wgpu::util::DeviceExt;

#[test]
fn alpha_to_coverage_is_limited_to_multisampled_cutout_color_passes() {
    for samples in [1, 2, 4, 8] {
        for cutout in [false, true] {
            let mut descriptor = RenderPipelineDescriptor {
                fragment: Some(Default::default()),
                ..Default::default()
            };
            descriptor.multisample.count = samples;
            apply(&mut descriptor, cutout);
            assert_eq!(
                descriptor.multisample.alpha_to_coverage_enabled,
                cutout && samples > 1
            );
            assert_eq!(
                descriptor
                    .fragment
                    .unwrap()
                    .shader_defs
                    .contains(&SHADER_DEF.into()),
                cutout && samples > 1
            );
        }
    }
}

#[test]
fn alpha_coverage_shader_variants_validate() {
    let actor = include_str!("../actor.wgsl")
        .replace(
            "ACTOR_GPU_INSTANCE_WORDS",
            &crate::ACTOR_GPU_INSTANCE_WORDS.to_string(),
        )
        .replace(
            "ACTOR_RIG_VERTEX_WORDS",
            &render_model::ACTOR_RIG_VERTEX_WORDS.to_string(),
        );
    let block_entity = block_entity_source();
    for source in [
        include_str!("../chunk.wgsl"),
        include_str!("../model.wgsl"),
        include_str!("../dropped_item.wgsl"),
        &actor,
        &block_entity,
    ] {
        for definitions in [&[][..], &[SHADER_DEF][..]] {
            let module = naga::front::wgsl::parse_str(&crate::shader_source::standalone(
                source,
                definitions,
            ))
            .unwrap();
            naga::valid::Validator::new(
                naga::valid::ValidationFlags::all(),
                naga::valid::Capabilities::all(),
            )
            .validate(&module)
            .unwrap();
        }
    }
}

/// Uses the production constructor so the cutout witness shares every packed vertex constant.
fn block_entity_source() -> String {
    let shader = crate::shader_safety::from_block_entity_wgsl(
        include_str!("../block_entity/block_entity.wgsl"),
        "block_entity.wgsl",
        crate::BLOCK_ENTITY_VERTEX_WORDS,
        crate::BLOCK_SELECTION_VERTICES_PER_EDGE,
    );
    let bevy::shader::Source::Wgsl(source) = shader.source else {
        panic!("block entity shader is WGSL");
    };
    source.into_owned()
}

/// Draws the production block-entity cutout fragment with controlled alpha texels.
fn cutout_raster(gpu: &Gpu, samples: u32, coverage: bool) -> Vec<u8> {
    let source = block_entity_source();
    let mut source =
        crate::shader_source::standalone(&source, if coverage { &[SHADER_DEF] } else { &[] })
            .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
            .replace("@group(1) @binding(1)", "@group(0) @binding(21)");
    source.push_str(
        r#"
@vertex fn coverage_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    var out: VertexOutput;
    out.position = vec4(uv * vec2(2.0, -2.0) + vec2(-1.0, 1.0), 0.5, 1.0);
    out.uv = uv;
    out.uv.x += uv.y * 0.35 - 0.175 + 0.00137;
    out.color = vec4(1.0);
    out.native_lighting = vec3(1.0);
    out.world_position = vec3(0.0);
    out.actor_light = 0u;
    return out;
}
"#,
    );
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("owned cutout coverage fixture"),
            size: wgpu::Extent3d {
                width: 4,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &[255, 0, 0, 0, 255, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255],
    );
    let atlas = texture.create_view(&Default::default());
    let sampler = gpu
        .device
        .create_sampler(&wgpu::SamplerDescriptor::default());
    let view = gpu.buffer(
        &crate::gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let atmosphere = gpu.buffer(&[0.0; 32], wgpu::BufferUsages::UNIFORM);
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(&atlas),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 21,
            resource: atmosphere.as_entire_binding(),
        },
    ];
    gpu.render_with_state(
        &source,
        "coverage_vertex",
        &[Draw {
            fragment: "block_entity_solid",
            vertices: 0..3,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }],
        RasterState {
            multisample: wgpu::MultisampleState {
                count: samples,
                alpha_to_coverage_enabled: coverage,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}

#[test]
fn cutout_edge_has_partial_coverage_at_four_samples_without_blurring_opaque_texels() {
    let Some(gpu) = Gpu::for_fixture(
        "cutout_edge_has_partial_coverage_at_four_samples_without_blurring_opaque_texels",
    ) else {
        return;
    };
    let one = cutout_raster(&gpu, 1, false);
    let four = cutout_raster(&gpu, 4, true);
    let pixel = |frame: &[u8], column: usize, row: usize| {
        let index = (row * SNAPSHOT_SIDE as usize + column) * 4;
        <[u8; 4]>::try_from(&frame[index..index + 4]).unwrap()
    };
    let clear = pixel(&four, 0, 128);
    let solid = pixel(&four, 160, 128);
    let mut partial_rows = 0;
    for row in 0..SNAPSHOT_SIDE as usize {
        let mut partial = false;
        for column in 0..128 {
            let original = pixel(&one, column, row);
            assert!(
                original == clear || original == solid,
                "1x keeps the binary 0.5 alpha test"
            );
            let edge = pixel(&four, column, row);
            partial |= edge[0] > clear[0] && edge[0] < solid[0];
        }
        partial_rows += usize::from(partial);
        for column in 128..SNAPSHOT_SIDE as usize {
            assert_eq!(
                pixel(&one, column, row),
                pixel(&four, column, row),
                "coverage leaves the opaque red/green texel boundary unchanged"
            );
        }
    }
    assert!(
        partial_rows > SNAPSHOT_SIDE as usize / 2,
        "the binary diagonal alpha contour receives partial sample coverage"
    );
    crate::gpu_snapshot::save("cutout-before", &one);
    crate::gpu_snapshot::save("cutout-after", &four);
}
