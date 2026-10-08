//! Native depth fixture for the production cascade projection and receiver filter.

use bevy::math::{UVec4, Vec3, Vec4};
use wgpu::util::DeviceExt;

use super::frame::{
    EnhancedFrameGpu, FEATURE_SHADOWS, SHADOW_FILTER_MARGIN_TEXELS, fit_cascade,
    fit_cascade_with_border,
};

const CASTER: &str = r#"
#import cinnabar::enhanced_common::EnhancedFrame
@group(0) @binding(0) var<uniform> frame: EnhancedFrame;
@vertex fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let vertices = array<vec3<f32>, 12>(
        vec3(-8.0, 0.0, -18.0), vec3(8.0, 0.0, -18.0), vec3(-8.0, 0.0, 0.0),
        vec3(-8.0, 0.0, 0.0), vec3(8.0, 0.0, -18.0), vec3(8.0, 0.0, 0.0),
        vec3(-2.0, 0.18, -10.0), vec3(2.0, 0.18, -10.0), vec3(-2.0, 0.18, -6.0),
        vec3(-2.0, 0.18, -6.0), vec3(2.0, 0.18, -10.0), vec3(2.0, 0.18, -6.0),
    );
    return frame.cascade_clip_from_world[0] * vec4(vertices[index], 1.0);
}
@fragment fn fragment() {}
"#;

const RECEIVER: &str = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_shadow::sun_shadow_sample
@group(0) @binding(0) var<uniform> frame: EnhancedFrame;
@group(0) @binding(1) var shadow_map: texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
@group(0) @binding(3) var<storage, read_write> results: array<vec4<f32>, 3>;
@compute @workgroup_size(1)
fn sample_receiver(@builtin(global_invocation_id) id: vec3<u32>) {
    let world = vec3(-6.0 + f32(id.x) * 6.0, 0.0, -8.0);
    let result = sun_shadow_sample(frame, shadow_map, shadow_sampler,
        world, vec3(0.0, 1.0, 0.0), vec2(f32(id.x) * 32.0 + 16.0, 32.0));
    let clip = frame.cascade_clip_from_world[0] * vec4(world, 1.0);
    let uv = vec2(clip.x * 0.5 + 0.5, 0.5 - clip.y * 0.5);
    let size = textureDimensions(shadow_map);
    let depth = textureLoad(shadow_map, vec2<i32>(uv * vec2<f32>(size)), 0, 0);
    results[id.x] = vec4(result, clip.z, depth);
}
"#;

const TILTED_PLANE: &str = r#"
const TILTED_SLOPE:vec2<f32>=vec2(0.8,0.45);
fn tilted_position(p:vec2<f32>)->vec3<f32>{return vec3(p.x,dot(TILTED_SLOPE,p),p.y);}
fn tilted_normal()->vec3<f32>{return normalize(vec3(-TILTED_SLOPE.x,1.0,-TILTED_SLOPE.y));}
"#;

fn sample_fixture(
    caster_source: &str,
    receiver_source: &str,
    result_count: u32,
    vertex_count: u32,
    configured: Option<(EnhancedFrameGpu, [f32; 2])>,
) -> Option<Vec<[f32; 4]>> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..Default::default()
    });
    let Ok(adapter) =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("missing fixture: native GPU adapter for Enhanced shadow depth regression");
        return None;
    };
    let (device, queue) =
        bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default()))
            .expect("Enhanced shadow fixture device");
    let resolution = configured.map_or(256, |(frame, _)| frame.flags.z);
    let fit = fit_cascade(Vec3::ZERO, 64.0, Vec3::Y, resolution);
    let mut frame: EnhancedFrameGpu = bytemuck::Zeroable::zeroed();
    frame.cascade_clip_from_world[0] = fit.clip_from_world;
    frame.cascade_texel = Vec4::new(fit.texel_world, 0.0, 0.0, 128.0);
    frame.cascade_receiver_radius = Vec4::new(
        64.0,
        0.0,
        0.0,
        fit.texel_world * SHADOW_FILTER_MARGIN_TEXELS,
    );
    frame.cascade_depth_scale = Vec4::new(
        1.0 / (fit.bounds.max.z - fit.bounds.min.z),
        0.0,
        0.0,
        SHADOW_FILTER_MARGIN_TEXELS,
    );
    frame.light_direction = Vec3::Y.extend(1.0);
    frame.flags = UVec4::new(FEATURE_SHADOWS, 1, resolution, 0);
    if let Some((custom, _)) = configured {
        frame = custom;
    }
    let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("shadow fixture production frame"),
        contents: bytemuck::bytes_of(&frame),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shadow fixture depth"),
        size: wgpu::Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: frame.flags.y,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Depth32Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let layers: Vec<_> = (0..frame.flags.y)
        .map(|index| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: index,
                array_layer_count: Some(1),
                ..Default::default()
            })
        })
        .collect();
    let sampled = texture.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        compare: Some(wgpu::CompareFunction::LessEqual),
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let shader = |source: &str| {
        device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("shadow fixture production sampling"),
            source: wgpu::ShaderSource::Wgsl(
                crate::shader_source::composed(&format!("{TILTED_PLANE}\n{source}"), &[]).into(),
            ),
        })
    };
    let caster = shader(caster_source);
    let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shadow fixture caster"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &caster,
            entry_point: Some("vertex"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &caster,
            entry_point: Some("fragment"),
            compilation_options: Default::default(),
            targets: &[],
        }),
        primitive: Default::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: wgpu::TextureFormat::Depth32Float,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: Default::default(),
            bias: super::shadows::directional_shadow_raster_bias(),
        }),
        multisample: Default::default(),
        multiview: None,
        cache: None,
    });
    let render_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &render_pipeline.get_bind_group_layout(0),
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: uniforms.as_entire_binding(),
        }],
    });
    let receiver = shader(receiver_source);
    let compute = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("shadow fixture receivers"),
        layout: None,
        module: &receiver,
        entry_point: Some("sample_receiver"),
        compilation_options: Default::default(),
        cache: None,
    });
    let result_size = (result_count as usize * std::mem::size_of::<[f32; 4]>()) as u64;
    let results = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result_size,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result_size,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let compute_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout: &compute.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: uniforms.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&sampled),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: results.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    for (index, layer) in layers.iter().enumerate() {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("shadow fixture depth draw"),
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: layer,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(configured.map_or(1.0, |(_, depths)| depths[index])),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&render_pipeline);
        pass.set_bind_group(0, &render_group, &[]);
        pass.draw(0..vertex_count, 0..1);
    }
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&compute);
        pass.set_bind_group(0, &compute_group, &[]);
        pass.dispatch_workgroups(result_count, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&results, 0, &readback, 0, result_size);
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
    Some(values.to_vec())
}

#[test]
fn native_cascade_shadows_keep_thin_contacts_and_lit_neighbors() {
    let Some(values) = sample_fixture(CASTER, RECEIVER, 3, 12, None) else {
        return;
    };
    assert!(
        values[1][0] < 0.2,
        "thin ledge must cast a contact shadow: {values:?}"
    );
    assert!(
        values[0][0] > 0.95 && values[2][0] > 0.95,
        "adjacent ground must stay lit: {values:?}"
    );
    for value in &values {
        assert_eq!(value[1], 0.0, "receiver must use the fitted near cascade");
        assert!((0.0..1.0).contains(&value[2]) && (0.0..1.0).contains(&value[3]));
    }
    assert!(
        values[1][3] < values[0][3],
        "closer-to-light occluder must write a smaller conventional depth"
    );
}

const TILTED_CASTER: &str = r#"
#import cinnabar::enhanced_common::EnhancedFrame
@group(0) @binding(0) var<uniform> frame:EnhancedFrame;
@vertex fn vertex(@builtin(vertex_index) index:u32)->@builtin(position) vec4<f32>{
    let points=array<vec2<f32>,6>(
        vec2(-12.0,-26.0),vec2(12.0,-26.0),vec2(-12.0,4.0),
        vec2(-12.0,4.0),vec2(12.0,-26.0),vec2(12.0,4.0));
    let p=points[index];
    return frame.cascade_clip_from_world[0]*vec4(tilted_position(p),1.0);
}
@fragment fn fragment(){}
"#;
const TILTED_RECEIVER: &str = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_shadow::sun_shadow_sample
@group(0) @binding(0) var<uniform> frame:EnhancedFrame;
@group(0) @binding(1) var shadow_map:texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler:sampler_comparison;
@group(0) @binding(3) var<storage,read_write> results:array<vec4<f32>,16>;
@compute @workgroup_size(1) fn sample_receiver(@builtin(global_invocation_id) id:vec3<u32>){
    let p=vec2(-3.6+f32(id.x%4u)*2.3,-13.1+f32(id.x/4u)*2.3);
    let world=tilted_position(p);
    let normal=tilted_normal();
    let sample=sun_shadow_sample(frame,shadow_map,shadow_sampler,world,normal,
        vec2(f32(id.x)*17.0+5.0,39.0));
    let clip=frame.cascade_clip_from_world[0]*vec4(world,1.0);
    let uv=clip.xy*vec2(0.5,-0.5)+vec2(0.5);
    let stored=textureLoad(shadow_map,vec2<i32>(uv*vec2<f32>(textureDimensions(shadow_map))),0,0);
    results[id.x]=vec4(sample,clip.z,stored);
}
"#;
#[test]
fn tilted_planar_receivers_stay_lit_across_filter_taps() {
    let Some(values) = sample_fixture(TILTED_CASTER, TILTED_RECEIVER, 16, 6, None) else {
        return;
    };
    for (point, value) in values.iter().enumerate() {
        assert!(
            value[0] > 0.95,
            "a plane cannot occlude itself at point {point}: {values:?}"
        );
        assert_eq!(
            value[1], 0.0,
            "the actual fitted cascade must receive the plane"
        );
        assert!(value[2].is_finite() && value[3].is_finite());
        assert!((0.0..1.0).contains(&value[2]) && (0.0..1.0).contains(&value[3]));
    }
}

const STABLE_RECEIVER: &str = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_shadow::sun_shadow_sample
@group(0) @binding(0) var<uniform> frame:EnhancedFrame;
@group(0) @binding(1) var shadow_map:texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler:sampler_comparison;
@group(0) @binding(3) var<storage,read_write> results:array<vec4<f32>,16>;
@compute @workgroup_size(1) fn sample_receiver(@builtin(global_invocation_id) id:vec3<u32>){
    let world=vec3(2.0,0.0,-8.0);
    let result=sun_shadow_sample(frame,shadow_map,shadow_sampler,world,vec3(0.0,1.0,0.0),vec2(f32(id.x)*37.13,25.91+f32(id.x)*3.76));
    results[id.x]=vec4(result,0.0,0.0);
}
"#;

#[test]
fn shadow_penumbra_is_stable_when_receiver_moves_across_screen_pixels() {
    let Some(values) = sample_fixture(CASTER, STABLE_RECEIVER, 16, 12, None) else {
        return;
    };
    let first = values[0][0];
    assert!(
        first > 0.01 && first < 0.99,
        "the regression samples the actual soft edge: {values:?}"
    );
    assert!(
        values.iter().all(|v| (v[0] - first).abs() < 0.000001),
        "camera reprojection cannot rotate the receiver's filter: {values:?}"
    );
}

const TRANSITION_RECEIVER: &str = r#"
#import cinnabar::enhanced_common::EnhancedFrame
#import cinnabar::enhanced_shadow::{sun_shadow_sample,shadow_receiver_radius}
@group(0) @binding(0) var<uniform> frame:EnhancedFrame;
@group(0) @binding(1) var shadow_map:texture_depth_2d_array;
@group(0) @binding(2) var shadow_sampler:sampler_comparison;
@group(0) @binding(3) var<storage,read_write> results:array<vec4<f32>,31>;
@compute @workgroup_size(1) fn sample_receiver(@builtin(global_invocation_id) id:vec3<u32>){
    var translated=frame;
    translated.camera_time.x=-0.6+f32(id.x)*0.04;
    let world=vec3(16.0,0.0,0.0);
    let sample=sun_shadow_sample(translated,shadow_map,shadow_sampler,world,vec3(0.0,1.0,0.0),vec2(32.0));
    results[id.x]=vec4(sample,distance(world,translated.camera_time.xyz),shadow_receiver_radius(translated,0u));
}
"#;

#[test]
fn translating_across_radial_cascades_blends_visibility_before_the_handoff() {
    let resolution = 256;
    let far = fit_cascade(Vec3::ZERO, 48.0, Vec3::Y, resolution);
    let border = far.texel_world * SHADOW_FILTER_MARGIN_TEXELS;
    let near = fit_cascade_with_border(Vec3::ZERO, 16.0, Vec3::Y, resolution, border);
    let mut frame: EnhancedFrameGpu = bytemuck::Zeroable::zeroed();
    frame.cascade_clip_from_world[0] = near.clip_from_world;
    frame.cascade_clip_from_world[1] = far.clip_from_world;
    frame.cascade_texel = Vec4::new(near.texel_world, far.texel_world, 0.0, 80.0);
    frame.cascade_receiver_radius = Vec4::new(16.0, 48.0, 0.0, border);
    frame.cascade_depth_scale = Vec4::new(
        1.0 / (near.bounds.max.z - near.bounds.min.z),
        1.0 / (far.bounds.max.z - far.bounds.min.z),
        0.0,
        SHADOW_FILTER_MARGIN_TEXELS,
    );
    frame.light_direction = Vec3::Y.extend(1.0);
    frame.flags = UVec4::new(FEATURE_SHADOWS, 2, resolution, 0);
    let Some(values) = sample_fixture(
        CASTER,
        TRANSITION_RECEIVER,
        31,
        0,
        Some((frame, [0.0, 1.0])),
    ) else {
        return;
    };
    assert!(
        values[0][0] > 0.995 && values[30][0] < 0.9,
        "the world receiver must cross a real shadow/lit transition: {values:?}"
    );
    assert_eq!(values[0][1], 1.0);
    assert_eq!(values[30][1], 0.0);
    for pair in values.windows(2) {
        assert!(
            pair[1][0] <= pair[0][0] + 1.0e-5,
            "radial overlap must be monotonic"
        );
        assert!(
            (pair[1][0] - pair[0][0]).abs() < 0.025,
            "translation cannot abruptly replace a cascade: {pair:?}"
        );
        assert!(
            (pair[0][3] - 16.0).abs() < 1.0e-4,
            "the GPU must reconstruct the CPU receiver coverage"
        );
    }
}
