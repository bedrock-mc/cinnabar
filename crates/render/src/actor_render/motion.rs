//! Motion storage is separate from vanilla actor bindings and tracks completed world resolves.

use std::sync::atomic::{AtomicBool, Ordering};

use super::*;
use crate::enhanced::actor_motion::{
    ActorMotionHistory, ActorMotionInstanceGpu, ActorMotionUploadCache,
};

#[derive(Resource, Default)]
pub(super) struct ActorMotionGpu {
    history: ActorMotionHistory,
    uploads: ActorMotionUploadCache,
    instances: Option<Buffer>,
    bones: Option<Buffer>,
    pub(super) bind_group: Option<BindGroup>,
    coverage_drawn: AtomicBool,
    visible_instances: Vec<AtomicBool>,
    resolved: AtomicBool,
    enabled: bool,
}

pub(crate) fn actor_motion_layout() -> BindGroupLayoutDescriptor {
    let storage = |binding, bytes| BindGroupLayoutEntry {
        binding,
        visibility: ShaderStages::VERTEX,
        ty: BindingType::Buffer {
            ty: BufferBindingType::Storage { read_only: true },
            has_dynamic_offset: false,
            min_binding_size: BufferSize::new(bytes),
        },
        count: None,
    };
    BindGroupLayoutDescriptor::new(
        "submitted actor pose motion layout",
        &[
            storage(0, size_of::<ActorMotionInstanceGpu>() as u64),
            storage(1, crate::actor::ACTOR_BONE_MATRIX_BYTES as u64),
        ],
    )
}

fn upload(
    device: &RenderDevice,
    queue: &RenderQueue,
    buffer: &mut Option<Buffer>,
    label: &'static str,
    bytes: &[u8],
    minimum: u64,
    contents_changed: bool,
) -> bool {
    let needed = (bytes.len() as u64).max(minimum);
    let changed = buffer.as_ref().is_none_or(|buffer| buffer.size() < needed);
    if changed {
        *buffer = Some(device.create_buffer(&BufferDescriptor {
            label: Some(label),
            size: needed.next_power_of_two(),
            usage: BufferUsages::STORAGE | BufferUsages::COPY_DST,
            mapped_at_creation: false,
        }));
    }
    if !bytes.is_empty() && (changed || contents_changed) {
        queue.write_buffer(buffer.as_ref().unwrap(), 0, bytes);
    }
    changed
}

pub(super) fn prepare_actor_motion(
    frame: Res<ActorRenderFrame>,
    actors: Res<ActorGpu>,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    views: Query<&crate::enhanced::EnhancedRendering>,
    mut motion: ResMut<ActorMotionGpu>,
) {
    let enabled = views.iter().any(|settings| !settings.reflection_capture);
    if !enabled || actors.instance_count == 0 {
        motion.history.clear();
        motion.coverage_drawn.store(false, Ordering::Relaxed);
        motion.resolved.store(false, Ordering::Relaxed);
        motion.enabled = enabled;
        return;
    }
    if motion.resolved.swap(false, Ordering::Relaxed) {
        let ActorMotionGpu {
            history,
            visible_instances,
            ..
        } = &mut *motion;
        history.commit_submitted(|index| {
            visible_instances
                .get(index)
                .is_some_and(|visible| visible.load(Ordering::Relaxed))
        });
    }
    motion
        .visible_instances
        .resize_with(frame.rig.instances.len(), || AtomicBool::new(false));
    for visible in &motion.visible_instances {
        visible.store(false, Ordering::Relaxed);
    }
    motion.coverage_drawn.store(false, Ordering::Relaxed);
    if !motion.enabled {
        motion.history.clear();
    }
    motion.enabled = true;
    if !motion.history.prepare(&frame.rig) && motion.bind_group.is_some() {
        return;
    }
    let ActorMotionGpu {
        history,
        instances,
        bones,
        uploads,
        ..
    } = &mut *motion;
    let [instances_changed, bones_changed] = uploads.update(history.instances(), history.bones());
    let instance_changed = upload(
        &device,
        &queue,
        instances,
        "submitted actor transforms",
        bytemuck::cast_slice(history.instances()),
        size_of::<ActorMotionInstanceGpu>() as u64,
        instances_changed,
    );
    let bone_changed = upload(
        &device,
        &queue,
        bones,
        "submitted interpolated actor bones",
        bytemuck::cast_slice(history.bones()),
        crate::actor::ACTOR_BONE_MATRIX_BYTES as u64,
        bones_changed,
    );
    if instance_changed || bone_changed || motion.bind_group.is_none() {
        motion.bind_group = Some(device.create_bind_group(
            "submitted actor pose motion bindings",
            &cache.get_bind_group_layout(&actor_motion_layout()),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: motion.instances.as_ref().unwrap().as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: motion.bones.as_ref().unwrap().as_entire_binding(),
                },
            ],
        ));
    }
}

pub(super) fn record_coverage(world: &World, first: u32, instances: u32) {
    if instances > 0 {
        if let Some(motion) = world.get_resource::<ActorMotionGpu>() {
            motion.coverage_drawn.store(true, Ordering::Relaxed);
            for index in first..first.saturating_add(instances) {
                if let Some(visible) = motion.visible_instances.get(index as usize) {
                    visible.store(true, Ordering::Relaxed);
                }
            }
        }
    }
}

pub(crate) fn mark_actor_motion_submitted(world: &World) {
    if let Some(motion) = world.get_resource::<ActorMotionGpu>() {
        if motion.coverage_drawn.load(Ordering::Relaxed) {
            motion.resolved.store(true, Ordering::Relaxed);
        }
    }
}
