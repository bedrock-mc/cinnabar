//! Validate every Enhanced shader and create its pipeline on an available real adapter.
use crate::shader_source;

type Variant = (&'static str, String, &'static str, &'static str, bool);

/// All forward, shadow-caster and fullscreen variants used by the extension.
pub(super) fn variants() -> Vec<Variant> {
    let mut result = Vec::new();
    for (name, source) in [
        ("chunk", include_str!("../chunk.wgsl")),
        ("model", include_str!("../model.wgsl")),
        ("liquid", include_str!("../liquid.wgsl")),
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
        if source.contains("#ifdef ENHANCED_SHADOW") {
            result.push((
                name,
                shader_source::composed(source, &["ENHANCED_SHADOW"]),
                "vertex",
                "fragment_shadow",
                true,
            ));
            let motion_name = match name {
                "chunk" => "chunk_motion",
                "model" => "model_motion",
                _ => unreachable!(),
            };
            result.push((
                motion_name,
                shader_source::composed(source, &["ENHANCED_SHADOW", "ENHANCED_MOTION"]),
                "vertex",
                "fragment_motion",
                true,
            ));
        }
    }
    let actor = variant_shader("actor");
    let bevy::shader::Source::Wgsl(actor_source) = actor.source else {
        panic!("actor shader source must remain WGSL");
    };
    for (definition, fragment, shadow) in [
        ("ENHANCED", "actor_fragment", false),
        ("ENHANCED_SHADOW", "actor_fragment_shadow", true),
    ] {
        result.push((
            "actor",
            shader_source::composed(&actor_source, &[definition]),
            "actor_vertex",
            fragment,
            shadow,
        ));
    }
    result.push((
        "actor_motion",
        shader_source::composed(&actor_source, &["ENHANCED_SHADOW", "ENHANCED_MOTION"]),
        "actor_vertex",
        "actor_fragment_motion",
        true,
    ));
    for fragment in [
        "light_shafts",
        "sky_lut",
        "sky_background",
        "cloud_shadows",
        "effects",
        "composite",
        "temporal_resolve",
        "present",
    ] {
        result.push((
            fragment,
            shader_source::composed(include_str!("../enhanced/post.wgsl"), &[]),
            "fullscreen",
            fragment,
            false,
        ));
    }
    for fragment in [
        "capture_probe",
        "filter_mip",
        "cloud_environment",
        "resolve_reflections",
    ] {
        result.push((
            fragment,
            shader_source::composed(include_str!("probe.wgsl"), &[]),
            "fullscreen",
            fragment,
            false,
        ));
    }
    result.push((
        "resolve_local_shadows",
        shader_source::composed(include_str!("local_shadow_history.wgsl"), &[]),
        "fullscreen",
        "resolve_local_shadows",
        false,
    ));
    result.push((
        "prefilter_environment",
        shader_source::composed(include_str!("probe_filter.wgsl"), &[]),
        "fullscreen",
        "prefilter_environment",
        false,
    ));
    result
}

#[test]
fn enhanced_exposure_shader_validates() {
    let source = shader_source::composed(include_str!("exposure.wgsl"), &[]);
    let module = naga::front::wgsl::parse_str(&source)
        .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&source)));
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .validate(&module)
    .unwrap();
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
fn enhanced_pipelines_build_on_native_adapter() {
    let pool = bevy::tasks::TaskPoolBuilder::new()
        .num_threads(1)
        .thread_name("Enhanced shader compilation".to_owned())
        .build();
    bevy::tasks::block_on(pool.spawn(async { build_native_pipelines() }));
}

pub(super) fn variant_shader(name: &str) -> bevy::prelude::Shader {
    let source = match name {
        "chunk" | "chunk_motion" => include_str!("../chunk.wgsl"),
        "model" | "model_motion" => include_str!("../model.wgsl"),
        "liquid" => include_str!("../liquid.wgsl"),
        "actor" | "actor_motion" => include_str!("../actor.wgsl"),
        "capture_probe" | "filter_mip" | "resolve_reflections" | "cloud_environment" => {
            include_str!("probe.wgsl")
        }
        "prefilter_environment" => include_str!("probe_filter.wgsl"),
        "resolve_local_shadows" => include_str!("local_shadow_history.wgsl"),
        _ => include_str!("post.wgsl"),
    };
    if matches!(name, "actor" | "actor_motion") {
        crate::shader_safety::from_actor_wgsl(
            source,
            name,
            crate::actor::ACTOR_GPU_INSTANCE_WORDS,
            render_model::ACTOR_RIG_VERTEX_WORDS,
        )
    } else {
        crate::shader_safety::from_wgsl(crate::material_shader::source(source), name)
    }
}

pub(super) fn variant_definitions(name: &str, shadow: bool) -> &'static [&'static str] {
    if matches!(name, "chunk_motion" | "model_motion" | "actor_motion") {
        &["ENHANCED_SHADOW", "ENHANCED_MOTION"]
    } else if shadow {
        &["ENHANCED_SHADOW"]
    } else if matches!(name, "chunk" | "model" | "liquid" | "actor") {
        &["ENHANCED"]
    } else {
        &[]
    }
}

pub(super) fn actor_descriptor(
    shadow: bool,
) -> bevy::render::render_resource::RenderPipelineDescriptor {
    use bevy::render::view::ViewTarget;
    if shadow {
        crate::actor_render::actor_shadow_pipeline_descriptor(
            super::gpu::enhanced_caster_layout(),
            super::gpu::SHADOW_FORMAT,
        )
    } else {
        let mut descriptor = crate::actor_render::actor_pipeline_descriptor(
            crate::actor_render::actor_bind_group_layout(),
        );
        descriptor.layout.push(super::gpu::enhanced_view_layout());
        descriptor.vertex.shader_defs = vec!["ENHANCED".into()];
        descriptor.fragment.as_mut().unwrap().shader_defs = vec!["ENHANCED".into()];
        descriptor.fragment.as_mut().unwrap().targets[0]
            .as_mut()
            .unwrap()
            .format = ViewTarget::TEXTURE_FORMAT_HDR;
        descriptor
    }
}

pub(super) fn actor_motion_descriptor() -> bevy::render::render_resource::RenderPipelineDescriptor {
    super::depth::camera_depth(crate::actor_render::actor_motion_pipeline_descriptor(
        super::gpu::enhanced_caster_layout(),
        super::gpu::SHADOW_FORMAT,
    ))
}

fn native_module(name: &str, shadow: bool) -> naga::Module {
    let shader = variant_shader(name);
    let bevy::shader::Source::Wgsl(source) = shader.source else {
        panic!("Enhanced shader source must remain WGSL");
    };
    shader_source::composed_module(&source, variant_definitions(name, shadow))
}

pub(super) fn native_instance() -> wgpu::Instance {
    #[cfg_attr(not(target_os = "windows"), allow(unused_mut))]
    let mut settings = bevy::render::settings::WgpuSettings::default();
    #[cfg(target_os = "windows")]
    super::configure_enhanced_shader_compiler(&mut settings);
    wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: settings.backends.unwrap_or(wgpu::Backends::PRIMARY),
        flags: wgpu::InstanceFlags::VALIDATION,
        backend_options: wgpu::BackendOptions {
            dx12: wgpu::Dx12BackendOptions {
                shader_compiler: settings.dx12_shader_compiler,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    })
}

fn build_native_pipelines() {
    let instance = native_instance();
    let Ok(adapter) =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("missing fixture: native GPU adapter for Enhanced pipeline validation");
        return;
    };
    let (device, _) = bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("enhanced smoke"),
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .expect("Enhanced smoke device");
    for (name, _, vertex, fragment, shadow) in variants() {
        eprintln!("compiling Enhanced native pipeline {name}/{fragment}");
        let motion = fragment.ends_with("_motion");
        let actor = match name {
            "actor" => Some(actor_descriptor(shadow)),
            "actor_motion" => Some(actor_motion_descriptor()),
            _ => None,
        };
        let descriptors = if let Some(actor) = &actor {
            actor.layout.clone()
        } else if matches!(
            fragment,
            "capture_probe" | "filter_mip" | "cloud_environment"
        ) {
            vec![super::probes::layout()]
        } else if fragment == "resolve_local_shadows" {
            vec![super::local_shadow_history::layout()]
        } else if fragment == "prefilter_environment" {
            vec![super::probes::environment_filter_layout()]
        } else if fragment == "resolve_reflections" {
            vec![super::probes::reflection_resolve_layout()]
        } else if vertex == "fullscreen" {
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
        device.push_error_scope(wgpu::ErrorFilter::Validation);
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
            bind_group_layouts: &groups.iter().collect::<Vec<_>>(),
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some(name),
            source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(native_module(name, shadow))),
        });
        let targets = if fragment == "resolve_local_shadows" {
            vec![
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
            ]
        } else if motion {
            [
                wgpu::TextureFormat::Rgba16Float,
                wgpu::TextureFormat::Rg16Float,
            ]
            .into_iter()
            .map(|format| {
                Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })
            })
            .collect()
        } else {
            vec![Some(if fragment == "resolve_reflections" {
                super::probes::reflection_resolve_target()
            } else {
                wgpu::ColorTargetState {
                    format: wgpu::TextureFormat::Rgba16Float,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }
            })]
        };
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
                targets: if shadow && !motion { &[] } else { &targets },
            }),
            primitive: actor
                .as_ref()
                .map_or_else(Default::default, |actor| actor.primitive),
            depth_stencil: actor
                .as_ref()
                .and_then(|actor| actor.depth_stencil.clone())
                .or_else(|| {
                    shadow.then_some(wgpu::DepthStencilState {
                        format: wgpu::TextureFormat::Depth32Float,
                        depth_write_enabled: true,
                        depth_compare: if motion {
                            wgpu::CompareFunction::GreaterEqual
                        } else {
                            wgpu::CompareFunction::LessEqual
                        },
                        stencil: Default::default(),
                        bias: Default::default(),
                    })
                }),
            multisample: Default::default(),
            multiview: None,
            cache: None,
        });
        let error = bevy::tasks::block_on(device.pop_error_scope());
        assert!(error.is_none(), "{name}/{fragment}: {error:?}");
    }
    let descriptor = super::exposure::layout();
    let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(descriptor.label.as_ref()),
        entries: &descriptor.entries,
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Enhanced exposure production layout"),
        bind_group_layouts: &[&group],
        push_constant_ranges: &[],
    });
    let source = shader_source::composed_module(include_str!("exposure.wgsl"), &[]);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Enhanced exposure"),
        source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(source)),
    });
    for entry in ["build_histogram", "adapt_exposure"] {
        device.push_error_scope(wgpu::ErrorFilter::Validation);
        let _pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        });
        let error = bevy::tasks::block_on(device.pop_error_scope());
        assert!(error.is_none(), "{entry}: {error:?}");
    }
    let descriptor = super::multiple_scattering::generation_layout();
    let group = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(descriptor.label.as_ref()),
        entries: &descriptor.entries,
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Enhanced atmospheric transfer production layout"),
        bind_group_layouts: &[&group],
        push_constant_ranges: &[],
    });
    let source = shader_source::composed_module(
        &super::multiple_scattering::shader_source(include_str!("multiple_scattering.wgsl")),
        &[],
    );
    device.push_error_scope(wgpu::ErrorFilter::Validation);
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("Enhanced atmospheric transfer"),
        source: wgpu::ShaderSource::Naga(std::borrow::Cow::Owned(source)),
    });
    let _pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("generate_multiple_scattering"),
        layout: Some(&layout),
        module: &shader,
        entry_point: Some("generate_multiple_scattering"),
        compilation_options: Default::default(),
        cache: None,
    });
    let error = bevy::tasks::block_on(device.pop_error_scope());
    assert!(error.is_none(), "atmospheric transfer: {error:?}");
}

// Night and brightness darken the lightmap, never the open-sky gate on direct moonlight.
#[test]
fn full_sky_exposure_keeps_direct_light_under_a_night_lightmap() {
    let instance = native_instance();
    let Ok(adapter) =
        bevy::tasks::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
    else {
        eprintln!("missing fixture: native GPU adapter for full-sky night-lightmap validation");
        return;
    };
    let (device, queue) =
        bevy::tasks::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default())).unwrap();
    let source = shader_source::composed(
        "#import cinnabar::enhanced_view::sky_illumination
#import cinnabar::lighting::light_colour
@group(0) @binding(0) var<storage, read_write> results: array<f32, 3>;
@compute @workgroup_size(1) fn main() {
    results[0] = smoothstep(0.25, 0.8, sky_illumination(240u));
    results[1] = sky_illumination(0u);
    results[2] = light_colour(240u).r;
}",
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
    let lightmap = buffer(wgpu::BufferUsages::UNIFORM, bytemuck::cast_slice(&night));
    let results = buffer(
        wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        &[0; 12],
    );
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 12,
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
    encoder.copy_buffer_to_buffer(&results, 0, &readback, 0, 12);
    queue.submit([encoder.finish()]);
    readback.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let values: Vec<f32> = bytemuck::cast_slice(&readback.slice(..).get_mapped_range()).to_vec();
    assert_eq!(values[0], 1.0, "direct-light gate at full sky");
    assert_eq!(values[1], 0.0, "no sky exposure");
}
