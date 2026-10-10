//! Executes history reconstruction against a real sampled texture on the native adapter.

use wgpu::util::DeviceExt as _;

#[test]
fn cubic_history_retains_subpixel_detail_and_rejects_foreground_background_mixing() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!(
            "missing fixture: native GPU adapter for Enhanced temporal history reconstruction"
        );
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("temporal history reconstruction regression device"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("temporal history reconstruction device");
    let source = r#"
#import cinnabar::enhanced_temporal::temporal_history_sample
@group(0) @binding(0) var history:texture_2d<f32>;
@group(0) @binding(1) var linear:sampler;
@group(0) @binding(2) var discontinuous:texture_2d<f32>;
@group(0) @binding(31) var<storage,read_write> results:array<vec4<f32>,5>;
@compute @workgroup_size(1) fn regression(){
    let uv=vec2((1.75+0.5)/8.0,0.5);
    results[0]=temporal_history_sample(history,linear,uv,1.0);
    results[1]=textureSampleLevel(history,linear,uv,0.0);
    results[2]=temporal_history_sample(discontinuous,linear,uv,1.0);
    results[3]=temporal_history_sample(history,linear,vec2(0.5),0.0);
    results[4]=temporal_history_sample(discontinuous,linear,vec2((4.25+0.5)/8.0,0.5),1.0);
}
"#;
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production cubic history reconstruction"),
        source: wgpu::ShaderSource::Wgsl(crate::shader_source::composed(source, &[]).into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("temporal history reconstruction fixture"),
        layout: None,
        module: &shader,
        entry_point: Some("regression"),
        compilation_options: Default::default(),
        cache: None,
    });
    let extent = wgpu::Extent3d {
        width: 8,
        height: 1,
        depth_or_array_layers: 1,
    };
    let descriptor = wgpu::TextureDescriptor {
        label: Some("subpixel edge history fixture"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    };
    let history = device.create_texture(&descriptor);
    let discontinuous = device.create_texture(&descriptor);
    let mut texels = [[0u8, 0, 0, 255]; 8];
    for texel in &mut texels[2..6] {
        *texel = [255; 4];
    }
    let layout = wgpu::TexelCopyBufferLayout {
        offset: 0,
        bytes_per_row: Some(32),
        rows_per_image: Some(1),
    };
    queue.write_texture(
        history.as_image_copy(),
        bytemuck::cast_slice(&texels),
        layout,
        extent,
    );
    for texel in &mut texels[..2] {
        texel[3] = 0;
    }
    queue.write_texture(
        discontinuous.as_image_copy(),
        bytemuck::cast_slice(&texels),
        layout,
        extent,
    );
    let history_view = history.create_view(&Default::default());
    let discontinuous_view = discontinuous.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        ..Default::default()
    });
    let byte_count = 5 * std::mem::size_of::<[f32; 4]>() as u64;
    let output = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("temporal reconstruction output"),
        contents: &vec![0; byte_count as usize],
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("temporal reconstruction readback"),
        size: byte_count,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("production cubic history texture inputs"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&history_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&discontinuous_view),
            },
            wgpu::BindGroupEntry {
                binding: 31,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &group, &[]);
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
    assert!(
        (values[0][0] - 0.796875).abs() < 0.0003,
        "cubic edge must keep subpixel contrast: {:?}",
        values[0]
    );
    assert!((values[1][0] - 0.75).abs() < 0.0001);
    assert!(
        values[0][0] > values[1][0] + 0.04,
        "history resampling must recover more detail than repeated bilinear filtering"
    );
    assert_eq!(values[0][3], 1.0);
    assert_eq!(
        values[2][3], 0.0,
        "mixed foreground/background history must reset"
    );
    assert_eq!(
        values[3][3], 0.0,
        "opaque surface history must not contaminate the sky"
    );
    assert_eq!(
        values[4][3], 1.0,
        "valid stable interior history stays usable"
    );
}
