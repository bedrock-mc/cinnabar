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
    /// Retires removed materials and collects finished compiles even without visible batches.
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
        for entry in self.entries.values_mut() {
            for state in entry.variants.values_mut() {
                if let State::Creating(task) = state
                    && let Some(result) = bevy::tasks::futures::check_ready(task)
                {
                    *state = result.map_or(State::Failed, State::Ready);
                }
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
    use crate::pipeline_warmup::PendingPipelines as _;

    #[test]
    fn geometry_removal_preserves_material_pipelines_for_reuse() {
        use super::super::{clock, gpu, pipeline, tests::drawing};
        use bevy::{ecs::system::RunSystemOnce, render::render_phase::AddRenderCommand};
        use mod_api::drawing::DrawingPermissions;
        use render_scene::{ModRenderScene, mod_scene::retained::DrawingFrame};

        let (mut app, view) = crate::queue_review_support::app();
        app.init_resource::<gpu::Gpu>()
            .init_resource::<Materials>()
            .init_resource::<clock::AnimationClock>()
            .init_resource::<pipeline::Pipeline>()
            .init_resource::<ModRenderScene>()
            .add_render_command::<bevy::core_pipeline::core_3d::Transparent3d, pipeline::Draw>();
        let (mut store, _, mut instance) = drawing();
        let geometry = instance.geometry;
        let handles: Vec<_> = store.snapshot().instances.keys().copied().collect();
        let materials: Vec<_> = handles
            .iter()
            .map(|handle| {
                store.begin();
                let material = store
                    .create_material(mod_render::material::BUILTIN, DrawingPermissions::ALL)
                    .unwrap();
                instance.material = Some(material);
                store
                    .update(*handle, instance.clone(), DrawingPermissions::ALL)
                    .unwrap();
                store.commit();
                material
            })
            .collect();
        let drawable = store.snapshot().clone();
        app.insert_resource(DrawingFrame {
            connection: 1,
            live_owners: [(geometry.owner, geometry.generation)].into(),
            ..default()
        });
        let key = Key {
            msaa: Msaa::Sample4,
            hdr: false,
            always: false,
            additive: false,
        };
        let layout = app
            .world()
            .resource::<PipelineCache>()
            .get_bind_group_layout(&app.world().resource::<pipeline::Pipeline>().layout);
        let pipelines: Vec<_> = materials
            .iter()
            .map(|handle| {
                let material = store.snapshot().materials[handle].clone();
                let shader = source(&material.shader, include_str!("drawing.wgsl"));
                let pipeline = compile(
                    app.world().resource::<RenderDevice>(),
                    &layout,
                    &shader,
                    key,
                )
                .unwrap();
                let id = pipeline.id();
                app.world_mut().resource_mut::<Materials>().entries.insert(
                    *handle,
                    Entry {
                        _material: material,
                        variants: [(key, State::Ready(pipeline))].into(),
                    },
                );
                id
            })
            .collect();
        app.world_mut().resource_mut::<ModRenderScene>().retained = vec![store.snapshot().clone()];
        app.world_mut().run_system_once(gpu::queue).unwrap();
        assert_eq!(crate::queue_review_support::items(&app, view).len(), 2);

        store.begin();
        for handle in handles {
            store.remove(handle).unwrap();
        }
        store.remove(geometry).unwrap();
        store.commit();
        app.world_mut().resource_mut::<ModRenderScene>().retained = vec![store.snapshot().clone()];
        crate::queue_review_support::clear(&mut app, view);
        app.world_mut().run_system_once(gpu::queue).unwrap();
        assert!(app.world().resource::<gpu::Gpu>().meshes.is_empty());
        let retained = app.world().resource::<Materials>();
        assert!(
            retained.retired.is_empty(),
            "live material pipelines must not retire with geometry"
        );
        for (handle, id) in materials.iter().zip(&pipelines) {
            assert_eq!(retained.ready(*handle, key).unwrap().id(), *id);
        }

        app.world_mut().resource_mut::<ModRenderScene>().retained = vec![drawable.clone()];
        app.world_mut().run_system_once(gpu::queue).unwrap();
        assert_eq!(
            crate::queue_review_support::items(&app, view).len(),
            2,
            "reused materials must draw together without recompilation"
        );
        for (handle, id) in materials.iter().zip(&pipelines) {
            assert_eq!(
                app.world()
                    .resource::<Materials>()
                    .ready(*handle, key)
                    .unwrap()
                    .id(),
                *id
            );
        }

        let mut removed = (*drawable).clone();
        removed
            .instances
            .retain(|_, draw| draw.material != Some(materials[0]));
        removed.materials.remove(&materials[0]);
        app.world_mut().resource_mut::<ModRenderScene>().retained = vec![Arc::new(removed)];
        app.world_mut().run_system_once(gpu::queue).unwrap();
        assert!(
            app.world()
                .resource::<Materials>()
                .ready(materials[0], key)
                .is_none()
        );
        assert!(
            app.world()
                .resource::<Materials>()
                .ready(materials[1], key)
                .is_some()
        );
        app.world_mut()
            .resource_mut::<DrawingFrame>()
            .live_owners
            .clear();
        app.world_mut().run_system_once(gpu::queue).unwrap();
        assert!(app.world().resource::<Materials>().entries.is_empty());
    }

    #[test]
    fn completed_hidden_variants_stop_blocking_readiness() {
        let pool = bevy::tasks::TaskPoolBuilder::new().num_threads(0).build();
        let material = Arc::new(Material { shader: "".into() });
        let handle = DrawingHandle {
            owner: 1,
            generation: 1,
            slot: 1,
        };
        let key = Key {
            msaa: Msaa::Off,
            hdr: false,
            always: false,
            additive: false,
        };
        let variants = [
            key,
            Key {
                always: true,
                ..key
            },
        ]
        .into_iter()
        .map(|key| (key, State::Creating(pool.spawn_local(async { None }))))
        .collect();
        let mut materials = Materials::default();
        materials.entries.insert(
            handle,
            Entry {
                _material: material.clone(),
                variants,
            },
        );
        let mut resources = Resources::default();
        resources.materials.insert(handle, material);
        let resources = Arc::new(resources);
        assert!(resources.instances.is_empty());
        materials.retain(&[&resources]);
        assert!(materials.pending());

        pool.with_local_executor(|executor| assert!(executor.try_tick()));
        materials.retain(&[&resources]);
        assert!(materials.pending(), "one variant is still compiling");

        pool.with_local_executor(|executor| assert!(executor.try_tick()));
        materials.retain(&[&resources]);
        assert!(materials.entries.contains_key(&handle));
        assert!(
            !materials.pending(),
            "completed variants must release readiness without visible batches"
        );
        assert!(
            materials.entries[&handle]
                .variants
                .values()
                .all(|state| matches!(state, State::Failed))
        );
    }

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
