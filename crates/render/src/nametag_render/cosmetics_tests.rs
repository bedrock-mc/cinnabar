//! Raster witnesses for an oversized cosmetic crossing an ordinary or sneaking label.

use super::*;
use crate::gpu_snapshot::{Draw, Gpu, RasterState};
use bevy::math::{Mat4, Vec3};
use wgpu::util::DeviceExt;

#[test]
fn overlapping_cosmetic_preserves_ordinary_ink_and_occludes_sneaking_ink() {
    let Some(gpu) = Gpu::for_fixture("nametag and oversized cosmetic") else {
        return;
    };
    let source = shader::from_wesl(include_str!("../nametag.wesl"), "cosmetic overlap");
    let source = crate::shader_source::standalone(source.source.as_str(), &[]);
    let uniform = gpu.buffer(
        &crate::gpu_snapshot::view(Mat4::IDENTITY, Vec3::ZERO),
        wgpu::BufferUsages::UNIFORM,
    );
    let mut glyphs = [0_u8; 16 * 8 * 4];
    for y in 1..7 {
        for x in 0..16 {
            if x == 1
                || x == 5
                || y == 4 && (1..=5).contains(&x)
                || x == 11
                || (y == 1 || y == 6) && (9..=13).contains(&x)
            {
                glyphs[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4].fill(255);
            }
        }
    }
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("original HI glyph fixture"),
            size: Extent3d {
                width: 16,
                height: 8,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &glyphs,
    );
    let atlas = texture.create_view(&default());
    let sampler = gpu.device.create_sampler(&default());
    let hat = NametagRecord {
        anchor: [0., 0., 0.8],
        rect: [-24., -24., 24., 24.],
        uv: [0., 0., -1., -1.],
        color: [0.2, 0.4, 0.6, 1.],
        depth_tested: 1,
        // This opaque geometry writes depth just as an opaque actor cosmetic does.
        text: 1,
        ..default()
    };
    let plate = NametagRecord {
        anchor: [0., 0., 0.5],
        rect: [-17., -9., 17., 9.],
        color: [0., 0., 0., 0.25],
        text: 0,
        depth_tested: 0,
        ..hat
    };
    let ink = NametagRecord {
        text: 1,
        rect: [-16., -8., 16., 8.],
        uv: [0., 0., 1., 1.],
        color: [1.; 4],
        ..plate
    };
    for (name, dim, tested) in [
        ("before", true, false),
        ("after", false, false),
        ("sneaking", true, true),
    ] {
        let mut records = vec![
            hat,
            NametagRecord {
                depth_tested: u32::from(tested),
                ..plate
            },
            NametagRecord {
                color: [
                    1.,
                    1.,
                    1.,
                    if dim {
                        render_model::NAMETAG_DIM_TEXT_ALPHA
                    } else {
                        1.
                    },
                ],
                depth_tested: u32::from(tested),
                ..ink
            },
        ];
        if dim && !tested {
            records.push(NametagRecord {
                depth_tested: 1,
                ..ink
            });
        }
        let buffer = gpu.buffer(bytemuck::cast_slice(&records), wgpu::BufferUsages::STORAGE);
        let bindings = [
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&atlas),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ];
        let mut state = RasterState::default();
        let draws = records
            .iter()
            .enumerate()
            .map(|(index, record)| {
                let mut descriptor = RenderPipelineDescriptor {
                    fragment: Some(FragmentState {
                        targets: vec![Some(ColorTargetState {
                            format: TextureFormat::Rgba8Unorm,
                            blend: None,
                            write_mask: ColorWrites::ALL,
                        })],
                        ..default()
                    }),
                    depth_stencil: Some(DepthStencilState {
                        format: CORE_3D_DEPTH_FORMAT,
                        depth_write_enabled: Some(false),
                        depth_compare: Some(CompareFunction::Always),
                        stencil: default(),
                        bias: default(),
                    }),
                    ..default()
                };
                NametagPipelineSpecializer
                    .specialize(
                        NametagPipelineKey {
                            msaa: Msaa::Off,
                            hdr: false,
                            gamma_blend: false,
                            depth_tested: record.depth_tested != 0,
                            text: record.text != 0,
                        },
                        &mut descriptor,
                    )
                    .unwrap();
                let depth = descriptor.depth_stencil.unwrap();
                state.depth_compares.push(depth.depth_compare.unwrap());
                Draw {
                    fragment: "nametag_fragment",
                    vertices: index as u32 * 6..(index as u32 + 1) * 6,
                    bindings: &bindings,
                    blend: descriptor.fragment.unwrap().targets[0]
                        .as_ref()
                        .unwrap()
                        .blend,
                    write_depth: depth.depth_write_enabled.unwrap(),
                }
            })
            .collect::<Vec<_>>();
        let pixels = gpu.render_with_state(&source, "nametag_vertex", &draws, state);
        crate::gpu_snapshot::save(&format!("nametag-cosmetic-{name}"), &pixels);
        let white_ink = pixels
            .as_chunks::<4>()
            .0
            .iter()
            .filter(|pixel| pixel[..3] == [255; 3])
            .count();
        assert_eq!(
            white_ink > 0,
            name == "after",
            "{name}: opaque name ink above the hat"
        );
    }
}
