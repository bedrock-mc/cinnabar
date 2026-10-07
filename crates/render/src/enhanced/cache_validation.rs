//! Exercises Bevy's recursive import loading and asynchronous native pipeline compilation.
use super::validation::{
    actor_descriptor, actor_motion_descriptor, variant_definitions, variant_shader, variants,
};
use crate::{shader_safety, shader_source};
use bevy::{
    asset::Assets,
    prelude::Shader,
    render::{
        render_resource::*,
        renderer::{RenderAdapter, RenderDevice, WgpuWrapper},
    },
    shader::{ShaderDefVal, ShaderImport},
    tasks::{AsyncComputeTaskPool, TaskPoolBuilder, block_on},
};
use std::{process::Command, sync::Arc};

#[test]
fn enhanced_pipelines_compile_through_async_cache() {
    const CHILD: &str = "CINNABAR_SHADER_CACHE_TEST_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "enhanced::cache_validation::enhanced_pipelines_compile_through_async_cache",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        if output.status.success() {
            eprint!("{}", String::from_utf8_lossy(&output.stderr));
        }
        assert!(
            output.status.success(),
            "async shader compilation failed ({}):\n{}\n{}",
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    AsyncComputeTaskPool::get_or_init(|| {
        TaskPoolBuilder::new()
            .num_threads(1)
            .thread_name("Async Compute Task Pool".to_owned())
            .build()
    });
    let mut settings = bevy::render::settings::WgpuSettings {
        backends: Some(if cfg!(target_os = "windows") {
            wgpu::Backends::DX12
        } else {
            wgpu::Backends::PRIMARY
        }),
        ..Default::default()
    };
    #[cfg(target_os = "windows")]
    super::configure_enhanced_shader_compiler(&mut settings);
    eprintln!("async shader compiler: {:?}", settings.dx12_shader_compiler);
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: settings.backends.unwrap(),
        // Debug's skipped HLSL optimization does not exercise the play-profile compiler.
        flags: wgpu::InstanceFlags::VALIDATION,
        backend_options: wgpu::BackendOptions {
            dx12: wgpu::Dx12BackendOptions {
                shader_compiler: settings.dx12_shader_compiler,
                ..Default::default()
            },
            ..Default::default()
        },
        ..Default::default()
    });
    let Ok(adapter) = block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    })) else {
        eprintln!("missing fixture: native GPU adapter for async Enhanced shader compilation");
        return;
    };
    eprintln!("async shader adapter: {:?}", adapter.get_info());
    let (device, _) = block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        required_limits: adapter.limits(),
        ..Default::default()
    }))
    .unwrap();
    let mut cache = PipelineCache::new(
        RenderDevice::from(device),
        RenderAdapter(Arc::new(WgpuWrapper::new(adapter))),
        false,
    );
    let mut assets = Assets::<Shader>::default();
    for (name, source) in shader_source::composable_sources() {
        let source = if source.contains("#define_import_path") {
            source
        } else {
            format!("#define_import_path {name}\n{source}")
        };
        let mut shader = shader_safety::from_wgsl(source, name);
        shader.import_path = ShaderImport::Custom(name.to_owned());
        let handle = assets.add(shader.clone());
        cache.set_shader(handle.id(), shader);
    }
    let fullscreen = shader_safety::from_wgsl(
        shader_source::fullscreen_vertex_source(),
        "async_fullscreen.wgsl",
    );
    let fullscreen_handle = assets.add(fullscreen.clone());
    cache.set_shader(fullscreen_handle.id(), fullscreen);
    for (name, _, vertex, fragment, shadow) in variants() {
        eprintln!("async cache compiling {name}/{fragment}");
        let shader = variant_shader(name);
        let handle = assets.add(shader.clone());
        cache.set_shader(handle.id(), shader);
        let shader_defs: Vec<ShaderDefVal> = variant_definitions(name, shadow)
            .iter()
            .map(|definition| (*definition).into())
            .collect();
        let motion = fragment.ends_with("_motion");
        let actor = match name {
            "actor" => Some(actor_descriptor(shadow)),
            "actor_motion" => Some(actor_motion_descriptor()),
            _ => None,
        };
        let layout = if let Some(actor) = &actor {
            actor.layout.clone()
        } else if matches!(fragment, "capture_probe" | "filter_mip") {
            vec![super::probes::layout()]
        } else if fragment == "resolve_reflections" {
            vec![super::probes::reflection_resolve_layout()]
        } else if fragment == "resolve_local_shadows" {
            vec![super::local_shadow_history::layout()]
        } else if fragment == "prefilter_environment" {
            vec![super::probes::environment_filter_layout()]
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
        let id = cache.queue_render_pipeline(RenderPipelineDescriptor {
            label: Some(format!("async {name}/{fragment}").into()),
            layout,
            vertex: VertexState {
                shader: if vertex == "fullscreen" {
                    fullscreen_handle.clone()
                } else {
                    handle.clone()
                },
                entry_point: Some(vertex.into()),
                shader_defs: shader_defs.clone(),
                ..Default::default()
            },
            fragment: Some(FragmentState {
                shader: handle,
                entry_point: Some(fragment.into()),
                shader_defs,
                targets: if shadow && !motion {
                    vec![]
                } else if fragment == "resolve_reflections" {
                    vec![Some(super::probes::reflection_resolve_target())]
                } else {
                    vec![Some(ColorTargetState {
                        format: TextureFormat::Rgba16Float,
                        blend: None,
                        write_mask: ColorWrites::ALL,
                    })]
                },
            }),
            primitive: actor
                .as_ref()
                .map_or_else(Default::default, |actor| actor.primitive),
            depth_stencil: actor
                .as_ref()
                .and_then(|actor| actor.depth_stencil.clone())
                .or_else(|| {
                    shadow.then_some(DepthStencilState {
                        format: TextureFormat::Depth32Float,
                        depth_write_enabled: true,
                        depth_compare: if motion {
                            CompareFunction::GreaterEqual
                        } else {
                            CompareFunction::LessEqual
                        },
                        stencil: Default::default(),
                        bias: Default::default(),
                    })
                }),
            ..Default::default()
        });
        cache.block_on_render_pipeline(id);
        assert!(
            matches!(
                cache.get_render_pipeline_state(id),
                CachedPipelineState::Ok(_)
            ),
            "{name}/{fragment}: {:?}",
            cache.get_render_pipeline_state(id)
        );
    }

    let cloud_shader = super::cloud_noise::shader(
        include_str!("cloud_noise.wgsl"),
        "async_cloud_noise.wgsl".to_owned(),
    );
    assets
        .insert(
            super::cloud_noise::CLOUD_NOISE_SHADER.id(),
            cloud_shader.clone(),
        )
        .unwrap();
    cache.set_shader(super::cloud_noise::CLOUD_NOISE_SHADER.id(), cloud_shader);
    let indirect_shader = shader_safety::from_wgsl(
        include_str!("indirect_compute.wgsl"),
        "async_indirect_compute.wgsl",
    );
    assets
        .insert(super::INDIRECT_COMPUTE_SHADER.id(), indirect_shader.clone())
        .unwrap();
    cache.set_shader(super::INDIRECT_COMPUTE_SHADER.id(), indirect_shader);
    for (shader, entry, layout, shader_defs) in [
        (
            super::cloud_noise::CLOUD_NOISE_SHADER,
            "generate_noise",
            super::cloud_noise::generation_layout(false),
            Vec::new(),
        ),
        (
            super::cloud_noise::CLOUD_NOISE_SHADER,
            "filter_noise",
            super::cloud_noise::generation_layout(true),
            Vec::new(),
        ),
        (
            super::INDIRECT_COMPUTE_SHADER,
            "update_spatial_irradiance",
            super::indirect::compute_layout(),
            vec![ShaderDefVal::Bool("INDIRECT_COMPUTE".into(), true)],
        ),
    ] {
        eprintln!("async cache compiling {entry}");
        let id = cache.queue_compute_pipeline(ComputePipelineDescriptor {
            label: Some(format!("async {entry}").into()),
            layout: vec![layout],
            shader,
            shader_defs,
            entry_point: Some(entry.into()),
            ..Default::default()
        });
        loop {
            cache.process_queue();
            match cache.get_compute_pipeline_state(id) {
                CachedPipelineState::Queued | CachedPipelineState::Creating(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
                CachedPipelineState::Ok(Pipeline::ComputePipeline(_)) => break,
                state => panic!("{entry}: {state:?}"),
            }
        }
        assert!(
            cache.get_compute_pipeline(id).is_some(),
            "{entry} must be usable"
        );
    }
}
