use super::*;
use crate::{
    gpu_snapshot::{self, Draw, Gpu},
    shader_source,
};
use wgpu::util::DeviceExt;

/// Uploads a synthetic or explicitly supplied fixture image with the production format.
fn texture(gpu: &Gpu, width: u32, height: u32, pixels: &[u8]) -> wgpu::TextureView {
    gpu.device
        .create_texture_with_data(
            &gpu.queue,
            &wgpu::TextureDescriptor {
                label: Some("pack skybox regression image"),
                size: wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: crate::atmosphere_render::textures::SKYBOX_TEXTURE_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            },
            wgpu::util::TextureDataOrder::LayerMajor,
            pixels,
        )
        .create_view(&Default::default())
}

#[test]
fn numbered_faces_uvs_and_translated_face_boundaries_render_on_the_gpu() {
    let Some(gpu) = Gpu::for_fixture("pack skybox projection") else {
        return;
    };
    let mut source = shader_source::standalone(include_str!("../atmosphere.wgsl"), &[]);
    source.push_str(
        r#"
@vertex fn probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return VertexOutput(vec4(position, 0.0, 1.0), 0.0);
}
@fragment fn projection_probe(input: VertexOutput) -> @location(0) vec4<f32> {
    let rays = array<vec3<f32>, 16>(
        vec3(0.0, 0.0, -1.0), vec3(1.0, 0.0, 0.0), vec3(0.0, 0.0, 1.0),
        vec3(-1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0), vec3(0.0, -1.0, 0.0),
        vec3(0.25, 1.1, 1.0), vec3(0.25, -0.9, 1.0),
        vec3(0.5, 0.4, -1.0), vec3(1.0, 0.4, 0.5), vec3(0.5, 0.4, 1.0),
        vec3(-1.0, 0.4, 0.5), vec3(0.5, 1.0, 0.4), vec3(0.5, -1.0, 0.4),
        vec3(0.0, 0.0, 1.0), vec3(0.0, 0.0, -1.0));
    let ray = rays[u32(input.position.x) / 16u];
    let mapping = pack_skybox_mapping(ray, skybox.parameters.x);
    return vec4(mapping.xy, mapping.z / 5.0, 1.0);
}
"#,
    );
    for (enabled, expected) in [
        (
            false,
            [
                [128, 128, 0],
                [128, 128, 51],
                [128, 128, 102],
                [128, 128, 153],
                [128, 128, 204],
                [128, 128, 255],
                [156, 12, 204],
                [96, 242, 102],
                [191, 77, 0],
                [191, 77, 51],
                [64, 77, 102],
                [64, 77, 153],
                [191, 77, 204],
                [191, 179, 255],
                [128, 128, 102],
                [128, 128, 0],
            ],
        ),
        (
            true,
            [
                [128, 160, 0],
                [128, 160, 51],
                [128, 160, 102],
                [128, 160, 153],
                [128, 128, 204],
                [128, 128, 255],
                [96, 20, 102],
                [154, 233, 255],
                [191, 109, 0],
                [191, 109, 51],
                [64, 109, 102],
                [64, 109, 153],
                [208, 63, 204],
                [175, 165, 255],
                [128, 160, 102],
                [128, 160, 0],
            ],
        ),
    ] {
        let uniform = SkyboxUniform::new(
            AtmosphereFrame::default(),
            SkyboxSettings {
                native_vertical_offset: enabled,
                ..Default::default()
            },
            63,
        );
        let buffer = gpu.buffer(
            bytemuck::cast_slice(bytemuck::bytes_of(&uniform)),
            wgpu::BufferUsages::UNIFORM,
        );
        let entries = [wgpu::BindGroupEntry {
            binding: 7,
            resource: buffer.as_entire_binding(),
        }];
        let pixels = gpu.render(
            &source,
            "probe_vertex",
            &[Draw {
                fragment: "projection_probe",
                vertices: 0..3,
                bindings: &entries,
                blend: None,
                write_depth: false,
            }],
        );
        gpu_snapshot::save(
            if enabled {
                "pack-skybox-projection-native"
            } else {
                "pack-skybox-projection-centered"
            },
            &pixels,
        );
        for (column, expected) in expected.into_iter().enumerate() {
            let actual = &pixels[(128 * 256 + column * 16 + 8) * 4..][..3];
            assert!(
                actual.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1),
                "offset {enabled}, ray {column}: {actual:?}, expected {expected:?}"
            );
        }
    }
}

/// Draws the production cube fragment after an opaque synthetic backdrop.
fn draw_cube(
    gpu: &Gpu,
    textures: &[wgpu::TextureView; 6],
    frame: AtmosphereFrame,
    settings: SkyboxSettings,
    mask: u32,
    eye: bevy::math::Vec3,
) -> Vec<u8> {
    draw_cube_view(
        gpu,
        textures,
        frame,
        settings,
        mask,
        eye,
        bevy::math::Quat::IDENTITY,
        false,
    )
}

/// Draws with a perspective camera and optional opaque foreground depth.
#[allow(clippy::too_many_arguments)]
fn draw_cube_view(
    gpu: &Gpu,
    textures: &[wgpu::TextureView; 6],
    frame: AtmosphereFrame,
    settings: SkyboxSettings,
    mask: u32,
    eye: bevy::math::Vec3,
    rotation: bevy::math::Quat,
    foreground: bool,
) -> Vec<u8> {
    let mut source = shader_source::standalone(include_str!("../atmosphere.wgsl"), &[]);
    source.push_str(r#"
@vertex fn cube_probe_vertex(@builtin(vertex_index) index: u32) -> VertexOutput {
    let position = vec2(f32(index & 1u), f32((index >> 1u) & 1u)) * 4.0 - vec2(1.0);
    return VertexOutput(vec4(position, 0.0, 1.0), 0.0);
}
@fragment fn background(input: VertexOutput) -> @location(0) vec4<f32> { return vec4(0.0, 0.0, 0.25, skybox.parameters.z); }
@fragment fn foreground(input: VertexOutput) -> PackSkyboxOutput { return PackSkyboxOutput(vec4(0.0, 1.0, 0.0, skybox.parameters.z), 0.5); }
"#);
    let projection =
        bevy::math::Mat4::perspective_infinite_reverse_rh(90_f32.to_radians(), 1.0, 0.1);
    let world_from_view = bevy::math::Mat4::from_rotation_translation(rotation, eye);
    let clip = projection * world_from_view.inverse();
    let mut view_words = Vec::new();
    for matrix in [
        clip,
        clip,
        world_from_view.inverse(),
        world_from_view,
        projection,
        projection.inverse(),
    ] {
        view_words.extend(matrix.to_cols_array());
    }
    view_words.extend([eye.x, eye.y, eye.z, 1.0, 0.0, 0.0, 256.0, 256.0]);
    let view = gpu.buffer(&view_words, wgpu::BufferUsages::UNIFORM);
    let atmosphere = gpu.buffer(
        bytemuck::cast_slice(bytemuck::bytes_of(&frame)),
        wgpu::BufferUsages::UNIFORM,
    );
    let uniform = SkyboxUniform::new(frame, settings, mask);
    let skybox = gpu.buffer(
        bytemuck::cast_slice(bytemuck::bytes_of(&uniform)),
        wgpu::BufferUsages::UNIFORM,
    );
    let sampler = gpu
        .device
        .create_sampler(&crate::atmosphere_render::textures::skybox_sampler_descriptor());
    let mut entries = vec![
        wgpu::BindGroupEntry {
            binding: 0,
            resource: view.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 1,
            resource: atmosphere.as_entire_binding(),
        },
        wgpu::BindGroupEntry {
            binding: 7,
            resource: skybox.as_entire_binding(),
        },
    ];
    entries.extend(
        textures
            .iter()
            .enumerate()
            .map(|(face, texture)| wgpu::BindGroupEntry {
                binding: 8 + face as u32,
                resource: wgpu::BindingResource::TextureView(texture),
            }),
    );
    entries.push(wgpu::BindGroupEntry {
        binding: 14,
        resource: wgpu::BindingResource::Sampler(&sampler),
    });
    let mut draws = vec![Draw {
        fragment: "background",
        vertices: 0..3,
        bindings: &entries[2..3],
        blend: None,
        write_depth: false,
    }];
    if foreground {
        draws.push(Draw {
            fragment: "foreground",
            vertices: 0..3,
            bindings: &entries[2..3],
            blend: None,
            write_depth: true,
        });
    }
    draws.push(Draw {
        fragment: "pack_skybox_fragment",
        vertices: 0..3,
        bindings: &entries,
        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
        write_depth: false,
    });
    gpu.render_srgb_with_state(
        &source,
        "cube_probe_vertex",
        &draws,
        gpu_snapshot::RasterState {
            write_mask: wgpu::ColorWrites::RED | wgpu::ColorWrites::GREEN | wgpu::ColorWrites::BLUE,
            ..Default::default()
        },
    )
}

#[test]
fn translucent_faces_blend_over_the_backdrop_without_covering_nearer_geometry() {
    let Some(gpu) = Gpu::for_fixture("pack skybox transparency and depth") else {
        return;
    };
    let frame = AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.0);
    for alpha in [0, 128, 255] {
        let images = std::array::from_fn(|_| texture(&gpu, 1, 1, &[255, 0, 0, alpha]));
        let pixels = draw_cube(
            &gpu,
            &images,
            frame,
            SkyboxSettings::default(),
            63,
            bevy::math::Vec3::ZERO,
        );
        let centre = &pixels[(128 * 256 + 128) * 4..][..4];
        let linear = alpha as f32 / 255.0;
        let encode = |v| (crate::celestial::rgb_to_gamma([v; 3])[0] * 255.0).round() as u8;
        let expected = [encode(linear), 0, encode(0.25 * (1.0 - linear)), 255];
        assert!(
            centre.iter().zip(expected).all(|(a, e)| a.abs_diff(e) <= 1),
            "alpha {alpha}: {centre:?}, expected {expected:?}"
        );
        let foreground = draw_cube_view(
            &gpu,
            &images,
            frame,
            SkyboxSettings::default(),
            63,
            bevy::math::Vec3::ZERO,
            bevy::math::Quat::IDENTITY,
            true,
        );
        assert_eq!(&foreground[(128 * 256 + 128) * 4..][..4], &[0, 255, 0, 255]);
    }
}

#[test]
fn faces_filter_linearly_and_clamp_at_their_shared_boundary() {
    let Some(gpu) = Gpu::for_fixture("pack skybox face filtering") else {
        return;
    };
    let images = std::array::from_fn(|face| {
        if face == 0 {
            texture(&gpu, 2, 1, &[255, 0, 0, 255, 0, 0, 255, 255])
        } else {
            texture(&gpu, 1, 1, &[0, 255, 0, 255])
        }
    });
    let frame = AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.0);
    let settings = SkyboxSettings {
        native_vertical_offset: false,
        ..Default::default()
    };
    let pixels = draw_cube(&gpu, &images, frame, settings, 63, bevy::math::Vec3::ZERO);
    let middle = &pixels[(128 * 256 + 128) * 4..][..4];
    assert!(
        middle[0].abs_diff(128) <= 2 && middle[2].abs_diff(128) <= 2,
        "linear midpoint: {middle:?}"
    );
    let boundary = draw_cube_view(
        &gpu,
        &images,
        frame,
        settings,
        63,
        bevy::math::Vec3::ZERO,
        bevy::math::Quat::from_rotation_y(-std::f32::consts::FRAC_PI_4),
        false,
    );
    assert_eq!(&boundary[(128 * 256 + 127) * 4..][..4], &[0, 0, 255, 255]);
    assert_eq!(&boundary[(128 * 256 + 128) * 4..][..4], &[0, 255, 0, 255]);
    gpu_snapshot::save("pack-skybox-face-boundary", &boundary);
}

#[test]
fn cube_render_preserves_fallback_and_is_independent_of_camera_height() {
    let Some(gpu) = Gpu::for_fixture("pack skybox composition") else {
        return;
    };
    let images =
        std::array::from_fn(|face| texture(&gpu, 1, 1, &[30 + face as u8 * 30, 80, 120, 255]));
    let frame = AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.0);
    let settings = SkyboxSettings::default();
    let fallback = draw_cube(&gpu, &images, frame, settings, 0, bevy::math::Vec3::ZERO);
    let cube = draw_cube(&gpu, &images, frame, settings, 63, bevy::math::Vec3::ZERO);
    assert_ne!(cube, fallback);
    assert_eq!(
        cube,
        draw_cube(
            &gpu,
            &images,
            frame,
            settings,
            63,
            bevy::math::Vec3::new(1200.0, -800.0, -900.0)
        )
    );
    assert_ne!(
        fallback,
        draw_cube(&gpu, &images, frame, settings, 1, bevy::math::Vec3::ZERO)
    );
    assert_eq!(
        fallback,
        draw_cube(&gpu, &images, frame, settings, 2, bevy::math::Vec3::ZERO)
    );
    for medium in [meshing::CameraMedium::Water, meshing::CameraMedium::Lava] {
        assert_eq!(
            cube,
            draw_cube(
                &gpu,
                &images,
                frame.with_camera_medium(medium),
                settings,
                63,
                bevy::math::Vec3::ZERO
            )
        );
    }
    assert_eq!(
        fallback,
        draw_cube(
            &gpu,
            &images,
            frame.with_sky_kind(SkyKind::Nether),
            settings,
            63,
            bevy::math::Vec3::ZERO
        )
    );
    assert_eq!(
        fallback,
        draw_cube(
            &gpu,
            &images,
            frame,
            SkyboxSettings {
                visible: false,
                ..settings
            },
            63,
            bevy::math::Vec3::ZERO
        )
    );
    let night = draw_cube(
        &gpu,
        &images,
        AtmosphereFrame::from_bedrock_time(18000.0, 0.0, 0.0),
        settings,
        63,
        bevy::math::Vec3::ZERO,
    );
    assert_eq!(night, cube);
    gpu_snapshot::save("pack-skybox-stock-fallback", &fallback);
    gpu_snapshot::save("pack-skybox-custom", &cube);
}

#[test]
fn supplied_pack_fixture_renders_both_offset_states() {
    let Some(path) = std::env::var_os("CINNABAR_SKYBOX_FIXTURE_DIR") else {
        eprintln!(
            "skipping supplied pack skybox capture: missing CINNABAR_SKYBOX_FIXTURE_DIR fixture"
        );
        return;
    };
    if (0..assets::SKYBOX_FACE_COUNT).any(|face| {
        !std::path::Path::new(&path)
            .join(format!("cubemap_{face}.png"))
            .is_file()
    }) {
        eprintln!("skipping supplied pack skybox capture: missing numbered PNG face fixture");
        return;
    }
    let Some(gpu) = Gpu::for_fixture("supplied pack skybox capture") else {
        return;
    };
    let images = std::array::from_fn(|face| {
        let image = image::open(std::path::Path::new(&path).join(format!("cubemap_{face}.png")))
            .unwrap()
            .to_rgba8();
        texture(&gpu, image.width(), image.height(), image.as_raw())
    });
    let frame = AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.0);
    let fallback = draw_cube(
        &gpu,
        &images,
        frame,
        SkyboxSettings::default(),
        0,
        bevy::math::Vec3::ZERO,
    );
    let native = draw_cube(
        &gpu,
        &images,
        frame,
        SkyboxSettings::default(),
        63,
        bevy::math::Vec3::ZERO,
    );
    let centered = draw_cube(
        &gpu,
        &images,
        frame,
        SkyboxSettings {
            native_vertical_offset: false,
            ..Default::default()
        },
        63,
        bevy::math::Vec3::ZERO,
    );
    assert_ne!(native, fallback);
    assert_ne!(native, centered);
    gpu_snapshot::save("supplied-pack-skybox-backdrop", &fallback);
    gpu_snapshot::save("supplied-pack-skybox-native", &native);
    gpu_snapshot::save("supplied-pack-skybox-centered", &centered);
    for (name, rotation) in [
        ("north", bevy::math::Quat::IDENTITY),
        (
            "east",
            bevy::math::Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
        ),
        (
            "south",
            bevy::math::Quat::from_rotation_y(std::f32::consts::PI),
        ),
        (
            "west",
            bevy::math::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        ),
        ("up", bevy::math::Quat::from_rotation_x(80_f32.to_radians())),
        (
            "down",
            bevy::math::Quat::from_rotation_x(-80_f32.to_radians()),
        ),
    ] {
        for enabled in [true, false] {
            let pixels = draw_cube_view(
                &gpu,
                &images,
                frame,
                SkyboxSettings {
                    native_vertical_offset: enabled,
                    ..Default::default()
                },
                63,
                bevy::math::Vec3::ZERO,
                rotation,
                false,
            );
            gpu_snapshot::save(
                &format!(
                    "supplied-pack-{name}-{}",
                    if enabled { "native" } else { "centered" }
                ),
                &pixels,
            );
        }
    }
}

#[test]
fn vertical_offset_defaults_to_native_and_only_changes_overworld_geometry() {
    let native = SkyboxSettings::default();
    let centered = SkyboxSettings {
        native_vertical_offset: false,
        ..Default::default()
    };
    for kind in [SkyKind::Overworld, SkyKind::End, SkyKind::Nether] {
        let frame = AtmosphereFrame::default().with_sky_kind(kind);
        let on = SkyboxUniform::new(frame, native, 63);
        let off = SkyboxUniform::new(frame, centered, 63);
        assert_eq!(off.parameters[0], 0.0);
        assert_eq!(
            on.parameters[0],
            if kind == SkyKind::Overworld {
                0.256
            } else {
                0.0
            }
        );
        assert_eq!(on.parameters[1], 63.0);
    }
}

#[test]
fn pack_pixels_remain_untinted_across_the_day_cycle_weather_and_end_fog() {
    let Some(gpu) = Gpu::for_fixture("pack skybox day and weather transfer") else {
        return;
    };
    let images = std::array::from_fn(|_| texture(&gpu, 1, 1, &[120, 160, 200, 255]));
    let noon = AtmosphereFrame::from_bedrock_time(6000.0, 0.0, 0.0);
    let midnight = AtmosphereFrame::from_bedrock_time(18000.0, 0.0, 0.0);
    let rain = AtmosphereFrame::from_bedrock_time(6000.0, 1.0, 0.0);
    let thunder = AtmosphereFrame::from_bedrock_time(6000.0, 1.0, 1.0);
    let end = noon.with_sky_kind(SkyKind::End);
    let frames = [
        noon,
        midnight,
        rain,
        thunder,
        end,
        end.with_boss_environment(true, true),
    ];
    let pixels = frames.map(|frame| {
        draw_cube(
            &gpu,
            &images,
            frame,
            SkyboxSettings::default(),
            63,
            bevy::math::Vec3::ZERO,
        )
    });
    for image in &pixels {
        assert_eq!(image, &pixels[0]);
        assert_eq!(&image[(128 * 256 + 128) * 4..][..4], &[120, 160, 200, 255]);
    }
}
