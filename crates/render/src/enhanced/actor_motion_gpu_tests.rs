//! Native storage-buffer and motion encoding regression using the production helpers.

use super::ActorMotionInstanceGpu;
use wgpu::util::DeviceExt as _;

#[test]
fn submitted_pose_reprojection_keeps_subpixel_precision_and_resets_invalid_lifetimes() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU adapter for submitted actor motion");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("submitted actor motion regression device"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("submitted actor motion device");
    let source = r#"
#import cinnabar::enhanced_actor_motion::{submitted_actor_position,submitted_surface_motion}
@group(0) @binding(29) var quantized_input:texture_2d<f32>;
@group(0) @binding(30) var quantized_output:texture_storage_2d<rgba16float,write>;
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,6>;
@compute @workgroup_size(1) fn regression(){
    results[0]=submitted_actor_position(0u,0u,vec3(1.0,2.0,3.0));
    results[1]=submitted_actor_position(1u,0u,vec3(1.0,2.0,3.0));
    results[2]=submitted_surface_motion(vec4(0.3,0.4,0.1,2.0),vec4(0.2,0.4,0.1,2.0),true);
    results[3]=submitted_surface_motion(vec4(0.3,0.4,0.1,2.0),vec4(0.2,0.4,0.1,2.0),false);
    let small=submitted_surface_motion(vec4(0.6,0.0,0.1,1.0),vec4(0.5999,0.0,0.1,1.0),true);
    textureStore(quantized_output,vec2(0),vec4(small.xy,0.79995,0.5));
    results[4]=vec4(small.x,0.0,0.0,1.0);
    results[5]=submitted_surface_motion(vec4(0.3,0.4,0.1,2.0),vec4(0.2,0.4,0.1,-2.0),true);
}
@compute @workgroup_size(1) fn read_quantization(){
    let stored=textureLoad(quantized_input,vec2(0),0);
    results[4]=vec4(results[4].x,stored.x,stored.z-0.8,1.0);
}
"#;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production submitted actor motion helper"),
        source: wgpu::ShaderSource::Wgsl(crate::shader_source::composed(source, &[]).into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("submitted actor motion fixture"),
        layout: None,
        module: &shader,
        entry_point: Some("regression"),
        compilation_options: Default::default(),
        cache: None,
    });
    let quantization_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("native half-float motion quantization readback"),
        layout: None,
        module: &shader,
        entry_point: Some("read_quantization"),
        compilation_options: Default::default(),
        cache: None,
    });
    let quantized = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("native half-float motion quantization"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let quantized_view = quantized.create_view(&Default::default());
    let transform = [
        [1.0, 0.0, 0.0, 12.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ];
    let instances = [
        ActorMotionInstanceGpu {
            world_from_actor: transform,
            metadata: [0, 1, 1, 0],
        },
        ActorMotionInstanceGpu {
            world_from_actor: transform,
            metadata: [0, 1, 0, 0],
        },
    ];
    let bones = [[
        [1.0_f32, 0.0, 0.0, 3.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]];
    let actor_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("submitted instance fixture"),
        contents: bytemuck::cast_slice(&instances),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let bone_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("submitted interpolated bone fixture"),
        contents: bytemuck::cast_slice(&bones),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let byte_count = 6 * std::mem::size_of::<[f32; 4]>() as u64;
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("submitted motion GPU output"),
        size: byte_count,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("submitted motion GPU readback"),
        size: byte_count,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let result_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("submitted motion results"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 30,
                resource: wgpu::BindingResource::TextureView(&quantized_view),
            },
            wgpu::BindGroupEntry {
                binding: 31,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let quantization_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("native motion quantization input"),
        layout: &quantization_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 29,
                resource: wgpu::BindingResource::TextureView(&quantized_view),
            },
            wgpu::BindGroupEntry {
                binding: 31,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let empty = |index| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("unused submitted motion fixture group"),
            layout: &pipeline.get_bind_group_layout(index),
            entries: &[],
        })
    };
    let group1 = empty(1);
    let group2 = empty(2);
    let pose_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("production submitted actor pose arenas"),
        layout: &pipeline.get_bind_group_layout(3),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: actor_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: bone_buffer.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &result_group, &[]);
        pass.set_bind_group(1, &group1, &[]);
        pass.set_bind_group(2, &group2, &[]);
        pass.set_bind_group(3, &pose_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&quantization_pipeline);
        pass.set_bind_group(0, &quantization_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, byte_count);
    queue.submit([encoder.finish()]);
    let (sender, receiver) = std::sync::mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            sender.send(result).unwrap();
        });
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    receiver.recv().unwrap().unwrap();
    let bytes = readback.slice(..).get_mapped_range();
    let values: &[[f32; 4]] = bytemuck::cast_slice(&bytes);
    assert_eq!(values[0], [16.0, 2.0, 3.0, 1.0]);
    assert_eq!(values[1][3], 0.0);
    assert!((values[2][0] + 0.025).abs() < 0.000001);
    assert_eq!(values[2][2..], [2.0, 1.0]);
    assert_eq!(values[3][3], -1.0);
    assert!(
        (values[4][0] - values[4][1]).abs() * 3840.0 < 0.001,
        "delta encoding preserves subpixel motion"
    );
    assert!(
        (values[4][0] - values[4][2]).abs() * 3840.0 > 0.1,
        "absolute half-float UV is an invalid precision control"
    );
    assert_eq!(values[5][3], -1.0);
}
