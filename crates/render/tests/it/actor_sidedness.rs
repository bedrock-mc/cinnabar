//! Material sidedness through emitted cube geometry and the production actor shader.
use crate::{gpu_snapshot, shader_source};

use assets::EntityRenderMaterial;
use bevy::math::{Mat4, Vec3, Vec4};
use gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};
use render::ActorGpuInstance;
use render_model::{ActorRigVertex, EntityRigId};

fn cube(size: [u32; 3], top: bool, bottom: bool) -> Vec<ActorRigVertex> {
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

fn source() -> String {
    let actor = include_str!("../../src/actor.wgsl")
        .replace(
            "ACTOR_GPU_INSTANCE_WORDS",
            &render::ACTOR_GPU_INSTANCE_WORDS.to_string(),
        )
        .replace(
            "ACTOR_RIG_VERTEX_WORDS",
            &render_model::ACTOR_RIG_VERTEX_WORDS.to_string(),
        );
    shader_source::standalone(&actor, &[])
        .replace("@group(1) @binding(0)", "@group(0) @binding(10)")
        .replace("@group(1) @binding(1)", "@group(0) @binding(11)")
}

fn raster(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    material: EntityRenderMaterial,
    below: bool,
    texels: [[u8; 4]; 2],
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
        light: render::pack_actor_light(15, 15),
        multitexture_layers: [u32::MAX; 2],
        material: material as u32,
        dissolve_multiplier: 1.0,
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
            binding: 10,
            resource: lightmap.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 11,
            resource: atmosphere.as_entire_binding(),
        },
    ];
    gpu.render(
        &source(),
        "actor_vertex",
        &[Draw {
            fragment: "actor_fragment",
            vertices: 0..vertices.len() as u32,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }],
    )
}

fn center(frame: &[u8]) -> &[u8] {
    let index =
        (SNAPSHOT_SIDE as usize / 2 * SNAPSHOT_SIDE as usize + SNAPSHOT_SIDE as usize / 2) * 4;
    &frame[index..index + 4]
}

#[test]
fn dragon_and_dissolve_materials_reject_volume_backfaces() {
    let Some(gpu) = Gpu::for_fixture("dragon_and_dissolve_materials_reject_volume_backfaces")
    else {
        return;
    };
    let cube = cube([16; 3], true, false);
    assert_eq!(cube.len(), 6);
    let clear = center(&raster(
        &gpu,
        &cube,
        EntityRenderMaterial::Dragon,
        false,
        [[0; 4]; 2],
    ))
    .to_vec();
    for material in [
        EntityRenderMaterial::Dragon,
        EntityRenderMaterial::DissolveDepth,
        EntityRenderMaterial::DissolveColor,
    ] {
        let front = raster(&gpu, &cube, material, false, [[255; 4]; 2]);
        let back = raster(&gpu, &cube, material, true, [[255; 4]; 2]);
        assert_ne!(center(&front), clear, "{material:?} admits its outer face");
        assert_eq!(center(&back), clear, "{material:?} rejects its inner face");
    }
}

#[test]
fn collapsed_membrane_keeps_both_authored_uv_faces_in_one_sided_materials() {
    let Some(gpu) =
        Gpu::for_fixture("collapsed_membrane_keeps_both_authored_uv_faces_in_one_sided_materials")
    else {
        return;
    };
    let plane = cube([16, 0, 16], true, true);
    assert_eq!(plane.len(), 12);
    for material in [
        EntityRenderMaterial::Dragon,
        EntityRenderMaterial::DissolveColor,
    ] {
        let front = raster(
            &gpu,
            &plane,
            material,
            false,
            [[255, 0, 0, 255], [0, 255, 0, 255]],
        );
        let back = raster(
            &gpu,
            &plane,
            material,
            true,
            [[255, 0, 0, 255], [0, 255, 0, 255]],
        );
        assert!(
            center(&front)[0] > 200 && center(&front)[1] < 2,
            "front UV survives"
        );
        assert!(
            center(&back)[1] > 20 && center(&back)[0] < 2,
            "opposing UV survives"
        );
    }
}

#[test]
fn collapsed_membrane_back_uses_the_authored_opposing_face_normal() {
    let Some(gpu) =
        Gpu::for_fixture("collapsed_membrane_back_uses_the_authored_opposing_face_normal")
    else {
        return;
    };
    let plane = cube([16, 0, 16], true, true);
    let opposing = cube([16, 0, 16], false, true);
    for material in [
        EntityRenderMaterial::Dragon,
        EntityRenderMaterial::DissolveColor,
    ] {
        let collapsed = raster(&gpu, &plane, material, true, [[255; 4]; 2]);
        let authored = raster(&gpu, &opposing, material, true, [[255; 4]; 2]);
        assert_eq!(
            center(&collapsed),
            center(&authored),
            "{material:?}: collapsing two faces must preserve their independent shading"
        );
    }
}

#[test]
fn nocull_planes_keep_the_visible_authored_face_from_either_side() {
    let Some(gpu) =
        Gpu::for_fixture("nocull_planes_keep_the_visible_authored_face_from_either_side")
    else {
        return;
    };
    let plane = cube([16, 0, 16], true, true);
    for texels in [[[255; 4], [0; 4]], [[0; 4], [255; 4]]] {
        let clear = raster(
            &gpu,
            &plane,
            EntityRenderMaterial::Default,
            false,
            [[0; 4]; 2],
        );
        for below in [false, true] {
            let frame = raster(&gpu, &plane, EntityRenderMaterial::Default, below, texels);
            assert_ne!(
                center(&frame),
                center(&clear),
                "a nocull authored face must remain visible through the transparent opposing face: below {below}, texels {texels:?}"
            );
        }
    }
}
