use super::{
    CameraMotionBlur,
    history::{CameraHistory, ExposureUniform},
    pipeline::BlurPipeline,
};
use bevy::{
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
};
use std::mem::size_of;

#[derive(Component)]
pub(super) struct BlurView {
    history: CameraHistory,
    uniform: Buffer,
    last_uniform: Option<ExposureUniform>,
    bindings: Vec<(TextureViewId, TextureViewId, BindGroup)>,
    pub pipeline: CachedRenderPipelineId,
    pub active: bool,
}

impl BlurView {
    pub fn binding(&self, source: TextureViewId, depth: TextureViewId) -> Option<&BindGroup> {
        self.bindings
            .iter()
            .find(|(a, b, _)| (*a, *b) == (source, depth))
            .map(|(_, _, binding)| binding)
    }
}

type Views<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static CameraMotionBlur,
        &'static ExtractedView,
        &'static ViewTarget,
        &'static ViewDepthTexture,
        &'static Msaa,
        Option<&'static mut BlurView>,
    ),
>;

pub(super) fn prepare_views(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<BlurPipeline>,
    mut views: Views,
    removed: Query<
        Entity,
        (
            With<BlurView>,
            Or<(
                Without<CameraMotionBlur>,
                Without<ExtractedView>,
                Without<ViewTarget>,
            )>,
        ),
    >,
) {
    for entity in &removed {
        commands.entity(entity).remove::<BlurView>();
    }
    for (entity, settings, view, target, depth, msaa, previous) in &mut views {
        let pose = view.world_from_view.to_matrix();
        let clip = view
            .clip_from_world
            .unwrap_or_else(|| view.clip_from_view * pose.inverse());
        let history = CameraHistory::new(clip, pose, view.viewport, settings.reset_epoch);
        let id = pipeline.specialize(&cache, target.main_texture_format(), msaa.samples());
        let mut new_view;
        let state = if let Some(state) = previous {
            state.into_inner()
        } else {
            new_view = BlurView {
                history,
                uniform: device.create_buffer(&BufferDescriptor {
                    label: Some("camera exposure uniform"),
                    size: size_of::<ExposureUniform>() as u64,
                    usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                }),
                last_uniform: None,
                bindings: Vec::with_capacity(2),
                pipeline: id,
                active: false,
            };
            update(
                &mut new_view,
                history,
                *settings,
                target,
                depth,
                msaa.samples(),
                &device,
                &queue,
                &pipeline,
                &cache,
            );
            commands.entity(entity).insert(new_view);
            continue;
        };
        state.pipeline = id;
        update(
            state,
            history,
            *settings,
            target,
            depth,
            msaa.samples(),
            &device,
            &queue,
            &pipeline,
            &cache,
        );
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "view resources are borrowed together during preparation"
)]
fn update(
    state: &mut BlurView,
    history: CameraHistory,
    settings: CameraMotionBlur,
    target: &ViewTarget,
    depth: &ViewDepthTexture,
    samples: u32,
    device: &RenderDevice,
    queue: &RenderQueue,
    pipeline: &BlurPipeline,
    cache: &PipelineCache,
) {
    let uniform = state.history.advance(history, settings);
    state.active = uniform.strength.x > 0.0;
    if state.last_uniform != Some(uniform) {
        queue.write_buffer(&state.uniform, 0, bytemuck::bytes_of(&uniform));
        state.last_uniform = Some(uniform);
    }
    let source = target.main_texture_view();
    if state.binding(source.id(), depth.view().id()).is_none() {
        // Two ping-pong source views share a depth allocation; a resize retires both bindings.
        if state.bindings.len() == 2
            || state
                .bindings
                .first()
                .is_some_and(|(_, old, _)| *old != depth.view().id())
        {
            state.bindings.clear();
        }
        let binding = device.create_bind_group(
            "camera exposure",
            &cache.get_bind_group_layout(pipeline.layout(samples)),
            &[
                BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::TextureView(source),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::Sampler(&pipeline.sampler),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(depth.view()),
                },
                BindGroupEntry {
                    binding: 3,
                    resource: state.uniform.as_entire_binding(),
                },
            ],
        );
        state
            .bindings
            .push((source.id(), depth.view().id(), binding));
    }
}
