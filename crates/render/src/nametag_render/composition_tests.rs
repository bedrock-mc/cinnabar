use super::*;

#[test]
fn translucent_plates_composite_scene_alpha_over_the_destination() {
    assert_eq!(NAMETAG_BLEND, BlendState::ALPHA_BLENDING);
}

#[test]
fn custom_label_colour_matches_each_attachment_colour_space() {
    use crate::gpu_snapshot::{Draw, Gpu};
    use wgpu::util::DeviceExt;
    let Some(gpu) = Gpu::for_fixture("nametag colour spaces") else {
        return;
    };
    let texture = gpu.device.create_texture_with_data(
        &gpu.queue,
        &wgpu::TextureDescriptor {
            label: Some("white label texel"),
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
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        &[255; 4],
    );
    let view = texture.create_view(&default());
    let sampler = gpu.device.create_sampler(&default());
    let bindings = [
        wgpu::BindGroupEntry {
            binding: 2,
            resource: wgpu::BindingResource::TextureView(&view),
        },
        wgpu::BindGroupEntry {
            binding: 3,
            resource: wgpu::BindingResource::Sampler(&sampler),
        },
    ];
    for gamma in [false, true] {
        let definitions = if gamma {
            &["NAMETAG_GAMMA_BLEND"][..]
        } else {
            &[][..]
        };
        let source = shader::from_wesl(include_str!("../nametag.wesl"), "nametag colour fixture");
        let mut source = crate::shader_source::standalone(source.source.as_str(), definitions);
        for textured in [0, 1] {
            let entry = format!("colour_vertex_{textured}");
            source.push_str(&format!(
                r#"
@vertex fn {entry}(@builtin(vertex_index) index: u32) -> VertexOutput {{
    var out: VertexOutput;
    let xy = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0))[index];
    out.position = vec4(xy, 0.5, 1.0);
    out.uv = vec2(0.5);
    out.color = vec4(0.5, 0.25, 0.75, 1.0);
    out.textured = {textured}u;
    return out;
}}
"#
            ));
            let pixels = gpu.render(
                &source,
                &entry,
                &[Draw {
                    fragment: "nametag_fragment",
                    vertices: 0..3,
                    bindings: &bindings,
                    blend: None,
                    write_depth: false,
                }],
            );
            crate::gpu_snapshot::save(&format!("nametag-colour-{gamma}-{textured}"), &pixels);
            let expected = if gamma {
                [128_u8, 64, 191, 255]
            } else {
                [55, 13, 133, 255]
            };
            for (actual, expected) in pixels[..4].iter().zip(expected) {
                assert!(
                    actual.abs_diff(expected) <= 1,
                    "gamma {gamma}, textured {textured}: {:?}",
                    &pixels[..4]
                );
            }
        }
    }
}

#[test]
fn coplanar_sneaking_shadow_never_rejects_the_following_glyph() {
    use crate::gpu_snapshot::{Draw, Gpu, RasterState};
    use bevy::math::{Mat4, Quat, Vec3};
    let Some(gpu) = Gpu::for_fixture("coplanar sneaking label shadow") else {
        return;
    };
    let source = shader::from_wesl(include_str!("../nametag.wesl"), "coplanar label fixture");
    let source = crate::shader_source::standalone(source.source.as_str(), &["NAMETAG_GAMMA_BLEND"]);
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("unused flat-quad atlas"),
        size: Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: TextureDimension::D2,
        format: TextureFormat::Rgba8Unorm,
        usage: TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let atlas = texture.create_view(&default());
    let sampler = gpu.device.create_sampler(&default());
    for yaw in [0.0, 0.4, 1.1] {
        for distance in [2.0, 16.0, 63.0] {
            let rotation = Quat::from_euler(bevy::math::EulerRot::YXZ, yaw, -0.3, 0.);
            let eye = Vec3::new(16.3, 2.1, 40.7);
            let camera = Mat4::from_rotation_translation(rotation, eye);
            let clip = glam::camera::rh::proj::directx::perspective_infinite_reverse(1.2, 1., 0.1)
                * camera.inverse();
            let mut words = crate::gpu_snapshot::view(clip, eye);
            words[48..64].copy_from_slice(&camera.to_cols_array());
            let uniform = gpu.buffer(bytemuck::cast_slice(&words), BufferUsages::UNIFORM);
            let glyph = NametagRecord {
                anchor: (eye + rotation * Vec3::new(0., 0., -distance)).to_array(),
                rect: [-16., -8., 16., 8.],
                uv: [0., 0., -1., -1.],
                color: [1., 1., 1., render_model::NAMETAG_DIM_TEXT_ALPHA],
                depth_tested: 1,
                text: 1,
                ..default()
            };
            let shadow = NametagRecord {
                rect: [-15., -7., 17., 9.],
                color: [0., 0., 0., glyph.color[3] * 0.8],
                ..glyph
            };
            let records = gpu.buffer(
                bytemuck::cast_slice(&[shadow, glyph]),
                BufferUsages::STORAGE,
            );
            let bindings = [
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: records.as_entire_binding(),
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
            let draw = |vertices, write_depth| Draw {
                fragment: "nametag_fragment",
                vertices,
                bindings: &bindings,
                blend: Some(NAMETAG_BLEND),
                write_depth,
            };
            let expected = gpu.render_with_state(
                &source,
                "nametag_vertex",
                &[draw(0..6, false), draw(6..12, true)],
                RasterState::default(),
            );
            let actual = gpu.render_with_state(
                &source,
                "nametag_vertex",
                &[draw(0..6, true), draw(6..12, true)],
                RasterState::default(),
            );
            assert!(
                actual == expected,
                "yaw {yaw}, distance {distance}: shadow depth changed {} channels",
                actual.iter().zip(&expected).filter(|(a, b)| a != b).count()
            );
        }
    }
}
