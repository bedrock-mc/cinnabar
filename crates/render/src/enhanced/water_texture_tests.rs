//! Exercises production refraction against real scene colour and reverse-Z depth textures.

use wgpu::util::DeviceExt as _;

#[test]
fn submerged_geometry_keeps_its_colour_beside_bright_sky_and_foreground_is_rejected() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU adapter for water texture regressions");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("water texture regression device");
    let size = wgpu::Extent3d {
        width: 2,
        height: 2,
        depth_or_array_layers: 1,
    };
    let colour = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("submerged plant beside cyan sky"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    queue.write_texture(
        colour.as_image_copy(),
        bytemuck::cast_slice(&[
            [0.08_f32, 0.2, 0.04, 1.0],
            [30.0, 80.0, 120.0, 1.0],
            [0.08, 0.2, 0.04, 1.0],
            [30.0, 80.0, 120.0, 1.0],
        ]),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(32),
            rows_per_image: Some(2),
        },
        size,
    );
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("submerged plant and empty sky depth"),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("deterministic reverse-Z water fixture"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
@vertex fn vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32>{
    let p=array<vec2<f32>,3>(vec2(-1.0,-1.0),vec2(3.0,-1.0),vec2(-1.0,3.0));
    return vec4(p[index],0.0,1.0);
}
@fragment fn fragment(@builtin(position) p:vec4<f32>)->@builtin(frag_depth) f32 {
    return select(0.0,0.005,p.x<1.0);
}
"#
            .into(),
        ),
    });
    let depth_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("water regression depth"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &depth_shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &depth_shader,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[],
        }),
        primitive: Default::default(),
        multisample: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multiview: None,
        cache: None,
    });
    let shader_source = crate::shader_source::composed(
        r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_water::{filtered_scene_colour,water_refraction}
@group(0) @binding(0) var colour:texture_2d<f32>;
@group(0) @binding(1) var depth:texture_depth_2d;
@group(0) @binding(2) var linear_sampler:sampler;
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,4>;
@compute @workgroup_size(1) fn regression(){
    var frame:EnhancedFrame;
    frame.projection.x=0.05;
    frame.clip_from_world=mat4x4<f32>(vec4(1.0,0.0,0.0,0.0),vec4(0.0,1.0,0.0,0.0),
        vec4(0.0,0.0,0.0,-1.0),vec4(0.0,0.0,0.05,0.0));
    frame.world_from_clip=mat4x4<f32>(vec4(1.0,0.0,0.0,0.0),vec4(0.0,1.0,0.0,0.0),
        vec4(0.0,0.0,0.0,20.0),vec4(0.0,0.0,-1.0,0.0));
    results[0]=filtered_scene_colour(frame,colour,depth,vec2(0.5),0.005);
    results[1]=filtered_scene_colour(frame,colour,depth,vec2(0.5),0.05);
    results[2]=water_refraction(frame,colour,depth,linear_sampler,vec3(0.0,0.0,-5.0),
        vec3(0.0,0.0,1.0),vec3(0.0,0.0,-1.0),vec2(0.75,0.25),true);
    results[3]=water_refraction(frame,colour,depth,linear_sampler,vec3(0.0,0.0,-50.0),
        vec3(0.0,0.0,1.0),vec3(0.0,0.0,-1.0),vec2(0.25),true);
}
"#,
        &[],
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production water refraction regression"),
        source: wgpu::ShaderSource::Wgsl(shader_source.into()),
    });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: None,
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Depth,
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 22,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D3,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 23,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 31,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: false },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
        ],
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: None,
        bind_group_layouts: &[&layout],
        push_constant_ranges: &[],
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some("regression"),
        compilation_options: Default::default(),
        cache: None,
    });
    let output = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: None,
        contents: &[0; 64],
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let colour_view = colour.create_view(&Default::default());
    let depth_view = depth.create_view(&Default::default());
    let sampler = device.create_sampler(&Default::default());
    let mut encoder = device.create_command_encoder(&Default::default());
    let (scattering, scattering_sampler) =
        crate::enhanced::multiple_scattering::fixture(&device, &mut encoder);
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&colour_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 22,
                resource: wgpu::BindingResource::TextureView(&scattering),
            },
            wgpu::BindGroupEntry {
                binding: 23,
                resource: wgpu::BindingResource::Sampler(&scattering_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 31,
                resource: output.as_entire_binding(),
            },
        ],
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_pipeline(&depth_pipeline);
        pass.draw(0..3, 0..1);
    }
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 64);
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |result| {
        result.expect("water readback")
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("water GPU completion");
    let mapped = readback.slice(..).get_mapped_range();
    let values: &[[f32; 4]] = bytemuck::cast_slice(&mapped);
    for (channel, expected) in [0.08, 0.2, 0.04].into_iter().enumerate() {
        assert!(
            (values[0][channel] - expected).abs() < 0.00001,
            "sky must not pollute the submerged object's colour: {:?}",
            values[0]
        );
    }
    assert!((values[0][3] - 0.5).abs() < 0.00001);
    assert_eq!(values[1], [0.0; 4]);
    for result in &values[2..4] {
        assert_eq!(
            *result,
            [0.0, 0.0, 0.0, 48.0],
            "invalid refraction falls back to unlit deep water"
        );
    }
}
