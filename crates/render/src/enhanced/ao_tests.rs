//! Exercises visibility against rasterized reverse-Z camera depth.

use super::frame::EnhancedFrameGpu;
use bevy::math::{Mat4, Vec3, Vec4};
use bytemuck::Zeroable as _;
use wgpu::util::DeviceExt as _;

#[test]
fn camera_depth_preserves_open_floor_and_occludes_local_creases_and_contacts() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let Ok(adapter) =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("missing fixture: native GPU adapter for rasterized Enhanced AO/contact");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("Enhanced camera-depth visibility fixture"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("AO fixture device");
    let size = 256u32;
    let camera = Vec3::new(2.4, 2.2, 3.4);
    let near = 0.1;
    let clip = Mat4::perspective_infinite_reverse_rh(60.0_f32.to_radians(), 1.0, near)
        * Mat4::look_at_rh(camera, Vec3::ZERO, Vec3::Y);
    let mut frame = EnhancedFrameGpu::zeroed();
    frame.clip_from_world = clip;
    frame.world_from_clip = clip.inverse();
    frame.camera_time = camera.extend(0.0);
    frame.light_direction = Vec3::new(0.25, 1.0, -0.65).normalize().extend(1.0);
    frame.viewport = Vec4::new(
        size as f32,
        size as f32,
        1.0 / size as f32,
        1.0 / size as f32,
    );
    frame.projection.x = near;
    let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("production Enhanced camera frame"),
        contents: bytemuck::bytes_of(&frame),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let mut positions = Vec::<[f32; 3]>::new();
    let mut quad = |a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]| {
        positions.extend([a, b, c, a, c, d]);
    };
    quad(
        [-4.0, 0.0, -4.0],
        [-4.0, 0.0, 4.0],
        [4.0, 0.0, 4.0],
        [4.0, 0.0, -4.0],
    );
    quad(
        [-0.3, 0.0, 0.3],
        [0.3, 0.0, 0.3],
        [0.3, 0.9, 0.3],
        [-0.3, 0.9, 0.3],
    );
    quad(
        [0.3, 0.0, -0.3],
        [-0.3, 0.0, -0.3],
        [-0.3, 0.9, -0.3],
        [0.3, 0.9, -0.3],
    );
    quad(
        [-0.3, 0.0, -0.3],
        [-0.3, 0.0, 0.3],
        [-0.3, 0.9, 0.3],
        [-0.3, 0.9, -0.3],
    );
    quad(
        [0.3, 0.0, 0.3],
        [0.3, 0.0, -0.3],
        [0.3, 0.9, -0.3],
        [0.3, 0.9, 0.3],
    );
    quad(
        [-0.3, 0.9, 0.3],
        [0.3, 0.9, 0.3],
        [0.3, 0.9, -0.3],
        [-0.3, 0.9, -0.3],
    );
    let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("floor and nearby box triangles"),
        contents: bytemuck::cast_slice(&positions),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let probes = [
        [1.5_f32, 0.0, 1.5, 0.0],
        [0.0, 0.0, 0.36, 0.0],
        [0.0, 0.0, 0.55, 0.0],
        [1.2, 0.0, 0.55, 0.0],
        [0.0, 0.0, 0.50, 0.0],
        [0.0, 0.0, 0.52, 0.0],
        [0.0, 0.0, 0.57, 0.0],
        [0.0, 0.0, 0.59, 0.0],
    ];
    let probe_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("open crease shadow and lit probes"),
        contents: bytemuck::cast_slice(&probes),
        usage: wgpu::BufferUsages::STORAGE,
    });
    let depth = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("rasterized reverse-Z fixture depth"),
        size: wgpu::Extent3d {
            width: size,
            height: size,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let depth_view = depth.create_view(&Default::default());
    let output = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("AO/contact outputs"),
        contents: &[0; 256],
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("AO/contact readback"),
        size: 256,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let source = crate::shader_source::composed(
        r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_ao::{horizon_ao,screen_contact_shadow,ao_world_position,ao_surface_normal}
@group(0) @binding(0) var<uniform> frame:EnhancedFrame;
@group(0) @binding(1) var depth:texture_depth_2d;
@group(0) @binding(2) var<storage,read> probes:array<vec4<f32>,8>;
@group(0) @binding(3) var<storage,read_write> results:array<vec4<f32>,16>;
@vertex fn vertex(@location(0) position:vec3<f32>)->@builtin(position) vec4<f32>{
    return frame.clip_from_world*vec4(position,1.0);
}
@compute @workgroup_size(1) fn evaluate(){
    let size=vec2<f32>(textureDimensions(depth));
    for(var index=0u;index<8u;index+=1u){
        let h=frame.clip_from_world*vec4(probes[index].xyz,1.0);
        let projected=h.xy/h.w*vec2(0.5,-0.5)+vec2(0.5);
        let pixel=clamp(floor(projected*size),vec2(0.0),size-vec2(1.0));
        let uv=(pixel+vec2(0.5))/size;
        let d=textureLoad(depth,vec2<i32>(pixel),0);
        var local_frame=frame;
        var visibility=vec2(0.0);
        var contacts=vec4(0.0);
        for(var rotation=0u;rotation<4u;rotation+=1u){
            local_frame.temporal.x=f32(rotation);
            contacts[rotation]=screen_contact_shadow(local_frame,depth,uv,d,pixel);
            visibility+=vec2(horizon_ao(local_frame,depth,uv,d,pixel),contacts[rotation]);
        }
        let normal=ao_surface_normal(frame,depth,uv,d);
        let world=ao_world_position(frame,uv,d);
        results[index]=vec4(visibility/4.0,dot(normal,vec3(0.0,1.0,0.0)),world.y);
        results[index+8u]=contacts;
    }
}
"#,
        &[],
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production depth AO/contact helpers"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let raster = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("floor/blocker reverse-Z depth raster"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[wgpu::VertexBufferLayout {
                array_stride: 12,
                step_mode: wgpu::VertexStepMode::Vertex,
                attributes: &[wgpu::VertexAttribute {
                    format: wgpu::VertexFormat::Float32x3,
                    offset: 0,
                    shader_location: 0,
                }],
            }],
        },
        primitive: wgpu::PrimitiveState {
            cull_mode: None,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::GreaterEqual,
            stencil: Default::default(),
            bias: Default::default(),
        }),
        multisample: Default::default(),
        fragment: None,
        multiview: None,
        cache: None,
    });
    let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("production floor visibility evaluation"),
        layout: None,
        module: &shader,
        entry_point: Some("evaluate"),
        compilation_options: Default::default(),
        cache: None,
    });
    let raster_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &raster.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }],
    });
    let compute_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &compute.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: probe_buffer.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("floor and box depth fixture"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&raster);
        pass.set_bind_group(0, &raster_group, &[]);
        pass.set_vertex_buffer(0, vertices.slice(..));
        pass.draw(0..positions.len() as u32, 0..1);
    }
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&compute);
        pass.set_bind_group(0, &compute_group, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 256);
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |result| {
        result.expect("AO fixture readback")
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("AO fixture completion");
    let values: Vec<[f32; 4]> =
        bytemuck::cast_slice(&readback.slice(..).get_mapped_range()).to_vec();
    for value in &values[..8] {
        assert!(
            value.iter().all(|v| v.is_finite()),
            "finite visibility: {values:?}"
        );
        assert!(
            value[2] > 0.98 && value[3].abs() < 0.02,
            "probe must reconstruct the floor: {values:?}"
        );
    }
    for contacts in &values[8..] {
        assert!(
            contacts
                .iter()
                .all(|value| (*value - contacts[0]).abs() < 1.0e-6),
            "a static contact ray cannot pulse with the temporal sample index: {contacts:?}"
        );
        assert!(
            contacts.iter().all(|value| *value >= 0.65 && *value <= 1.0),
            "screen depth supplements the stable cascade instead of replacing it: {contacts:?}"
        );
    }
    assert!(
        values[0][0] > 0.95,
        "open floor remains unoccluded: {values:?}"
    );
    assert!(
        values[1][0] < values[0][0] - 0.04,
        "crease receives local AO: {values:?}"
    );
    assert!(
        values[2][1] < 0.85,
        "nearby box casts a contact shadow: {values:?}"
    );
    assert!(
        values[3][1] > 0.95,
        "lit neighbour remains visible: {values:?}"
    );
    for value in &values[4..8] {
        assert!(
            value[1] < 0.85,
            "nearby contact survives receiver offsets between fixed marching samples: {values:?}"
        );
    }
}
