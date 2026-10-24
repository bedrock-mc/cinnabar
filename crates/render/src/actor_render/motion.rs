//! Depth-tested velocities reuse the actor mesh, material coverage and previous rendered pose.

use super::*;
use crate::motion_blur::{
    actor_history::{ActorHistory, MotionInstance},
    history::CameraHistory,
};
use bevy::render::{renderer::RenderContext, view::ViewDepthStencilTexture};

mod buffer;
use buffer::MotionBuffer;

pub(crate) const MOTION_FORMAT: TextureFormat = TextureFormat::Rgba16Float;

/// Shared actor bindings occupy groups zero and one; only the velocity pass binds group two.
pub(super) fn layout() -> BindGroupLayoutDescriptor {
    BindGroupLayoutDescriptor::new(
        "previous rendered actor pose",
        &[
            BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(size_of::<MotionInstance>() as u64),
                },
                count: None,
            },
            BindGroupLayoutEntry {
                binding: 1,
                visibility: ShaderStages::VERTEX,
                ty: BindingType::Buffer {
                    ty: BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: BufferSize::new(render_model::ACTOR_BONE_MATRIX_BYTES as u64),
                },
                count: None,
            },
        ],
    )
}

pub(super) struct MotionSpecializer;

impl Specializer<RenderPipeline> for MotionSpecializer {
    type Key = ActorPipelineKey;

    /// Keeps the color pass's culling and alpha coverage while reading its final depth.
    fn specialize(
        &self,
        key: Self::Key,
        descriptor: &mut RenderPipelineDescriptor,
    ) -> Result<Canonical<Self::Key>, BevyError> {
        let contract = ActorPipelineSpecializer.specialize(key, descriptor)?;
        descriptor.label = Some("previous rendered actor velocity".into());
        let depth = descriptor.depth_stencil.as_mut().unwrap();
        depth.depth_write_enabled = Some(false);
        depth.depth_compare = Some(CompareFunction::Equal);
        descriptor
            .vertex
            .shader_defs
            .push(bevy::shader::ShaderDefVal::Bool(
                "ACTOR_MOTION".into(),
                true,
            ));
        let fragment = descriptor.fragment.as_mut().unwrap();
        fragment.entry_point = Some("actor_motion_fragment".into());
        fragment.shader_defs.push(bevy::shader::ShaderDefVal::Bool(
            "ACTOR_MOTION".into(),
            true,
        ));
        fragment.targets[0] = Some(ColorTargetState {
            format: MOTION_FORMAT,
            blend: None,
            write_mask: ColorWrites::ALL,
        });
        Ok(contract)
    }
}

/// Only surfaces represented in the final scene depth can overwrite its camera velocity.
pub(super) fn eligible(material: u32) -> bool {
    let kind = material & assets::EntityRenderMaterialState::KIND_MASK;
    !super::phase::sorted(material)
        && kind != assets::EntityRenderMaterial::DissolveDepth as u32
        && crate::actor::material::state(material).is_none_or(|state| state.depth_write)
}

/// Enabled views own their history and reusable velocity resources; disabling drops them together.
pub(crate) struct MotionView {
    texture: Texture,
    pub view: TextureView,
    history: ActorHistory,
    instances: MotionBuffer,
    bones: MotionBuffer,
    binding: Option<BindGroup>,
    draws: Vec<(crate::actor::gpu::ActorDrawSpan, CachedRenderPipelineId)>,
    frame_generation: u64,
    pub moved: bool,
    pub ready: bool,
}

impl MotionView {
    /// Allocates only for a visible actor frame under an enabled exposure setting.
    fn new(device: &RenderDevice, depth: &ViewDepthStencilTexture) -> Option<Self> {
        let size = depth.texture().size();
        let bytes = u64::from(size.width)
            * u64::from(size.height)
            * 8
            * u64::from(depth.texture().sample_count());
        let texture = gpu_allocation::checked(device, "rendered actor velocities", bytes, || {
            device.create_texture(&TextureDescriptor {
                label: Some("rendered actor velocities"),
                size: depth.texture().size(),
                mip_level_count: 1,
                sample_count: depth.texture().sample_count(),
                dimension: TextureDimension::D2,
                format: MOTION_FORMAT,
                usage: TextureUsages::TEXTURE_BINDING | TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        })?;
        let view = gpu_allocation::checked(device, "actor velocity view", 0, || {
            texture.create_view(&Default::default())
        })?;
        Some(Self {
            view,
            texture,
            history: ActorHistory::default(),
            instances: MotionBuffer::new(device, size_of::<MotionInstance>())?,
            bones: MotionBuffer::new(device, render_model::ACTOR_BONE_MATRIX_BYTES)?,
            binding: None,
            draws: Vec::new(),
            frame_generation: 0,
            moved: false,
            ready: false,
        })
    }
}

/// Prepares exact current instance ordering after ordinary actor resource preparation.
#[expect(
    clippy::too_many_arguments,
    reason = "borrowed resources belong to the same prepared view"
)]
pub(crate) fn prepare(
    state: &mut Option<MotionView>,
    frame: Option<&ActorRenderFrame>,
    gpu: Option<&ActorGpu>,
    pipeline: Option<&ActorPipeline>,
    previous_camera: CameraHistory,
    current_camera: CameraHistory,
    continuous: bool,
    depth: &ViewDepthStencilTexture,
    hdr: bool,
    enhanced: bool,
    device: &RenderDevice,
    queue: &RenderQueue,
    cache: &PipelineCache,
) {
    let Some((frame, gpu, pipeline)) = frame
        .zip(gpu)
        .zip(pipeline)
        .map(|((frame, gpu), pipeline)| (frame, gpu, pipeline))
    else {
        *state = None;
        return;
    };
    if !pipeline.motion_supported
        || gpu.instance_count == 0
        || gpu.frame_generation != frame.rig.frame_generation
        || gpu.instances.len() != frame.rig.instances.len()
    {
        *state = None;
        return;
    }
    if !gpu.spans.iter().any(|span| eligible(span.material)) {
        *state = None;
        return;
    }
    if state.as_ref().is_none_or(|state| {
        state.texture.size() != depth.texture().size()
            || state.texture.sample_count() != depth.texture().sample_count()
    }) {
        let Some(replacement) = MotionView::new(device, depth) else {
            *state = None;
            return;
        };
        *state = Some(replacement);
    }
    let state = state.as_mut().unwrap();
    state.moved = state
        .history
        .advance(&frame.rig, previous_camera, current_camera, continuous);
    state.ready = false;
    let Some(instances_changed) = state.instances.upload(
        device,
        bytemuck::cast_slice(&state.history.instances),
        |buffer, bytes| queue.profiled_write_buffer(buffer, 0, bytes),
    ) else {
        return;
    };
    if instances_changed {
        state.binding = None;
    }
    let Some(bones_changed) = state.bones.upload(
        device,
        bytemuck::cast_slice(&state.history.bones),
        |buffer, bytes| queue.profiled_write_buffer(buffer, 0, bytes),
    ) else {
        return;
    };
    if instances_changed || bones_changed || state.binding.is_none() {
        state.binding = gpu_allocation::checked(device, "actor velocity bindings", 0, || {
            device.create_bind_group(
                "previous rendered actor pose",
                &cache.get_bind_group_layout(&layout()),
                &[
                    BindGroupEntry {
                        binding: 0,
                        resource: state.instances.buffer.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: state.bones.buffer.as_entire_binding(),
                    },
                ],
            )
        });
        if state.binding.is_none() {
            return;
        }
    }
    state.draws.clear();
    state.ready = gpu.bind_group.is_some();
    let msaa = match depth.texture().sample_count() {
        2 => Msaa::Sample2,
        4 => Msaa::Sample4,
        8 => Msaa::Sample8,
        _ => Msaa::Off,
    };
    for span in &gpu.spans {
        if !eligible(span.material) {
            continue;
        }
        state.ready &= pipeline
            .draw_variant(msaa, hdr, enhanced, span.material)
            .is_some_and(|id| cache.get_render_pipeline(id).is_some());
        let key = ActorPipelineKey {
            msaa,
            hdr,
            enhanced,
            material: span.material,
        };
        let Some(id) = pipeline.motion_variant(key) else {
            state.ready = false;
            continue;
        };
        state.ready &= cache.get_render_pipeline(id).is_some();
        if span.page != 0 {
            state.ready &= gpu.artwork_current
                && gpu
                    .artwork
                    .page(usize::from(span.page) - 1)
                    .is_some_and(|page| page.bind_group.is_some());
        }
        state.draws.push((*span, id));
    }
    state.frame_generation = gpu.frame_generation;
}

#[cfg(test)]
mod fault_matrix_tests;
#[cfg(test)]
mod raster_tests;
#[cfg(test)]
mod recovery_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod viewport_raster_tests;

/// Fills covered actor samples without changing world depth, color or presentation witnesses.
pub(crate) fn draw(
    world: &World,
    state: &MotionView,
    view: Entity,
    depth: &ViewDepthStencilTexture,
    viewport: bevy::math::UVec4,
    context: &mut RenderContext,
) -> bool {
    let (Some(gpu), Some(lightmap), Some(offset), Some(binding)) = (
        world.get_resource::<ActorGpu>(),
        world.get_resource::<crate::lighting::LightmapGpu>(),
        world.get::<ViewUniformOffset>(view),
        state.binding.as_ref(),
    ) else {
        return false;
    };
    if !state.ready || gpu.frame_generation != state.frame_generation {
        return false;
    }
    let Some(rect) = crate::render_bounds::scissor(
        render_model::UiScissor::new(viewport.x, viewport.y, viewport.z, viewport.w),
        crate::render_bounds::extent(&state.view),
    ) else {
        return false;
    };
    let cache = world.resource::<PipelineCache>();
    let depth_range = world
        .get::<bevy::render::camera::ExtractedCamera>(view)
        .and_then(|camera| camera.viewport.as_ref())
        .map_or(0.0..1.0, |viewport| viewport.depth.clone());
    let attachments = [Some(RenderPassColorAttachment {
        view: &state.view,
        depth_slice: None,
        resolve_target: None,
        ops: wgpu::Operations {
            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
            store: wgpu::StoreOp::Store,
        },
    })];
    let mut attachment = depth.get_attachment(wgpu::StoreOp::Store);
    attachment.depth_ops = Some(wgpu::Operations {
        load: wgpu::LoadOp::Load,
        store: wgpu::StoreOp::Store,
    });
    let mut pass = context.begin_tracked_render_pass(RenderPassDescriptor {
        label: Some("rendered actor velocities"),
        color_attachments: &attachments,
        depth_stencil_attachment: Some(attachment),
        timestamp_writes: crate::gpu_timing::render_pass_timestamps(
            world,
            render_scene::RuntimeStage::GpuPost,
        ),
        occlusion_query_set: None,
        multiview_mask: None,
    });
    pass.set_viewport(
        rect.x as f32,
        rect.y as f32,
        rect.width as f32,
        rect.height as f32,
        depth_range.start,
        depth_range.end,
    );
    pass.set_bind_group(1, &lightmap.bind_group, &[]);
    pass.set_bind_group(2, binding, &[]);
    for (span, id) in &state.draws {
        let Some(pipeline) = cache.get_render_pipeline(*id) else {
            return false;
        };
        let binding = if span.page == 0 {
            gpu.bind_group.as_ref()
        } else {
            gpu.artwork
                .page(usize::from(span.page) - 1)
                .and_then(|page| page.bind_group.as_ref())
        };
        let Some(binding) = binding else {
            return false;
        };
        pass.set_render_pipeline(pipeline);
        pass.set_bind_group(0, binding, &[offset.offset]);
        pass.draw(0..span.vertex_count, span.first..span.first + span.count);
    }
    true
}
