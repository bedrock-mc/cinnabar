//! Shared actor geometry and shader raster fixture.
#![allow(
    dead_code,
    reason = "shared actor rasters serve different shader test targets"
)]

use super::{gpu_snapshot, render_api as render, shader_source};
use assets::EntityRenderMaterial;
use bevy::math::{Mat4, Vec3, Vec4};
use gpu_snapshot::{Draw, Gpu};
use render::ActorGpuInstance;
use render_model::{ActorRigVertex, EntityRigId};

pub(super) fn cube(size: [u32; 3], top: bool, bottom: bool) -> Vec<ActorRigVertex> {
    let face = |u| serde_json::json!({"uv":[u,0],"uv_size":[1,1]});
    let mut uv = serde_json::Map::new();
    if top {
        uv.insert("up".into(), face(0));
    }
    if bottom {
        uv.insert("down".into(), face(1));
    }
    let geometry = serde_json::json!({
        "format_version":"1.12.0",
        "minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture","texture_width":2,"texture_height":1},
            "bones":[{"name":"root","pivot":[0,0,0],"cubes":[{
                "origin":[-8,0,-8],"size":size,"uv":uv
            }]}]
        }]
    });
    let geometry = assets::parse_skin_geometry(
        r#"{"geometry":{"default":"geometry.fixture"}}"#,
        &geometry.to_string(),
    )
    .unwrap()
    .unwrap();
    render_model::skin_geometry(&geometry, EntityRigId(0))
        .unwrap()
        .vertices
        .to_vec()
}

pub(super) fn source(gamma: bool) -> String {
    let actor = include_str!("../../../src/actor.wesl")
        .replace(
            "ACTOR_GPU_INSTANCE_WORDS",
            &render::ACTOR_GPU_INSTANCE_WORDS.to_string(),
        )
        .replace(
            "ACTOR_RIG_VERTEX_WORDS",
            &render_model::ACTOR_RIG_VERTEX_WORDS.to_string(),
        );
    shader_source::standalone(&actor, if gamma { &["ACTOR_GAMMA_BLEND"] } else { &[] })
        .replace("@group(1) @binding(0)", "@group(0) @binding(20)")
        .replace("@group(1) @binding(1)", "@group(0) @binding(21)")
}

pub(super) fn raster(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: EntityRenderMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
) -> Vec<u8> {
    raster_material(
        gpu,
        vertices,
        render::ActorMaterial {
            kind: material,
            ..Default::default()
        },
        below,
        texels,
    )
}

pub(super) fn raster_material(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: render::ActorMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
) -> Vec<u8> {
    raster_material_target(gpu, vertices, material, below, texels, false, false)
}

pub(super) fn raster_material_target(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: render::ActorMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
    srgb: bool,
    gamma: bool,
) -> Vec<u8> {
    raster_material_lighting(
        gpu,
        vertices,
        material,
        below,
        texels,
        srgb,
        gamma,
        render::pack_actor_light(15, 15),
    )
}

#[allow(
    clippy::too_many_arguments,
    reason = "each knob is an independent shader input under test"
)]
pub(super) fn raster_material_lighting(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: render::ActorMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
    srgb: bool,
    gamma: bool,
    light: u32,
) -> Vec<u8> {
    raster_material_with_overlay(
        gpu, vertices, material, below, texels, srgb, gamma, light, 0, 0,
    )
}

/// Renders independent actor tint and overlay inputs with the fixed fixture camera.
#[allow(
    clippy::too_many_arguments,
    reason = "each knob is an independent shader input under test"
)]
pub(super) fn raster_material_with_overlay(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: render::ActorMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
    srgb: bool,
    gamma: bool,
    light: u32,
    overlay_rgba8: u32,
    tint_rgba8: u32,
) -> Vec<u8> {
    let clip = Mat4::from_cols(
        Vec4::new(2.0, 0.0, 0.0, 0.0),
        Vec4::ZERO,
        Vec4::new(0.0, if below { 2.0 } else { -2.0 }, 0.0, 0.0),
        Vec4::new(0.0, 0.0, 0.5, 1.0),
    );
    let view = gpu.buffer(
        &gpu_snapshot::view(clip, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let affine = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let instance = ActorGpuInstance {
        world_from_actor: affine,
        partial_tick: 1.0,
        uv_anim: render::IDENTITY_UV_ANIM,
        light,
        overlay_rgba8,
        tint: tint_rgba8,
        multitexture_layers: [u32::MAX; 2],
        material: material.gpu_word(),
        dissolve_multiplier: 1.0,
        light_color_multiplier: material.light_color_multiplier,
        ..Default::default()
    };
    let instance = gpu.buffer(
        bytemuck::cast_slice(&[instance]),
        wgpu::BufferUsages::STORAGE,
    );
    let mesh = gpu.buffer(bytemuck::cast_slice(vertices), wgpu::BufferUsages::STORAGE);
    let spans = gpu.buffer(
        &[f32::from_bits(0), f32::from_bits(vertices.len() as u32)],
        wgpu::BufferUsages::STORAGE,
    );
    let bones = gpu.buffer(bytemuck::cast_slice(&affine), wgpu::BufferUsages::STORAGE);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("opposing cube UV witness"),
        size: wgpu::Extent3d {
            width: 2,
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
        bytemuck::cast_slice(&texels),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(8),
            rows_per_image: Some(1),
        },
        texture.size(),
    );
    let skin = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = gpu.device.create_sampler(&wgpu::SamplerDescriptor {
        min_filter: wgpu::FilterMode::Nearest,
        mag_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });
    let class = gpu.buffer(&[0.0; 4], wgpu::BufferUsages::UNIFORM);
    let lightmap = gpu.buffer(&[1.0; 256 * 4], wgpu::BufferUsages::UNIFORM);
    let mut atmosphere = [0.0; 32];
    atmosphere[19] = 1000.0;
    atmosphere[20] = 1001.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: instance.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: mesh.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: spans.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 4,
            resource: bones.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: bones.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 6,
            resource: wgpu::BindingResource::TextureView(&skin),
        },
        wgpu::BindGroupEntry {
            binding: 7,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 8,
            resource: class.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 9,
            resource: wgpu::BindingResource::TextureView(&skin),
        },
        wgpu::BindGroupEntry {
            binding: 10,
            resource: wgpu::BindingResource::TextureView(&skin),
        },
        wgpu::BindGroupEntry {
            binding: 11,
            resource: wgpu::BindingResource::TextureView(&skin),
        },
        wgpu::BindGroupEntry {
            binding: 20,
            resource: lightmap.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 21,
            resource: atmosphere.as_entire_binding(),
        },
    ];
    let glint_view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut bindings = bindings.to_vec();
    bindings.push(wgpu::BindGroupEntry {
        binding: 12,
        resource: wgpu::BindingResource::TextureView(&glint_view),
    });
    bindings.push(wgpu::BindGroupEntry {
        binding: 13,
        resource: wgpu::BindingResource::Sampler(&sampler),
    });
    let draws = [Draw {
        fragment: "actor_fragment",
        vertices: 0..vertices.len() as u32,
        bindings: &bindings,
        blend: material.blend_state(),
        write_depth: material.state.is_none_or(|state| state.depth_write),
    }];
    if srgb {
        gpu.render_srgb(&source(gamma), "actor_vertex", &draws)
    } else {
        gpu.render(&source(gamma), "actor_vertex", &draws)
    }
}
