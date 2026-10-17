//! Native material pipelines are owned and released, rather than retained in Bevy's cache.
use super::pipeline::Key;
use bevy::{
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
    },
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
    retired: Vec<Entry>,
}

/// Combines host-owned vertex code with an already admitted fragment module.
pub(super) fn source(fragment: &str, vertex: &str) -> String {
    format!("{fragment}\n{vertex}")
}

impl Materials {
    /// Releases native pipelines and shader references on removal, revocation or session end.
    pub fn retain(&mut self, resources: &[&Arc<Resources>]) {
        let removed: Vec<_> = self
            .entries
            .keys()
            .copied()
            .filter(|handle| !resources.iter().any(|r| r.materials.contains_key(handle)))
            .collect();
        for handle in removed {
            if let Some(entry) = self.entries.remove(&handle) {
                self.retired.push(entry);
            }
        }
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
        if let std::collections::hash_map::Entry::Vacant(slot) = entry.variants.entry(key)
            && *left > 0
        {
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
            slot.insert(state);
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

/// Retains material quota and native pipelines until the last submitted frame completes.
pub(super) fn retire(mut materials: ResMut<Materials>, queue: Res<RenderQueue>) {
    if materials.retired.is_empty() {
        return;
    }
    let retired = std::mem::take(&mut materials.retired);
    crate::device_poll::on_frame_complete(&queue, move || drop(retired));
}

impl crate::pipeline_warmup::PendingPipelines for Materials {
    /// Includes asynchronous custom compiles in renderer readiness.
    fn pending(&self) -> bool {
        self.entries
            .values()
            .any(|e| e.variants.values().any(|s| matches!(s, State::Creating(_))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removed_materials_stay_charged_until_frame_retirement() {
        let material = Arc::new(Material { shader: "".into() });
        let handle = DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        };
        let mut materials = Materials::default();
        materials.entries.insert(
            handle,
            Entry {
                _material: material.clone(),
                variants: HashMap::new(),
            },
        );
        let mut resources = Resources::default();
        resources.materials.insert(handle, material.clone());
        materials.retain(&[&Arc::new(resources)]);
        assert!(materials.entries.contains_key(&handle));
        materials.retain(&[]);
        assert!(materials.entries.is_empty());
        assert_eq!(Arc::strong_count(&material), 2);
        assert_eq!(materials.retired.len(), 1);
        drop(materials);
        assert_eq!(Arc::strong_count(&material), 1);
    }
}
