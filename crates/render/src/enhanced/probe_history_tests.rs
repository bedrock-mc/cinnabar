//! Actual half-float blending preserves displayed radiance through replacement captures.

use super::{ReflectionHistory, colour_target, layout};

#[test]
fn reflection_display_blends_continuously_and_resets_without_stale_radiance() {
    let instance = crate::enhanced::validation::native_instance();
    let Ok(adapter) = bevy::tasks::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("missing fixture: native GPU adapter for continuous reflection resolve");
        return;
    };
    let (device, queue) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("reflection display history regression"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("reflection history fixture device");
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("replacement reflection radiance"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let displayed = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("persistent displayed reflection radiance"),
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_DST,
        ..texture_descriptor()
    });
    let source = texture.create_view(&Default::default());
    let destination = displayed.create_view(&Default::default());
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        min_filter: wgpu::FilterMode::Linear,
        mag_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    });
    let descriptor = layout();
    let bind_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(descriptor.label.as_ref()),
        entries: &descriptor.entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("production reflection resolve layout"),
        bind_group_layouts: &[&bind_layout],
        push_constant_ranges: &[],
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("cached reflection target fixture"),
        layout: &bind_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&source),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&sampler),
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("production reflection resolve"),
        source: wgpu::ShaderSource::Wgsl(
            crate::shader_source::composed(include_str!("probe.wgsl"), &[]).into(),
        ),
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("production continuous reflection blend"),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("fullscreen"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("resolve_reflections"),
            compilation_options: Default::default(),
            targets: &[Some(colour_target())],
        }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview: None,
        cache: None,
    });
    let sample_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("reflection history readback"),
        source: wgpu::ShaderSource::Wgsl(
            r#"
@group(0) @binding(0) var displayed:texture_2d<f32>;
@group(0) @binding(1) var<storage,read_write> outputs:array<vec4<f32>,128>;
@group(0) @binding(2) var<uniform> sample_index:vec4<u32>;
@compute @workgroup_size(1) fn sample_display(){
    outputs[sample_index.x]=textureLoad(displayed,vec2(0),0);
}
"#
            .into(),
        ),
    });
    let sample_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("reflection display sampling"),
        layout: None,
        module: &sample_shader,
        entry_point: Some("sample_display"),
        compilation_options: Default::default(),
        cache: None,
    });
    let output = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("reflection history samples"),
        size: 128 * 16,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("reflection history readback"),
        size: output.size(),
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let parameters = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("reflection sample destination"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let sample_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("reflection sampling fixture"),
        layout: &sample_pipeline.get_bind_group_layout(0),
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&destination),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: output.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: parameters.as_entire_binding(),
            },
        ],
    });
    let encode = |index: u32, target: f32, weight: Option<f32>| {
        queue.write_buffer(&parameters, 0, bytemuck::bytes_of(&[index, 0, 0, 0]));
        let mut encoder = device.create_command_encoder(&Default::default());
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &source,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: f64::from(target),
                            g: f64::from(target) * 0.5,
                            b: f64::from(target) * 0.25,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
        }
        if let Some(weight) = weight {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &destination,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Load,
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &group, &[]);
            let weight = f64::from(weight);
            pass.set_blend_constant(wgpu::Color {
                r: weight,
                g: weight,
                b: weight,
                a: weight,
            });
            pass.draw(0..3, 0..1);
        } else {
            encoder.copy_texture_to_texture(
                texture.as_image_copy(),
                displayed.as_image_copy(),
                wgpu::Extent3d {
                    width: 1,
                    height: 1,
                    depth_or_array_layers: 1,
                },
            );
        }
        {
            let mut pass = encoder.begin_compute_pass(&Default::default());
            pass.set_pipeline(&sample_pipeline);
            pass.set_bind_group(0, &sample_group, &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }
        queue.submit([encoder.finish()]);
    };
    let mut expected = Vec::new();
    let mut history = ReflectionHistory::default();
    assert!(history.target_changed(0));
    assert!(history.weights(0.0)[0].is_none());
    encode(0, 0.0, None);
    expected.push((0.0, None));
    assert!(!history.target_changed(0));
    let mut current = 0.0;
    let mut target = 1.0;
    for step in 1..=60 {
        if step == 8 || step == 12 {
            target = if step == 8 { 0.0 } else { 0.75 };
            assert!(!history.target_changed(0));
        }
        if let Some(weight) = history.weights(step as f32 * 0.01)[0] {
            current += (target - current) * weight;
            encode(expected.len() as u32, target, Some(weight));
            expected.push((target, Some(weight)));
        }
    }
    assert_eq!(current, target);
    assert!(history.weights(0.7).iter().all(Option::is_none));
    history.reset();
    assert!(history.target_changed(0));
    encode(expected.len() as u32, 0.25, None);
    let reset_index = expected.len();
    expected.push((0.25, None));
    history.reset();
    assert!(history.target_changed(0));
    history.weights(0.0);
    let alternating_start = expected.len();
    encode(expected.len() as u32, 0.0, None);
    expected.push((0.0, None));
    assert!(!history.target_changed(0));
    target = 1.0;
    for step in 1..=24 {
        if step % 2 == 0 {
            target = 1.0 - target;
            assert!(!history.target_changed(0));
        }
        let weight = history.weights(step as f32 * 0.01)[0].unwrap();
        encode(expected.len() as u32, target, Some(weight));
        expected.push((target, Some(weight)));
    }
    let alternating_end = expected.len();
    let mut frequency_indices = Vec::new();
    for rate in [30, 60, 120] {
        history.reset();
        assert!(history.target_changed(0));
        history.weights(0.0);
        encode(expected.len() as u32, 0.0, None);
        expected.push((0.0, None));
        assert!(!history.target_changed(0));
        for step in 1..=rate / 5 {
            let weight = history.weights(step as f32 / rate as f32)[0].unwrap();
            encode(expected.len() as u32, 1.0, Some(weight));
            expected.push((1.0, Some(weight)));
        }
        frequency_indices.push(expected.len() - 1);
    }
    let mut encoder = device.create_command_encoder(&Default::default());
    encoder.copy_buffer_to_buffer(&output, 0, &readback, 0, output.size());
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |result| {
        result.expect("reflection history readback");
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .expect("reflection history completion");
    let samples: Vec<[f32; 4]> =
        bytemuck::cast_slice(&readback.slice(..).get_mapped_range()).to_vec();
    let mut ideals = Vec::with_capacity(expected.len());
    let mut write_bounds = Vec::with_capacity(expected.len());
    let mut ideal = 0.0_f64;
    let mut accumulated_write_bound = 0.0_f64;
    for (index, (target, weight)) in expected.into_iter().enumerate() {
        let sources = [target, target * 0.5, target * 0.25, 1.0];
        for (channel, source) in sources.into_iter().enumerate() {
            let actual = samples[index][channel];
            assert!(actual.is_finite() && (0.0..=1.0).contains(&actual));
            if let Some(weight) = weight {
                let previous = f64::from(samples[index - 1][channel]);
                let next = previous + (f64::from(source) - previous) * f64::from(weight);
                let ulp = half_float_ulp((next as f32).max(actual));
                assert!(
                    (f64::from(actual) - next).abs() <= ulp,
                    "each resolve continues from the stored radiance within one RGBA16F ULP: sample {index}, channel {channel}: {actual} vs {next}, ULP {ulp}"
                );
            } else {
                assert_eq!(
                    actual, source,
                    "an immediate capture copies exact fixture radiance"
                );
            }
        }
        if let Some(weight) = weight {
            let retained = 1.0 - f64::from(weight);
            let previous = f64::from(samples[index - 1][0]);
            let next_stored = previous + (f64::from(target) - previous) * f64::from(weight);
            let write_ulp = half_float_ulp((next_stored as f32).max(samples[index][0]));
            ideal += (f64::from(target) - ideal) * f64::from(weight);
            accumulated_write_bound = accumulated_write_bound * retained + write_ulp;
        } else {
            ideal = f64::from(target);
            accumulated_write_bound = 0.0;
        }
        assert!(
            (f64::from(samples[index][0]) - ideal).abs() <= accumulated_write_bound,
            "the continuous response stays within its accumulated half-float write budget: {index}: {} vs {ideal}, bound {accumulated_write_bound}",
            samples[index][0]
        );
        ideals.push(ideal);
        write_bounds.push(accumulated_write_bound);
    }
    assert_eq!(samples[reset_index][0], 0.25);
    for pair in samples[alternating_start..alternating_end].windows(2) {
        assert!(
            (pair[1][0] - pair[0][0]).abs() < 0.155,
            "rapid replacement remains bounded instead of flashing to the raw target: {pair:?}"
        );
    }
    let reference_index = frequency_indices[0];
    let reference = f64::from(samples[reference_index][0]);
    for index in frequency_indices {
        let budget = write_bounds[index]
            + write_bounds[reference_index]
            + (ideals[index] - ideals[reference_index]).abs();
        assert!(
            (f64::from(samples[index][0]) - reference).abs() <= budget,
            "frame-rate endpoints differ only within the accumulated storage precision: {index}: {} vs {reference}, bound {budget}",
            samples[index][0]
        );
    }
}

fn half_float_ulp(value: f32) -> f64 {
    let exponent = ((value.max(2.0_f32.powi(-14)).to_bits() >> 23) & 0xff) as i32 - 127;
    2.0_f64.powi(exponent - 10)
}

fn texture_descriptor() -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::empty(),
        view_formats: &[],
    }
}
