//! Native material pipelines are owned and released, rather than retained in Bevy's cache.
use super::pipeline::Key;
use bevy::{
    prelude::*,
    render::{render_resource::*, renderer::RenderDevice},
    tasks::{AsyncComputeTaskPool, Task},
};
use mod_api::drawing::DrawingHandle;
use mod_render::{material::Material, retained::Resources};
use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

enum State {
    Creating(Task<Option<RenderPipeline>>),
    Ready(RenderPipeline),
    Failed,
}
struct Entry {
    _material: Arc<Material>,
    variants: HashMap<Key, State>,
}
#[derive(Resource, Default)]
pub(super) struct Materials {
    entries: BTreeMap<DrawingHandle, Entry>,
}

/// Combines host-owned vertex code with an already admitted fragment module.
pub(super) fn source(fragment: &str, vertex: &str) -> String {
    format!("{fragment}\n{vertex}")
}

impl Materials {
    /// Releases native pipelines and shader references on removal, revocation or session end.
    pub fn retain(&mut self, resources: &[&Arc<Resources>]) {
        self.entries
            .retain(|handle, _| resources.iter().any(|r| r.materials.contains_key(handle)));
    }
    /// Starts at most the shared frame compile allowance; failed variants stay failed.
    #[allow(
        clippy::too_many_arguments,
        reason = "one owner admits a bounded pipeline variant"
    )]
    pub fn ensure(
        &mut self,
        handle: DrawingHandle,
        material: &Arc<Material>,
        key: Key,
        device: &RenderDevice,
        cache: &PipelineCache,
        layout: &BindGroupLayoutDescriptor,
        left: &mut usize,
    ) -> bool {
        let entry = self.entries.entry(handle).or_insert_with(|| Entry {
            _material: material.clone(),
            variants: HashMap::new(),
        });
        if !entry.variants.contains_key(&key) && *left > 0 {
            *left -= 1;
            let device = device.clone();
            let layout = cache.get_bind_group_layout(layout);
            let shader = source(&material.shader, include_str!("drawing.wgsl"));
            let task = async move { compile(&device, &layout, &shader, key) };
            let state = if cfg!(any(target_os = "macos", target_arch = "wasm32")) {
                bevy::tasks::block_on(task).map_or(State::Failed, State::Ready)
            } else {
                State::Creating(AsyncComputeTaskPool::get().spawn(task))
            };
            entry.variants.insert(key, state);
        }
        if let Some(State::Creating(task)) = entry.variants.get_mut(&key)
            && let Some(result) = bevy::tasks::futures::check_ready(task)
        {
            entry
                .variants
                .insert(key, result.map_or(State::Failed, State::Ready));
        }
        matches!(entry.variants.get(&key), Some(State::Ready(_)))
    }
    /// Returns only successfully compiled pipelines; pending/failed materials do not draw.
    pub fn ready(&self, handle: DrawingHandle, key: Key) -> Option<&RenderPipeline> {
        match self.entries.get(&handle)?.variants.get(&key)? {
            State::Ready(pipeline) => Some(pipeline),
            _ => None,
        }
    }
}

/// Compiles a host descriptor under a validation scope so invalid device output skips drawing.
fn compile(
    device: &RenderDevice,
    layout: &BindGroupLayout,
    shader: &str,
    key: Key,
) -> Option<RenderPipeline> {
    let validation = device
        .wgpu_device()
        .push_error_scope(wgpu::ErrorFilter::Validation);
    let module = device.create_and_validate_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("mod drawing material"),
        source: wgpu::ShaderSource::Wgsl(shader.into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("mod drawing material"),
        bind_group_layouts: &[Some(layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("mod drawing material"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertex"),
            compilation_options: default(),
            buffers: &[],
        },
        primitive: default(),
        depth_stencil: Some(super::pipeline::depth(key)),
        multisample: MultisampleState {
            count: key.msaa.samples(),
            ..default()
        },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("material_fragment"),
            compilation_options: default(),
            targets: &[Some(super::pipeline::target(key))],
        }),
        multiview_mask: None,
        cache: None,
    });
    if let Some(error) = bevy::tasks::block_on(validation.pop()) {
        warn!("mod drawing material rejected by device: {error}");
        None
    } else {
        Some(pipeline)
    }
}

impl crate::pipeline_warmup::PendingPipelines for Materials {
    /// Includes asynchronous custom compiles in renderer readiness.
    fn pending(&self) -> bool {
        self.entries
            .values()
            .any(|e| e.variants.values().any(|s| matches!(s, State::Creating(_))))
    }
}
