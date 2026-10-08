//! First-person lighting through the production vertex and fragment entry points.
use crate::{
    gpu_snapshot::{Draw, Gpu},
    shader_safety, shader_source,
};
use bevy::math::{Mat4, Vec3};
use render::{ActorGpuInstance, ActorRigGeometrySpan, HandItemAlphaMode, HandRigLight};
use render_model::ActorRigVertex;

/// Resolves packed storage constants and binds the real lightmap beside the hand resources.
fn source() -> String {
    let shader = shader_safety::from_actor_wgsl(
        include_str!("../../src/hand_rig.wgsl"),
        "hand_lighting.wgsl",
        render::ACTOR_GPU_INSTANCE_WORDS,
        render_model::ACTOR_RIG_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wgsl(source) = shader.source else {
        panic!("WGSL shader")
    };
    shader_source::standalone(&source, &[])
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
        .replace("@group(1) @binding(1)", "@group(0) @binding(21)")
}

#[test]
fn java_hand_lights_normals_and_composes_texture_in_gamma() {
    let Some(gpu) = Gpu::for_fixture("Java first-person lighting") else {
        return;
    };
    let identity: [[f32; 4]; 3] = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let projection = gpu.buffer(
        &bevy::math::Mat4::IDENTITY.to_cols_array(),
        wgpu::BufferUsages::UNIFORM,
    );
    let bones = gpu.buffer(bytemuck::cast_slice(&identity), wgpu::BufferUsages::STORAGE);
    let spans = gpu.buffer(
        bytemuck::cast_slice(&[ActorRigGeometrySpan {
            first_vertex: 0,
            vertex_count: 6,
        }]),
        wgpu::BufferUsages::STORAGE,
    );
    let blend = HandItemAlphaMode::Blend.texture_layer_flag();
    let cutout = HandItemAlphaMode::Cutout.texture_layer_flag();
    let material = gpu.words(
        &[
            render::HAND_ITEM_LAYER_FLAG,
            render::HAND_OFFHAND_LAYER_FLAG,
            blend,
            cutout,
            !(render::HAND_ITEM_LAYER_FLAG | render::HAND_OFFHAND_LAYER_FLAG | blend | cutout),
            0,
            0,
            0,
        ],
        wgpu::BufferUsages::UNIFORM,
    );
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("original lighting test texel"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &[128, 128, 128, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    let atlas = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&Default::default());
    for (layer, basis, stretch) in [
        (0, Mat4::IDENTITY, 1.0),
        (render::HAND_ITEM_LAYER_FLAG, Mat4::IDENTITY, 1.0),
        (
            render::HAND_ITEM_LAYER_FLAG,
            Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
            1.2,
        ),
    ] {
        let model = Mat4::from_scale(Vec3::new(1.0, 1.0, stretch)) * basis;
        let rows = model.transpose().to_cols_array_2d();
        let instances = gpu.buffer(
            bytemuck::cast_slice(&[ActorGpuInstance {
                world_from_actor: [rows[0], rows[1], rows[2]],
                texture_layer: layer,
                ..Default::default()
            }]),
            wgpu::BufferUsages::STORAGE,
        );
        for (java, normal, world_light, expected) in [
            (false, [0.0, -1.0, 0.0], 1.0_f32, 128.0),
            (true, [0.0, -1.0, 0.0], 1.0, 51.2),
            (true, [0.0, 1.0, 0.0], 1.0, 128.0),
            (
                true,
                [1.0, 0.0, 0.0],
                1.0,
                128.0 * (0.4 + stretch * 0.6 * 0.2 / 1.53_f32.sqrt()),
            ),
            (true, [0.0, 0.0, 1.0], 1.0, 94.6614),
            (true, [0.0, 1.0, 0.0], 0.5, 64.0),
        ] {
            let vertices = [
                (-1.0, -1.0),
                (1.0, -1.0),
                (1.0, 1.0),
                (-1.0, -1.0),
                (1.0, 1.0),
                (-1.0, 1.0),
            ]
            .map(|(x, y)| ActorRigVertex {
                position: model
                    .inverse()
                    .transform_point3(Vec3::new(x, y, 0.5))
                    .to_array(),
                normal: basis
                    .inverse()
                    .transform_vector3(Vec3::from_array(normal))
                    .to_array(),
                uv: [0.5; 2],
                back_uv: [0.5; 2],
                ..Default::default()
            });
            let vertices = gpu.buffer(bytemuck::cast_slice(&vertices), wgpu::BufferUsages::STORAGE);
            let mut light = HandRigLight::default();
            if java {
                light = light.with_java_lighting(Mat4::IDENTITY);
                light.java_normal_axes = [basis
                    .inverse()
                    .transform_vector3(Vec3::Z)
                    .extend(0.0)
                    .to_array(); 3];
            }
            let light = gpu.buffer(bytemuck::cast_slice(&[light]), wgpu::BufferUsages::UNIFORM);
            let lightmap = gpu.buffer(
                bytemuck::cast_slice(&[[world_light, world_light, world_light, 1.0]; 256]),
                wgpu::BufferUsages::UNIFORM,
            );
            let buffers = [
                (0, &projection),
                (1, &instances),
                (2, &vertices),
                (3, &spans),
                (4, &bones),
                (5, &bones),
                (8, &material),
                (9, &light),
                (20, &lightmap),
            ];
            let mut bindings: Vec<_> = buffers
                .into_iter()
                .map(|(binding, buffer)| wgpu::BindGroupEntry {
                    binding,
                    resource: buffer.as_entire_binding(),
                })
                .collect();
            bindings.extend([6, 10, 11].map(|binding| wgpu::BindGroupEntry {
                binding,
                resource: wgpu::BindingResource::TextureView(&atlas),
            }));
            bindings.push(wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::Sampler(&sampler),
            });
            let pixels = gpu.render_srgb(
                &source(),
                "hand_vertex",
                &[Draw {
                    fragment: "hand_fragment",
                    vertices: 0..6,
                    bindings: &bindings,
                    blend: None,
                    write_depth: true,
                }],
            );
            let observed = pixels[(128 * 256 + 128) * 4];
            assert!(
                (f32::from(observed) - expected).abs() <= 1.0,
                "Java={java}, layer={layer}, stretch={stretch}, normal={normal:?}, world={world_light}: expected {expected}, got {observed}"
            );
        }
    }
}
