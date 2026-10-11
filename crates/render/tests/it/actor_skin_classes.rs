//! Skins drawn from their native-resolution class arrays match the standard-raster array.
use crate::gpu_snapshot;

use bevy::math::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu};
use render::ActorGpuInstance;
use render_model::{ActorRigVertex, EntityRigId, STANDARD_SKIN_SIDE};

fn box_uv_cube() -> Vec<ActorRigVertex> {
    let geometry = serde_json::json!({
        "format_version":"1.12.0",
        "minecraft:geometry":[{
            "description":{"identifier":"geometry.fixture","texture_width":64,"texture_height":64},
            "bones":[{"name":"root","pivot":[0,0,0],"cubes":[{
                "origin":[-4,0,-4],"size":[8,8,8],"uv":[0,0]
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

fn noise(side: usize) -> Vec<u8> {
    let mut state = 0x2545_f491_4f6c_dd1d_u64;
    (0..side * side)
        .flat_map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            let [r, g, b, ..] = state.to_le_bytes();
            [r, g, b, 255]
        })
        .collect()
}

fn upscale(native: &[u8], side: usize) -> Vec<u8> {
    let scale = STANDARD_SKIN_SIDE / side;
    (0..STANDARD_SKIN_SIDE * STANDARD_SKIN_SIDE)
        .flat_map(|index| {
            let (x, y) = (
                index % STANDARD_SKIN_SIDE / scale,
                index / STANDARD_SKIN_SIDE / scale,
            );
            let at = (y * side + x) * 4;
            <[u8; 4]>::try_from(&native[at..at + 4]).unwrap()
        })
        .collect()
}

fn skin_array(gpu: &Gpu, side: usize, texels: &[u8]) -> wgpu::TextureView {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("skin class witness"),
        size: wgpu::Extent3d {
            width: side as u32,
            height: side as u32,
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
        texels,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(side as u32 * 4),
            rows_per_image: Some(side as u32),
        },
        texture.size(),
    );
    texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    })
}

/// Draws the cube in perspective, sampling `layer` from `views` bound at 6, 9, 10 and 11.
fn raster(
    gpu: &Gpu,
    vertices: &[ActorRigVertex],
    views: [&wgpu::TextureView; 4],
    layer: u32,
) -> Vec<u8> {
    let eye = Vec3::new(0.45, 0.7, 0.55);
    let clip = Mat4::perspective_infinite_reverse_rh(1.0, 1.0, 0.05)
        * Mat4::look_at_rh(eye, Vec3::new(0.0, 0.25, 0.0), Vec3::Y);
    let view = gpu.buffer(&gpu_snapshot::view(clip, eye), wgpu::BufferUsages::UNIFORM);
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
        texture_layer: layer,
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
    fn buffer(binding: u32, buffer: &wgpu::Buffer) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding,
            resource: buffer.as_entire_binding(),
        }
    }
    fn texture(binding: u32, view: &wgpu::TextureView) -> wgpu::BindGroupEntry<'_> {
        wgpu::BindGroupEntry {
            binding,
            resource: wgpu::BindingResource::TextureView(view),
        }
    }
    let glint = gpu.blank_texture_view();
    let bindings = [
        buffer(0, &view),
        buffer(1, &instance),
        buffer(2, &mesh),
        buffer(3, &spans),
        buffer(4, &bones),
        buffer(5, &bones),
        texture(6, views[0]),
        wgpu::BindGroupEntry {
            binding: 7,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        buffer(8, &class),
        texture(9, views[1]),
        texture(10, views[2]),
        texture(11, views[3]),
        texture(12, &glint),
        wgpu::BindGroupEntry {
            binding: 13,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        buffer(20, &lightmap),
        buffer(21, &atmosphere),
    ];
    gpu.render(
        &crate::actor_sidedness::source(),
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

#[test]
fn native_class_arrays_draw_the_same_pixels_as_the_standard_raster() {
    let Some(gpu) =
        Gpu::for_fixture("native_class_arrays_draw_the_same_pixels_as_the_standard_raster")
    else {
        return;
    };
    let cube = box_uv_cube();
    let black = [0, 0, 0, 255].repeat(STANDARD_SKIN_SIDE * STANDARD_SKIN_SIDE);
    let decoy = skin_array(&gpu, STANDARD_SKIN_SIDE, &black);
    for (class, side) in [(1, 64), (2, 128), (3, 256)] {
        let native = noise(side);
        let standard = skin_array(&gpu, STANDARD_SKIN_SIDE, &upscale(&native, side));
        let small = skin_array(&gpu, side, &native);
        let reference = raster(&gpu, &cube, [&standard; 4], 0);
        // Every other binding holds a black decoy, so only the class array can match.
        let mut views = [&decoy; 4];
        views[class] = &small;
        let drawn = raster(&gpu, &cube, views, render::pack_skin_slot(class, 0));
        let background = &reference[..4];
        let covered = reference
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| *pixel != background)
            .count();
        assert!(covered > 5_000, "{side}-texel cube covers {covered} pixels");
        let differing = reference
            .as_chunks::<4>()
            .0
            .iter()
            .zip(drawn.as_chunks::<4>().0.iter())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differing, 0,
            "{side}-texel class differs in {differing} pixels"
        );
    }
}
