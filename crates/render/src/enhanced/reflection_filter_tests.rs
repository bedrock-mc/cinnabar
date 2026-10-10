//! Native texture fixtures exercise the production GGX and cosine convolution.

use wgpu::util::DeviceExt as _;

#[test]
fn roughness_broadens_environment_lobes_and_diffuse_preserves_constant_radiance() {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor::default());
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU adapter for environment convolution");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("environment convolution regression"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("environment convolution device");
    let mut source = include_str!("probe_filter.wgsl").to_owned();
    source.push_str(
        r#"
@group(0) @binding(31) var<storage,read_write> result:array<vec4<f32>,1>;
@compute @workgroup_size(1) fn regression() {
    result[0]=filtered_environment(vec2(0.5));
}
"#,
    );
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production environment convolution"),
        source: wgpu::ShaderSource::Wgsl(crate::shader_source::composed(&source, &[]).into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("environment convolution fixture"),
        layout: None,
        module: &shader,
        entry_point: Some("regression"),
        compilation_options: Default::default(),
        cache: None,
    });
    let texture = |layers| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some("environment radiance fixture"),
            size: wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        })
    };
    let cube = texture(6);
    let cube_view = cube.create_view(&wgpu::TextureViewDescriptor {
        dimension: Some(wgpu::TextureViewDimension::D2Array),
        ..Default::default()
    });
    let sky = texture(1);
    let sky_view = sky.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("environment convolution fixture parameters"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let output = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("environment convolution outputs"),
        contents: &[0; 16],
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("environment convolution readback"),
        size: 16,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("environment convolution fixture bindings"),
        layout: &pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&cube_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&sky_view),
            },
            wgpu::BindGroupEntry {
                binding: 31,
                resource: output.as_entire_binding(),
            },
        ],
    });
    let write_texture = |target: &wgpu::Texture, texels: &[[u8; 4]]| {
        queue.write_texture(
            target.as_image_copy(),
            bytemuck::cast_slice(texels),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4),
                rows_per_image: Some(1),
            },
            wgpu::Extent3d {
                width: 1,
                height: 1,
                depth_or_array_layers: texels.len() as u32,
            },
        );
    };
    let evaluate = |roughness: f32, mode: f32| {
        queue.write_buffer(
            &uniform,
            0,
            bytemuck::bytes_of(&[0.0, roughness, mode, 64.0]),
        );
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, 16);
        queue.submit([encoder.finish()]);
        let (sender, receiver) = std::sync::mpsc::channel();
        readback
            .slice(..)
            .map_async(wgpu::MapMode::Read, move |result| {
                sender.send(result).unwrap();
            });
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        receiver.recv().unwrap().unwrap();
        let value = *bytemuck::from_bytes::<[f32; 4]>(&readback.slice(..).get_mapped_range());
        readback.unmap();
        value
    };
    let uniform_radiance = [64, 96, 160, 255];
    write_texture(&cube, &[uniform_radiance; 6]);
    write_texture(&sky, &[uniform_radiance]);
    for (roughness, mode) in [(0.02, 0.0), (1.0, 0.0), (1.0, 1.0), (1.0, 2.0), (1.0, 3.0)] {
        let actual = evaluate(roughness, mode);
        for channel in 0..4 {
            assert!(
                (actual[channel] - f32::from(uniform_radiance[channel]) / 255.0).abs() < 0.00001,
                "convolution must preserve constant radiance: {actual:?}"
            );
        }
    }
    let mut directional = [[0, 0, 0, 255]; 6];
    directional[2] = [255; 4];
    write_texture(&cube, &directional);
    let smooth = evaluate(0.02, 0.0);
    let rough = evaluate(1.0, 0.0);
    let diffuse = evaluate(1.0, 1.0);
    assert!(
        smooth[0] < 0.00001,
        "a smooth +X reflection must not include the +Y face"
    );
    assert!(
        rough[0] > 0.01 && rough[0] < 0.8,
        "rough GGX must include adjacent directions: {rough:?}"
    );
    assert!(
        diffuse[0] > 0.01 && diffuse[0] < 0.8,
        "cosine irradiance includes a lit adjacent hemisphere"
    );
    directional[2][3] = 0;
    write_texture(&cube, &directional);
    let incomplete = evaluate(1.0, 1.0);
    assert!(
        incomplete[0] < 0.00001 && incomplete[3] < 0.999,
        "an unpopulated face remains uncertain and contributes no radiance"
    );
}
