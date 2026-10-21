//! Geometry-frame depth, custom-image feedback and viewport coverage.

use super::*;

#[test]
fn native_overlay_refreshes_pre_translucent_depth() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let (_, bytes) = render_frame_options(
        &gpu,
        &[
            ("gbuffers_basic.vsh", PASS_VERTEX),
            ("gbuffers_basic.fsh", FILL),
            ("gbuffers_terrain.vsh", PASS_VERTEX),
            ("gbuffers_terrain.fsh", INVERT),
            ("final.vsh", PASS_VERTEX),
            (
                "final.fsh",
                "#version 120\nuniform sampler2D depthtex1; varying vec2 texcoord; void main() { gl_FragColor = vec4(vec3(texture2D(depthtex1, texcoord).r), 1.0); }",
            ),
        ],
        1,
        [0, 0, 4, 4],
        false,
        true,
    );
    assert_pixels(&bytes, |_| [191; 3]);
}

#[test]
fn geometry_draws_refresh_custom_image_feedback_before_opening_the_pass() {
    use crate::shaderpack::frame_geometry::open_kind;
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let source = pack(&[
        ("gbuffers_basic.vsh", PASS_VERTEX),
        ("gbuffers_basic.fsh", FILL),
        (
            "shaders.properties",
            "image.marker = markerSampler RGBA RGBA8 UNSIGNED_BYTE true false 1 1\n",
        ),
        ("gbuffers_terrain.vsh", PASS_VERTEX),
        (
            "gbuffers_terrain.fsh",
            "#version 430\nuniform image2D marker; uniform sampler2D markerSampler; void main() { vec4 c = texelFetch(markerSampler, ivec2(0), 0); imageStore(marker, ivec2(0), c); gl_FragData[0] = c; }",
        ),
    ]);
    let pack = Arc::new(
        build(
            &source,
            "fixture",
            JAVA_DIMENSIONS[0],
            None,
            &gpu,
            &Default::default(),
            &Default::default(),
        )
        .unwrap(),
    );
    let mut state = ViewState::new(&gpu.device, Arc::clone(&pack), SIZE).unwrap();
    let texture = &state.targets.images[0].0;
    gpu.queue.write_texture(
        texture.as_image_copy(),
        &[255, 0, 0, 255],
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(4),
            rows_per_image: None,
        },
        texture.size(),
    );
    let mut encoder = gpu.device.create_command_encoder(&Default::default());
    let depth = state.targets.geometry_depth.as_ref().unwrap().view.clone();
    let extent = state.targets.screen_size;
    drop(
        open_kind(
            &mut encoder,
            &mut state,
            crate::shaderpack::draws::DrawKind::TerrainSolid,
            0,
            Some((&depth, extent)),
        )
        .unwrap(),
    );
    let source = &state.targets.image_snapshots[0].as_ref().unwrap().view;
    let pipeline = &pack
        .host
        .present
        .iter()
        .find(|(f, _)| *f == render_scene::SCENE_COLOR_FORMAT)
        .unwrap()
        .1;
    let group = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::TextureView(source),
        }],
    });
    // Read the snapshot through a compatible color attachment, without requiring COPY_SRC on it.
    let output = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: render_scene::SCENE_COLOR_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    {
        let view = output.create_view(&Default::default());
        let mut pass = crate::shaderpack::passes::pass(
            &mut encoder,
            "snapshot probe",
            &[crate::shaderpack::passes::load_attachment(&view)],
            None,
        );
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.draw(0..3, 0..1);
    }
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        output.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: None,
            },
        },
        output.size(),
    );
    gpu.queue.submit([encoder.finish()]);
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, |r| r.unwrap());
    gpu.device
        .poll(wgpu::PollType::wait_indefinitely())
        .unwrap();
    assert_eq!(
        &readback.slice(..).get_mapped_range().unwrap()[..4],
        &[255, 0, 0, 255]
    );
}

#[test]
fn geometry_overlay_crops_the_native_camera_viewport() {
    let Some(gpu) = gpu() else {
        eprintln!("missing fixture: native GPU adapter for the shader pack frame");
        return;
    };
    let (_, bytes) = render_frame_options(
        &gpu,
        &[
            ("gbuffers_basic.vsh", PASS_VERTEX),
            ("gbuffers_basic.fsh", FILL),
            ("gbuffers_terrain.vsh", PASS_VERTEX),
            ("gbuffers_terrain.fsh", INVERT),
            ("final.vsh", PASS_VERTEX),
            ("final.fsh", FINAL),
        ],
        1,
        [1, 0, 2, 2],
        false,
        true,
    );
    assert_pixels(&bytes, |row| if row < 2 { [255, 0, 0] } else { [128; 3] });
}
