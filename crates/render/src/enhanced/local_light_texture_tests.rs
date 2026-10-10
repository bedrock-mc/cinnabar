//! Actual depth-array coverage and unchanged CPU/GPU preparation on a native device.

use super::super::*;
use wgpu::util::DeviceExt as _;

const DEPTH: &str = r#"
@vertex fn vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32> {
    let p=array<vec2<f32>,3>(vec2(-1.0,-1.0),vec2(3.0,-1.0),vec2(-1.0,3.0));
    return vec4(p[index],0.0,1.0);
}
@fragment fn fragment()->@builtin(frag_depth) f32{return 0.25;}
"#;
const SAMPLE: &str = r#"
#import cinnabar::enhanced_local_lights::{local_light_visibility,local_block_residual}
#import cinnabar::enhanced_view::local_material_lighting
#import cinnabar::enhanced_water::water_fresnel
@group(0) @binding(0) var shadow_map:texture_depth_2d_array;
@group(0) @binding(1) var shadow_sampler:sampler_comparison;
@group(0) @binding(31) var<storage,read_write> result:array<vec4<f32>,5>;
@compute @workgroup_size(1) fn regression(){
    result[0]=vec4(local_light_visibility(0u,vec3(2.0,0.0,0.0),vec3(-1.0,0.0,0.0),shadow_map,shadow_sampler),
        local_light_visibility(0u,vec3(-2.0,0.0,0.0),vec3(1.0,0.0,0.0),shadow_map,shadow_sampler),
        local_light_visibility(0u,vec3(0.0,2.0,0.0),vec3(0.0,-1.0,0.0),shadow_map,shadow_sampler),
        local_light_visibility(0u,vec3(0.0,0.0,2.0),vec3(0.0,0.0,-1.0),shadow_map,shadow_sampler));
    result[1]=vec4(local_light_visibility(0u,vec3(2.0,1.9,0.0),vec3(-1.0,0.0,0.0),shadow_map,shadow_sampler),0.0,0.0,1.0);
    let f0=water_fresnel(1.0,1.0/1.333);
    let glint=local_material_lighting(vec3(0.0),vec3(0.0,1.0,0.0),vec3(0.0,1.0,0.0),vec3(0.0,-2.0,0.0),vec2(0.0),0.2,0.0,f0,false,vec3(0.4));
    let blocked=local_material_lighting(vec3(0.0),vec3(-1.0,0.0,0.0),vec3(-1.0,0.0,0.0),vec3(2.0,0.0,0.0),vec2(0.0),0.2,0.0,f0,false,vec3(0.4));
    result[2]=vec4(glint,blocked.x);
    result[3]=vec4(local_block_residual(vec3(0.0),vec2(0.0)),
        local_block_residual(vec3(24.0,0.0,0.0),vec2(0.0)),
        local_block_residual(vec3(12.0,0.0,0.0),vec2(0.0)),1.0);
    result[4]=vec4(
        local_light_visibility(0u,vec3(2.001,0.0,2.0),normalize(vec3(-1.0,0.0,-1.0)),shadow_map,shadow_sampler),
        local_light_visibility(0u,vec3(2.0,0.0,2.001),normalize(vec3(-1.0,0.0,-1.0)),shadow_map,shadow_sampler),
        local_light_visibility(0u,vec3(2.0,0.0,0.08),vec3(-1.0,0.0,0.0),shadow_map,shadow_sampler),
        local_light_visibility(0u,vec3(2.0,0.0,-0.08),vec3(-1.0,0.0,0.0),shadow_map,shadow_sampler));
}
"#;

#[test]
fn native_point_shadow_face_and_embedded_viewport_reject_occluded_receivers() {
    let instance = crate::enhanced::validation::native_instance();
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!(
            "missing fixture: native GPU adapter for Enhanced point shadows and local-light caching"
        );
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("point-shadow regression device");
    let depth_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("embedded point occluder fixture"),
        source: wgpu::ShaderSource::Wgsl(DEPTH.into()),
    });
    let caster = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("embedded point occluder depth"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &depth_shader,
            entry_point: Some("vertex"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &depth_shader,
            entry_point: Some("fragment"),
            targets: &[],
            compilation_options: Default::default(),
        }),
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview: None,
        cache: None,
    });
    let receiver = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production local point visibility"),
        source: wgpu::ShaderSource::Wgsl(crate::shader_source::composed(SAMPLE, &[]).into()),
    });
    let sample = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("point-shadow native receivers"),
        layout: None,
        module: &receiver,
        entry_point: Some("regression"),
        compilation_options: Default::default(),
        cache: None,
    });
    let resolution = super::super::super::EnhancedRendering::default()
        .shadow_resolution
        .max(POINT_SHADOW_RESOLUTION * 2);
    let map = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("point faces within larger shadow layers"),
        size: wgpu::Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: POINT_SHADOW_FACES as u32,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let array = map.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let comparison = device.create_sampler(&wgpu::SamplerDescriptor {
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        compare: Some(wgpu::CompareFunction::LessEqual),
        ..Default::default()
    });
    let mut lights = LocalLightBlock::zeroed();
    lights.info = [1, POINT_SHADOW_RESOLUTION, 1, 0];
    lights.lights[0].position_radius = Vec3::ZERO.extend(LIGHT_RADIUS).to_array();
    lights.lights[0].radiance_shadow = [1.0, 1.0, 1.0, 1.0];
    lights.lights[0].shape = [0.0, POINT_SHADOW_NEAR, 0.0, 0.0];
    lights.lights[0].clip =
        point_shadow_matrices(Vec3::ZERO).map(|matrix| matrix.to_cols_array_2d());
    let sources = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("production local light GPU layout"),
        contents: bytemuck::bytes_of(&lights),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("local shadow results"),
        size: 80,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("local shadow readback"),
        size: 80,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let receivers = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("point receiver bindings"),
        layout: &sample.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&array),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&comparison),
            },
            wgpu::BindGroupEntry {
                binding: 31,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let empty = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &sample.get_bind_group_layout(1),
        entries: &[],
    });
    let mut tile_words = vec![0u32; 4 + TILE_LIGHTS + 1];
    tile_words[..5].copy_from_slice(&[1, 1, TILE_SIDE, TILE_LIGHTS as u32, 1]);
    let tiles = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("single admitted local-light tile"),
        contents: bytemuck::cast_slice(&tile_words),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let frame = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("raw local-shadow fixture frame"),
        contents: bytemuck::bytes_of(&crate::enhanced::frame::EnhancedFrameGpu::zeroed()),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let visibility = device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("unresolved receiver-history fixture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba16Float,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
        .create_view(&Default::default());
    let illumination = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("actual production local source binding"),
        layout: &sample.get_bind_group_layout(2),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: frame.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&comparison),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: sources.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 11,
                resource: tiles.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 16,
                resource: wgpu::BindingResource::TextureView(&visibility),
            },
            wgpu::BindGroupEntry {
                binding: 17,
                resource: wgpu::BindingResource::TextureView(&visibility),
            },
            wgpu::BindGroupEntry {
                binding: 18,
                resource: frame.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 19,
                resource: wgpu::BindingResource::TextureView(&visibility),
            },
            wgpu::BindGroupEntry {
                binding: 20,
                resource: wgpu::BindingResource::TextureView(&array),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    for face in 0..POINT_SHADOW_FACES {
        let target = map.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: face as u32,
            array_layer_count: Some(1),
            ..Default::default()
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        if face == 0 {
            pass.set_pipeline(&caster);
            pass.set_viewport(
                0.0,
                0.0,
                POINT_SHADOW_RESOLUTION as f32,
                POINT_SHADOW_RESOLUTION as f32,
                0.0,
                1.0,
            );
            pass.set_scissor_rect(0, 0, POINT_SHADOW_RESOLUTION, POINT_SHADOW_RESOLUTION);
            pass.draw(0..3, 0..1);
        }
    }
    let sample_receivers = |encoder: &mut wgpu::CommandEncoder| {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&sample);
        pass.set_bind_group(0, &receivers, &[]);
        pass.set_bind_group(1, &empty, &[]);
        pass.set_bind_group(2, &illumination, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    };
    sample_receivers(&mut encoder);
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 80);
    queue.submit([encoder.finish()]);
    let read = || {
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap();
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.recv().unwrap().unwrap();
        let values =
            bytemuck::cast_slice::<u8, [f32; 4]>(&readback.slice(..).get_mapped_range()).to_vec();
        readback.unmap();
        values
    };
    let values = read();
    assert!(
        values[0][0] < 0.01 && values[1][0] < 0.01,
        "occluded +X receivers must sample the populated quarter of the correct face"
    );
    assert!(
        values[0][1..].iter().all(|value| *value > 0.99),
        "neighbour faces remain lit"
    );
    assert!(
        values[2][..3].iter().all(|value| *value > 0.01),
        "local water glints exist with no sun binding or diffuse albedo"
    );
    assert!(
        values[2][3] < 0.00001,
        "local occlusion removes water glints independently of sun shadowing"
    );
    assert!(
        (values[3][0] - 0.3).abs() < 0.00001,
        "local source replaces only its covered propagation residual"
    );
    assert_eq!(
        &values[3][1..3],
        &[1.0, 1.0],
        "far receivers retain their propagated fallback"
    );
    lights.lights[0].shape[0] = SOURCE_RADIUS;
    queue.write_buffer(&sources, 0, bytemuck::bytes_of(&lights));
    let mut encoder = device.create_command_encoder(&Default::default());
    sample_receivers(&mut encoder);
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 80);
    queue.submit([encoder.finish()]);
    let seam = read()[4];
    assert!(
        (seam[0] - seam[1]).abs() < 0.15,
        "cross-face source filtering must not jump at a cube seam: {seam:?}"
    );
    assert!(
        seam[..2].iter().all(|value| (0.05..0.95).contains(value)),
        "finite emitters create a partial penumbra across cube faces: {seam:?}"
    );
    let face = map.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2),
        base_array_layer: 0,
        array_layer_count: Some(1),
        ..Default::default()
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &face,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&caster);
        pass.set_viewport(
            0.0,
            0.0,
            POINT_SHADOW_RESOLUTION as f32,
            POINT_SHADOW_RESOLUTION as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(0, 0, POINT_SHADOW_RESOLUTION / 2, POINT_SHADOW_RESOLUTION);
        pass.draw(0..3, 0..1);
    }
    sample_receivers(&mut encoder);
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 80);
    queue.submit([encoder.finish()]);
    let penumbra = read()[4];
    assert!(
        penumbra[2..]
            .iter()
            .all(|value| (0.05..0.95).contains(value)),
        "receiver points on either side of an emitter shadow edge remain softly filtered: {penumbra:?}"
    );
    lights.lights[0].shape[0] = 0.0;
    queue.write_buffer(&sources, 0, bytemuck::bytes_of(&lights));
    let mut encoder = device.create_command_encoder(&Default::default());
    for layer in 0..POINT_SHADOW_FACES {
        let target = map.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            base_array_layer: layer as u32,
            array_layer_count: Some(1),
            ..Default::default()
        });
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &target,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&caster);
        pass.set_viewport(
            0.0,
            0.0,
            POINT_SHADOW_RESOLUTION as f32,
            POINT_SHADOW_RESOLUTION as f32,
            0.0,
            1.0,
        );
        pass.set_scissor_rect(0, 0, POINT_SHADOW_RESOLUTION, POINT_SHADOW_RESOLUTION);
        pass.draw(0..3, 0..1);
    }
    sample_receivers(&mut encoder);
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 80);
    queue.submit([encoder.finish()]);
    let surrounded = read()[4];
    assert!(
        surrounded[..2].iter().all(|value| *value < 0.01),
        "comparison filtering must not bleed clear atlas padding into fully occluded cube seams: {surrounded:?}"
    );
    lights.info[2] = 0;
    queue.write_buffer(&sources, 0, bytemuck::bytes_of(&lights));
    let mut encoder = device.create_command_encoder(&Default::default());
    sample_receivers(&mut encoder);
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 80);
    queue.submit([encoder.finish()]);
    assert!(
        read()[0].iter().all(|value| *value > 0.99),
        "unready depth is never sampled as a valid shadow"
    );

    let render_device = RenderDevice::from(device);
    let render_queue = RenderQueue(std::sync::Arc::new(
        bevy::render::renderer::WgpuWrapper::new(queue),
    ));
    let mut view = LocalLightView::new(&render_device);
    let mut source = LocalLightSources::default();
    source.chunks.insert(
        Entity::from_bits(1),
        vec![LightSource {
            position: Vec3::new(0.0, 0.0, -24.0),
            level: 15,
            dimension: 0,
        }],
    );
    let clip = Mat4::perspective_rh(std::f32::consts::FRAC_PI_2, 1.0, 0.05, 96.0);
    view.prepare(
        &render_device,
        &render_queue,
        &source,
        Some(0),
        Vec3::ZERO,
        clip,
        [256, 256],
        0,
        false,
        true,
    );
    view.submitted.store(true, Ordering::Relaxed);
    let counts = (view.rebuilds, view.uploads);
    let pointers = (
        view.candidates.as_ptr(),
        view.tile_scratch.as_ptr(),
        view.tile_scores.as_ptr(),
        view.tile_rays.as_ptr(),
        view.shadows.as_ptr(),
    );
    assert!(!view.prepare(
        &render_device,
        &render_queue,
        &source,
        Some(0),
        Vec3::ZERO,
        clip,
        [256, 256],
        0,
        false,
        true
    ));
    assert_eq!(
        (view.rebuilds, view.uploads),
        counts,
        "unchanged local inputs rebuild and upload nothing"
    );
    assert_eq!(
        pointers,
        (
            view.candidates.as_ptr(),
            view.tile_scratch.as_ptr(),
            view.tile_scores.as_ptr(),
            view.tile_rays.as_ptr(),
            view.shadows.as_ptr()
        )
    );
    assert!(!view.dirty, "unchanged submitted point maps remain cached");
    source.record_changes(true, false);
    view.prepare(
        &render_device,
        &render_queue,
        &source,
        Some(0),
        Vec3::ZERO,
        clip,
        [256, 256],
        0,
        false,
        true,
    );
    assert!(
        view.dirty,
        "a changed nonemitting wall invalidates point maps"
    );
    assert_eq!(
        (view.rebuilds, view.uploads),
        counts,
        "geometry-only changes reuse source and tile GPU bytes"
    );
    let identities = view.shadow_lane_identities();
    let pool = (
        view.shadow_candidates.as_ptr(),
        view.shadow_candidates.clone(),
    );
    view.submitted.store(true, Ordering::Relaxed);
    view.prepare(
        &render_device,
        &render_queue,
        &source,
        Some(0),
        Vec3::ZERO,
        clip * Mat4::from_rotation_y(std::f32::consts::PI),
        [256, 256],
        0,
        false,
        true,
    );
    assert_eq!(
        view.shadow_lane_identities(),
        identities,
        "camera turns preserve shadow ownership"
    );
    assert_eq!(view.shadow_candidates.as_ptr(), pool.0);
    assert_eq!(view.shadow_candidates, pool.1);
    assert_eq!(
        view.data.info[3], 1,
        "offscreen owners remain in the dense atlas prefix"
    );
    assert!(
        !view.dirty,
        "turning alone does not recapture a lamp's static world"
    );
}
