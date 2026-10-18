//! Validate every Enhanced shader and create its pipeline on an available real adapter.

use crate::shader_source;
type Variant = (&'static str, String, &'static str, &'static str, bool);

/// All forward, shadow-caster and fullscreen variants used by the extension.
fn variants() -> Vec<Variant> {
    let mut result = Vec::new();
    for (name, source) in [
        ("chunk", include_str!("../chunk.wesl")),
        ("model", include_str!("../model.wesl")),
        ("liquid", include_str!("../liquid.wesl")),
        (
            "transparent_terrain",
            include_str!("../transparent_terrain.wesl"),
        ),
    ] {
        result.push((
            name,
            shader_source::composed(source, &["ENHANCED"]),
            "vertex",
            "fragment",
            false,
        ));
        if name == "model" {
            result.push((
                name,
                shader_source::composed(source, &["ENHANCED"]),
                "vertex",
                "fragment_blend",
                false,
            ));
        }
        if name == "liquid" {
            result.push((
                name,
                shader_source::composed(source, &["ENHANCED"]),
                "vertex_depth",
                "fragment_depth",
                false,
            ));
        }
        if source.contains("@if(ENHANCED_SHADOW)") {
            result.push((
                name,
                shader_source::composed(source, &["ENHANCED_SHADOW"]),
                "vertex",
                "fragment_shadow",
                true,
            ));
        }
    }
    for fragment in ["light_shafts", "composite"] {
        result.push((
            fragment,
            shader_source::composed(include_str!("../enhanced/post.wesl"), &[]),
            "fullscreen",
            fragment,
            false,
        ));
    }
    result
}

#[test]
fn enhanced_shaders_validate() {
    for (name, source, _, fragment, _) in variants() {
        let module = naga::front::wgsl::parse_str(&source)
            .unwrap_or_else(|error| panic!("{name}/{fragment}: {}", error.emit_to_string(&source)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap_or_else(|error| panic!("{name}/{fragment}: {error:?}"));
    }
}

#[test]
#[ignore = "Enhanced disabled after GPU faults and system freezes"]
fn enhanced_pipelines_build_on_native_adapter() {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .expect("this fixture requires a native GPU adapter");
    let (device, _) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("enhanced smoke"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("Enhanced smoke device");
    for (name, source, vertex, fragment, shadow) in variants() {
        let descriptors = if vertex == "fullscreen" {
            vec![super::gpu::enhanced_post_layout()]
        } else {
            vec![
                crate::chunk::pipeline::layouts::chunk_bind_group_layout(),
                crate::lighting::layout(),
                if shadow {
                    super::gpu::enhanced_caster_layout()
                } else {
                    super::gpu::enhanced_view_layout()
                },
            ]
        };
        let validation_scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let groups: Vec<_> = descriptors
            .iter()
            .map(|descriptor| {
                device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                    label: Some(descriptor.label.as_ref()),
                    entries: &descriptor.entries,
                })
            })
            .collect();
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("production Enhanced layout"),
            bind_group_layouts: &groups.iter().map(Some).collect::<Vec<_>>(),
            immediate_size: 0,
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let targets = [Some(wgpu::ColorTargetState {
            format: wgpu::TextureFormat::Rgba16Float,
            blend: None,
            write_mask: wgpu::ColorWrites::ALL,
        })];
        let _pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some(name),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some(vertex),
                compilation_options: Default::default(),
                buffers: &[],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some(fragment),
                compilation_options: Default::default(),
                targets: if shadow { &[] } else { &targets },
            }),
            primitive: Default::default(),
            depth_stencil: shadow.then_some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::LessEqual),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let error = bevy::tasks::block_on(validation_scope.pop());
        assert!(error.is_none(), "{name}/{fragment}: {error:?}");
    }
}

// Night and brightness darken the lightmap, never the open-sky gate on direct moonlight.
#[test]
fn full_sky_exposure_keeps_direct_light_under_a_night_lightmap() {
    let Some(gpu) = crate::gpu_snapshot::Gpu::for_fixture("Enhanced sky exposure") else {
        return;
    };
    let (device, queue) = (gpu.device, gpu.queue);
    let samples = [
        240_u32,
        0,
        u32::from(meshing::lighting::JAVA_SAMPLE_FLAG) | (60 << 6),
        u32::from(meshing::lighting::JAVA_SAMPLE_FLAG),
        u32::from(meshing::lighting::JAVA_SAMPLE_FLAG) | (29 << 6) | 60,
    ];
    let sample_count = samples.len();
    let result_count = sample_count + 1;
    let result_bytes = result_count * std::mem::size_of::<f32>();
    let sample_words = samples.map(|sample| format!("{sample}u")).join(", ");
    let source = shader_source::composed(
        &format!(
            "import render::enhanced::view::sky_illumination;
import render::lighting::light_colour;
@group(0) @binding(0) var<storage, read_write> results: array<f32, {result_count}>;
@compute @workgroup_size(1) fn main() {{
    let samples = array({sample_words});
    for (var i = 0u; i < {sample_count}u; i += 1u) {{
        results[i] = sky_illumination(samples[i]);
    }}
    results[{sample_count}] = light_colour(240u).r;
}}"
        ),
        &["ENHANCED"],
    );
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sky exposure"),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: None,
        layout: None,
        module: &module,
        entry_point: Some("main"),
        compilation_options: Default::default(),
        cache: None,
    });
    let buffer = |usage, contents: &[u8]| {
        wgpu::util::DeviceExt::create_buffer_init(
            &device,
            &wgpu::util::BufferInitDescriptor {
                label: None,
                contents,
                usage,
            },
        )
    };
    // Midnight at brightness zero: open sky is about 0.129 in every lightmap channel.
    let night = [[0.129_f32, 0.129, 0.129, 1.0]; 256];
    let lightmap = buffer(
        wgpu::BufferUsages::UNIFORM,
        bytemuck::bytes_of(&render_model::LightmapUniform::new(night)),
    );
    let results = buffer(
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        &vec![0; result_bytes],
    );
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: result_bytes as u64,
        usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = |index, resource: &wgpu::Buffer| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &pipeline.get_bind_group_layout(index),
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: resource.as_entire_binding(),
            }],
        })
    };
    let (outputs, lights) = (group(0, &results), group(1, &lightmap));
    let mut encoder = device.create_command_encoder(&Default::default());
    {
        let mut pass = encoder.begin_compute_pass(&Default::default());
        pass.set_pipeline(&pipeline);
        pass.set_bind_group(0, &outputs, &[]);
        pass.set_bind_group(1, &lights, &[]);
        pass.dispatch_workgroups(1, 1, 1);
    }
    encoder.copy_buffer_to_buffer(&results, 0, &readback, 0, result_bytes as u64);
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let values: Vec<f32> = bytemuck::cast_slice(
        &readback
            .slice(..)
            .get_mapped_range()
            .expect("readback buffer is mapped"),
    )
    .to_vec();
    assert_eq!(values[0], 1.0, "direct-light gate at full sky");
    assert_eq!(values[1], 0.0, "no sky exposure");
    assert_eq!(values[2], 1.0, "Java full sky with no block light");
    assert_eq!(values[3], 0.0, "Java no sky exposure");
    let sky = 29.0_f32 / 60.0;
    assert!(
        (values[4] - sky / (4.0 - 3.0 * sky)).abs() < 1.0e-6,
        "Java fractional sky ignores block light: {}",
        values[4]
    );
    assert!(
        (values[sample_count] - 0.129).abs() < 1.0e-6,
        "night lightmap stays dark"
    );
}
