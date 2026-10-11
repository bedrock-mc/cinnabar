//! Exercise the production overlay shader against native cube UVs and atlas blending.
use crate::gpu_snapshot;

use bevy::prelude::{Mat4, Vec3};
use gpu_snapshot::{Draw, Gpu, RasterState};
use wgpu::util::DeviceExt;

fn uniform(portal_from_clip: Mat4) -> Vec<f32> {
    let mut words = vec![1.0, 0.0, 0.0, 0.0];
    words.extend([0.0; 4]);
    words.extend([1.0, 1.0, 0.0, 0.0]);
    words.extend([0.0, 1.0, 0.25, 1.0]);
    words.extend(portal_from_clip.to_cols_array());
    let layer_start = words.len();
    words.extend([
        1.0,
        1.0,
        1.0,
        0.75,
        render::ScreenOverlayKind::Portal as u32 as f32,
        0.0,
        0.0,
        0.0,
    ]);
    words.resize(layer_start + render::MAX_SCREEN_OVERLAY_LAYERS * 8, 0.0);
    words
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
    pixels[(y * 256 + x) * 4..][..4].try_into().unwrap()
}

#[test]
fn entering_portal_samples_and_blends_the_pack_texture_with_native_opacity() {
    let Some(gpu) =
        Gpu::for_fixture("entering_portal_samples_and_blends_the_pack_texture_with_native_opacity")
    else {
        return;
    };
    let projection = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_2,
        1.0,
        0.1,
    );
    let buffer = gpu.buffer(&uniform(projection.inverse()), wgpu::BufferUsages::UNIFORM);
    let texture = |pixels: &[u8]| {
        gpu.device
            .create_texture_with_data(
                &gpu.queue,
                &wgpu::TextureDescriptor {
                    label: Some("owned portal overlay regression texture"),
                    size: wgpu::Extent3d {
                        width: 1,
                        height: 1,
                        depth_or_array_layers: 2,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                },
                wgpu::util::TextureDataOrder::LayerMajor,
                pixels,
            )
            .create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
    };
    let overlays = texture(&[255; 8]);
    let portal = texture(&[255, 0, 0, 255, 0, 0, 255, 255]);
    let sampler = gpu
        .device
        .create_sampler(&wgpu::SamplerDescriptor::default());
    let entries = [
        wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: wgpu::BindingResource::TextureView(&overlays),
        },
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::TextureView(&overlays),
        },
        wgpu::BindGroupEntry {
            binding: 4,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
        wgpu::BindGroupEntry {
            binding: 5,
            resource: wgpu::BindingResource::TextureView(&portal),
        },
        wgpu::BindGroupEntry {
            binding: 6,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
    ];
    let pixels = gpu.render_with_state(
        include_str!("../../src/screen_overlay.wgsl"),
        "overlay_vertex",
        &[Draw {
            fragment: "overlay_fragment",
            vertices: 0..3,
            bindings: &entries,
            blend: None,
            write_depth: false,
        }],
        RasterState {
            depth_compare: wgpu::CompareFunction::Always,
            ..Default::default()
        },
    );
    let actual = pixel(&pixels, 128, 128);
    for (actual, expected) in actual.into_iter().zip([191_u8, 0, 64, 191]) {
        assert!(actual.abs_diff(expected) <= 1);
    }
}

#[test]
fn native_cube_uvs_follow_view_rotation_and_projection() {
    let Some(gpu) = Gpu::for_fixture("native_cube_uvs_follow_view_rotation_and_projection") else {
        return;
    };
    let projection = glam::camera::rh::proj::directx::perspective_infinite_reverse(
        std::f32::consts::FRAC_PI_2,
        1.0,
        0.1,
    );
    let view_rotation = Mat4::from_axis_angle(Vec3::Y, std::f32::consts::FRAC_PI_2);
    let buffer = gpu.buffer(
        &uniform(view_rotation * projection.inverse()),
        wgpu::BufferUsages::UNIFORM,
    );
    let source = format!(
        "{}\n@fragment fn cube_uv_fixture(in: VertexOutput) -> @location(0) vec4<f32> {{ return vec4<f32>(portal_cube_uv(in.uv), 0.0, 1.0); }}",
        include_str!("../../src/screen_overlay.wgsl")
    );
    let entries = [wgpu::BindGroupEntry {
        binding: 0,
        resource: buffer.as_entire_binding(),
    }];
    let pixels = gpu.render_with_state(
        &source,
        "overlay_vertex",
        &[Draw {
            fragment: "cube_uv_fixture",
            vertices: 0..3,
            bindings: &entries,
            blend: None,
            write_depth: false,
        }],
        RasterState {
            depth_compare: wgpu::CompareFunction::Always,
            ..Default::default()
        },
    );
    // Native West quad maps Z=-1 to U=0, Z=+1 to U=1 and Y=+1 to V=0.
    // Looking West reverses screen U; the cube remains world aligned.
    let actual = pixel(&pixels, 64, 128);
    assert!(actual[0].abs_diff(191) <= 1);
    assert!(actual[1].abs_diff(128) <= 1);
}
