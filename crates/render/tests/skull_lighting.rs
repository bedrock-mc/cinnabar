//! Placed mob-head witnesses through the production mesh, vertex and fragment shaders.
#[path = "support/gpu_snapshot.rs"]
mod gpu_snapshot;
#[path = "../src/material_shader.rs"]
#[allow(dead_code, reason = "shared shader substitutions")]
mod material_shader;
#[path = "../src/shader_safety.rs"]
#[allow(dead_code, reason = "shared checked shader constructors")]
mod shader_safety;
#[path = "support/shader_source.rs"]
mod shader_source;

use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu, SNAPSHOT_SIDE};
use render::{
    BlockEntityKind, BlockEntityLight, BlockEntityScene, BlockEntitySubmission, LightmapInputs,
    SceneClock, SkullKind, SkullModel, SkullMount,
};

fn center_pixel() -> usize {
    let side = SNAPSHOT_SIDE as usize;
    (side / 2 * side + side / 2) * 4
}

fn assets(texel: [u8; 4]) -> assets::RuntimeBlockEntityAssets {
    let bytes = assets::encode_block_entity_catalog(
        b"{}",
        64,
        64,
        &texel.repeat(64 * 64),
        &[assets::BlockEntityPlacement {
            name: "textures/entity/steve".into(),
            x: 0,
            y: 0,
            width: 64,
            height: 64,
        }],
    )
    .unwrap();
    assets::RuntimeBlockEntityAssets::decode(&bytes).unwrap()
}

fn source() -> String {
    let shader = shader_safety::from_block_entity_wgsl(
        include_str!("../src/block_entity/block_entity.wgsl"),
        "skull.wgsl",
        render::BLOCK_ENTITY_VERTEX_WORDS,
    );
    let bevy::shader::Source::Wgsl(source) = shader.source else {
        unreachable!()
    };
    // Remap resources for the shared readback helper, retaining production entry points.
    shader_source::standalone(&source, &[])
        .replace("@group(1) @binding(0)", "@group(0) @binding(4)")
        .replace("@group(1) @binding(1)", "@group(0) @binding(5)")
}

fn raster(
    gpu: &Gpu,
    assets: &assets::RuntimeBlockEntityAssets,
    light: BlockEntityLight,
    inputs: LightmapInputs,
    rotation: f32,
    gallery: bool,
) -> Vec<u8> {
    let mut scene = BlockEntityScene::default();
    scene.install_assets(assets);
    let frame = scene.update(
        SceneClock::default(),
        &[],
        &[BlockEntitySubmission {
            block: [0; 3],
            light,
            kind: BlockEntityKind::Skull(SkullModel {
                kind: SkullKind::Player,
                mount: SkullMount::Floor {
                    rotation_degrees: rotation,
                },
            }),
        }],
    );
    let center = Vec3::new(0.5, 0.25, 0.5);
    let front = Mat4::from_rotation_y(render::floor_yaw_degrees(rotation).to_radians())
        .transform_vector3(Vec3::NEG_Z);
    let eye = if gallery {
        center + Vec3::new(-1.2, 0.7, 2.0)
    } else {
        center + front * 2.0
    };
    let clip = Mat4::perspective_infinite_reverse_rh(35.0_f32.to_radians(), 1.0, 0.1)
        * Mat4::look_at_rh(eye, center, Vec3::Y);
    let view = gpu.buffer(&gpu_snapshot::view(clip, eye), wgpu::BufferUsages::UNIFORM);
    let vertices: &[f32] = bytemuck::cast_slice(&frame.solid);
    let vertices = gpu.buffer(vertices, wgpu::BufferUsages::STORAGE);
    let atlas = frame.atlas.as_ref().unwrap();
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("placed skull atlas"),
        size: wgpu::Extent3d {
            width: atlas.size[0],
            height: atlas.size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let mut pixels = atlas.static_rgba8.to_vec();
    pixels.resize((atlas.size[0] * atlas.size[1] * 4) as usize, 0);
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &pixels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(atlas.size[0] * 4),
            rows_per_image: Some(atlas.size[1]),
        },
        texture.size(),
    );
    let texture = texture.create_view(&Default::default());
    let sampler = gpu.device.create_sampler(&Default::default());
    let lightmap = gpu.buffer(
        bytemuck::cast_slice(&inputs.build()),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut atmosphere = [0.0; 32];
    atmosphere[20] = 1000.0;
    atmosphere[19] = 999.0;
    let atmosphere = gpu.buffer(&atmosphere, wgpu::BufferUsages::UNIFORM);
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: vertices.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(&texture),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 4,
            resource: lightmap.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: atmosphere.as_entire_binding(),
        },
    ];
    gpu.render_srgb(
        &source(),
        "block_entity_vertex",
        &[Draw {
            fragment: "block_entity_solid",
            vertices: 0..frame.solid.len() as u32,
            bindings: &bindings,
            blend: None,
            write_depth: true,
        }],
    )
}

fn sampled_light(inputs: LightmapInputs, block: u8, sky: u8) -> [f32; 3] {
    let table = inputs.build();
    // Native getColorForUV: /16 coordinates, clamp-linear sampling of RGB bytes.
    std::array::from_fn(|channel| {
        [
            (block, sky),
            (block.saturating_sub(1), sky),
            (block, sky.saturating_sub(1)),
            (block.saturating_sub(1), sky.saturating_sub(1)),
        ]
        .into_iter()
        .map(|(b, s)| {
            (table[usize::from(b) | (usize::from(s) << 4)][channel] * 255.0).floor() / 255.0
        })
        .sum::<f32>()
            * 0.25
    })
}

#[test]
fn skull_shader_validates_with_the_shared_native_lightmap() {
    let module = naga::front::wgsl::parse_str(&source()).unwrap();
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn placed_skull_pixels_match_native_day_night_torch_darkness_and_rotation() {
    let gpu = Gpu::new().unwrap();
    let texel = [128, 192, 64, 255];
    let assets = assets(texel);
    for (name, block, sky, sky_darken, brightness) in [
        ("day", 0, 15, 1.0, 0.0),
        ("night", 0, 15, 0.0, 0.0),
        ("torch", 12, 0, 0.0, 0.0),
        ("dark", 0, 0, 0.0, 0.0),
        ("brightness", 0, 8, 0.0, 1.0),
    ] {
        let inputs = LightmapInputs {
            ambient_adjustment: true,
            sky_darken,
            brightness,
            ..Default::default()
        };
        let light = sampled_light(inputs, block, sky);
        for rotation in [0.0, 45.0, 90.0] {
            let normal = Mat4::from_rotation_y(render::floor_yaw_degrees(rotation).to_radians())
                .transform_vector3(Vec3::NEG_Z)
                .to_array();
            let shade = render_api::fancy_actor_shade(normal, 0.0);
            let pixels = raster(
                &gpu,
                &assets,
                BlockEntityLight::Actor { block, sky },
                inputs,
                rotation,
                false,
            );
            let offset = center_pixel();
            for channel in 0..3 {
                let expected = f32::from(texel[channel]) * light[channel] * shade;
                assert!(
                    (f32::from(pixels[offset + channel]) - expected).abs() <= 2.0,
                    "{name} rotation {rotation}, channel {channel}: {} vs {expected}",
                    pixels[offset + channel]
                );
            }
            assert_eq!(pixels[offset + 3], 255);
        }
    }
}

#[test]
#[ignore = "requires a native GPU adapter; run explicitly on a GPU host"]
fn placed_skull_alpha_cutoff_and_legacy_linear_lighting_are_preserved() {
    let gpu = Gpu::new().unwrap();
    let inputs = LightmapInputs {
        ambient_adjustment: true,
        ..Default::default()
    };
    for alpha in [127, 128] {
        let pixels = raster(
            &gpu,
            &assets([255, 255, 255, alpha]),
            BlockEntityLight::Actor { block: 15, sky: 15 },
            inputs,
            0.0,
            false,
        );
        let pixel = &pixels[center_pixel()..][..3];
        assert_eq!(
            pixel[0] == pixel[1] && pixel[1] == pixel[2],
            alpha >= 128,
            "alpha below .5 must retain the colored clear background"
        );
    }
    let pixels = raster(&gpu, &assets([255; 4]), 0.25.into(), inputs, 0.0, false);
    let expected = (1.055 * (0.25_f32 * 0.8).powf(1.0 / 2.4) - 0.055) * 255.0;
    assert!((f32::from(pixels[center_pixel()]) - expected).abs() <= 2.0);
}

#[test]
#[ignore = "local visual gallery requires a native GPU and explicit carrier/output paths"]
fn write_local_placed_skull_gallery() {
    let carrier = std::env::var_os("CINNABAR_SKULL_GALLERY_CARRIER").expect("local carrier path");
    let output = std::env::var_os("CINNABAR_SKULL_GALLERY_OUT").expect("ignored-local output path");
    let assets =
        assets::RuntimeBlockEntityAssets::decode(&std::fs::read(carrier).unwrap()).unwrap();
    let gpu = Gpu::new().unwrap();
    let mut gallery = image::RgbaImage::new(5 * SNAPSHOT_SIDE, 2 * SNAPSHOT_SIDE);
    for (column, (name, block, sky, sky_darken, brightness, old_light)) in [
        ("day", 0, 15, 1.0, 0.0, 1.0),
        ("night", 0, 15, 0.0, 0.0, 1.0 / 12.0),
        ("torch", 12, 0, 0.0, 0.0, 0.5),
        ("dark", 0, 0, 0.0, 0.0, 0.0),
        ("brightness", 0, 8, 0.0, 1.0, (2.0 / 9.0) / 12.0),
    ]
    .into_iter()
    .enumerate()
    {
        let inputs = LightmapInputs {
            ambient_adjustment: true,
            sky_darken,
            brightness,
            ..Default::default()
        };
        for (row, light) in [old_light.into(), BlockEntityLight::Actor { block, sky }]
            .into_iter()
            .enumerate()
        {
            let pixels = raster(&gpu, &assets, light, inputs, 0.0, true);
            let frame = image::RgbaImage::from_raw(SNAPSHOT_SIDE, SNAPSHOT_SIDE, pixels).unwrap();
            image::imageops::replace(
                &mut gallery,
                &frame,
                column as i64 * i64::from(SNAPSHOT_SIDE),
                row as i64 * i64::from(SNAPSHOT_SIDE),
            );
        }
        println!("gallery column {column}: {name}; rows previous scalar / native mob-head");
    }
    gallery.save(output).unwrap();
}
