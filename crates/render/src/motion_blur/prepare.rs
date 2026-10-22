use super::{
    CameraMotionBlur,
    history::{CameraHistory, ExposureUniform},
    pipeline::BlurPipeline,
};
use crate::queue_writes::ProfiledQueue as _;
use bevy::ecs::system::SystemParam;
use bevy::{
    camera::MainPassResolutionOverride,
    prelude::*,
    render::{
        render_resource::*,
        renderer::{RenderDevice, RenderQueue},
        view::{ExtractedView, ViewDepthStencilTexture, ViewTarget},
    },
};
use std::mem::size_of;

/// Actor resources are absent in actorless scenes and lightweight renderer fixtures.
#[derive(SystemParam)]
pub(super) struct ActorResources<'w> {
    frame: Option<Res<'w, render_scene::ActorRenderFrame>>,
    gpu: Option<Res<'w, crate::actor_render::ActorGpu>>,
    pipeline: Option<Res<'w, crate::actor_render::pipeline::ActorPipeline>>,
}

#[derive(Component)]
pub(crate) struct BlurView {
    history: CameraHistory,
    uniform: Buffer,
    last_uniform: Option<ExposureUniform>,
    bindings: Vec<(TextureViewId, TextureViewId, TextureViewId, BindGroup)>,
    pub actors: Option<crate::actor_render::motion::MotionView>,
    pub pipeline: CachedRenderPipelineId,
    pub footprint_pipeline: CachedRenderPipelineId,
    pub row_pipeline: CachedRenderPipelineId,
    pub resolve_pipeline: CachedRenderPipelineId,
    pub(super) resolved: Option<super::resolve::ResolvedInputs>,
    pub(super) footprint: Option<super::footprint::Footprint>,
    /// The main-texture extent whose targets ran out of memory; blur stays off until a resize.
    unavailable_at: Option<Extent3d>,
    pub active: bool,
}

impl BlurView {
    pub fn viewport(&self) -> UVec4 {
        self.history.viewport()
    }

    pub fn binding(&self, source: TextureViewId, depth: TextureViewId) -> Option<&BindGroup> {
        self.bindings
            .iter()
            .find(|(a, b, _, _)| (*a, *b) == (source, depth))
            .map(|(_, _, _, binding)| binding)
    }
}

type Views<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static CameraMotionBlur,
        &'static ExtractedView,
        Option<&'static MainPassResolutionOverride>,
        &'static ViewTarget,
        &'static ViewDepthStencilTexture,
        &'static Msaa,
        Option<&'static render_scene::EnhancedRendering>,
        Option<&'static mut BlurView>,
    ),
>;

type RemovedViews<'w, 's> = Query<
    'w,
    's,
    Entity,
    (
        With<BlurView>,
        Or<(
            Without<CameraMotionBlur>,
            Without<ExtractedView>,
            Without<ViewTarget>,
        )>,
    ),
>;

#[expect(
    clippy::too_many_arguments,
    reason = "render resources and view queries are system parameters"
)]
pub(super) fn prepare_views(
    mut commands: Commands,
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    cache: Res<PipelineCache>,
    mut pipeline: ResMut<BlurPipeline>,
    actors: ActorResources,
    mut views: Views,
    removed: RemovedViews,
) {
    for entity in &removed {
        commands.entity(entity).remove::<BlurView>();
    }
    for (entity, settings, view, resolution, target, depth, msaa, enhanced, previous) in &mut views
    {
        let pose = view.world_from_view.to_matrix();
        let clip = view.clip_from_world.map_or(view.clip_from_view, |clip| {
            (clip.as_dmat4() * pose.as_dmat4()).as_mat4()
        });
        let mut viewport = view.viewport;
        if let Some(resolution) = resolution {
            viewport.z = resolution.0.x;
            viewport.w = resolution.0.y;
        }
        let history = CameraHistory::new(clip, pose, viewport, settings.reset_epoch);
        let id = pipeline.specialize(&cache, target.main_texture_format(), msaa.samples());
        let [row_pipeline, footprint_pipeline] = pipeline.footprint(&cache);
        let resolve_pipeline = pipeline.resolve(&cache, msaa.samples());
        let neutral_motion = pipeline.neutral_motion(&device, msaa.samples());
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
                actors: None,
                pipeline: id,
                footprint_pipeline,
                row_pipeline,
                resolve_pipeline,
                resolved: None,
                footprint: None,
                unavailable_at: None,
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
                &actors,
                enhanced.is_some(),
                &neutral_motion,
            );
            commands.entity(entity).insert(new_view);
            continue;
        };
        state.pipeline = id;
        state.footprint_pipeline = footprint_pipeline;
        state.row_pipeline = row_pipeline;
        state.resolve_pipeline = resolve_pipeline;
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
            &actors,
            enhanced.is_some(),
            &neutral_motion,
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
    depth: &ViewDepthStencilTexture,
    samples: u32,
    device: &RenderDevice,
    queue: &RenderQueue,
    pipeline: &BlurPipeline,
    cache: &PipelineCache,
    actors: &ActorResources,
    enhanced: bool,
    neutral_motion: &TextureView,
) {
    crate::actor_render::motion::prepare(
        &mut state.actors,
        actors.frame.as_deref(),
        actors.gpu.as_deref(),
        actors.pipeline.as_deref(),
        state.history,
        history,
        state.history.continuous(history, settings),
        depth,
        target.main_texture_format() == render_scene::SCENE_HDR_FORMAT,
        enhanced,
        device,
        queue,
        cache,
    );
    let actor_moved = state.actors.as_ref().is_some_and(|actors| actors.moved);
    let mut uniform = if actor_moved {
        state
            .history
            .advance_with_actor_motion(history, settings, true)
    } else {
        state.history.advance(history, settings)
    };
    let actor_ready = state.actors.as_ref().is_none_or(|actors| actors.ready);
    if !actor_ready {
        uniform.strength.x = 0.0;
    }
    uniform.strength.z = if state.actors.is_some() { 1.0 } else { 0.0 };
    state.active = uniform.strength.x > 0.0;
    if state.last_uniform != Some(uniform) {
        queue.profiled_write_buffer(&state.uniform, 0, bytemuck::bytes_of(&uniform));
        state.last_uniform = Some(uniform);
    }
    let motion = state
        .actors
        .as_ref()
        .map_or(neutral_motion, |actors| &actors.view);
    let size = target.main_texture().size();
    if state.unavailable_at == Some(size) {
        state.active = false;
        return;
    }
    if super::resolve::ResolvedInputs::prepare(
        &mut state.resolved,
        device,
        cache,
        pipeline.resolve_layout(samples),
        &state.uniform,
        (crate::scene_sampling::view_depth(depth), motion),
        size,
    ) {
        state.bindings.clear();
    }
    if let Some(resolved) = &state.resolved
        && super::footprint::Footprint::prepare(
            &mut state.footprint,
            device,
            cache,
            pipeline.footprint_layouts(),
            &state.uniform,
            &resolved.motion,
            size,
        )
    {
        state.bindings.clear();
    }
    if state.resolved.is_none() || state.footprint.is_none() {
        // Out of memory: the frame keeps its unblurred scene.
        state.resolved = None;
        state.footprint = None;
        state.unavailable_at = Some(size);
        state.active = false;
        return;
    }
    state.unavailable_at = None;
    let resolved = state.resolved.as_ref().unwrap();
    let footprint = &state.footprint.as_ref().unwrap().view;
    if state
        .bindings
        .first()
        .is_some_and(|(_, _, old, _)| *old != motion.id())
    {
        state.bindings.clear();
    }
    for source in [target.main_texture_view(), target.main_texture_other_view()] {
        if state
            .binding(source.id(), crate::scene_sampling::view_depth(depth).id())
            .is_none()
        {
            // Bind both scene colours before SMAA can switch them; resizing retires both bindings.
            if state.bindings.len() == 2
                || state.bindings.first().is_some_and(|(_, old, _, _)| {
                    *old != crate::scene_sampling::view_depth(depth).id()
                })
            {
                state.bindings.clear();
            }
            let binding = device.create_bind_group(
                "camera exposure",
                &cache.get_bind_group_layout(pipeline.layout()),
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
                        binding: 3,
                        resource: state.uniform.as_entire_binding(),
                    },
                    BindGroupEntry {
                        binding: 5,
                        resource: BindingResource::TextureView(footprint),
                    },
                    BindGroupEntry {
                        binding: 6,
                        resource: BindingResource::TextureView(&resolved.motion),
                    },
                    BindGroupEntry {
                        binding: 7,
                        resource: BindingResource::TextureView(&resolved.depth),
                    },
                ],
            );
            state.bindings.push((
                source.id(),
                crate::scene_sampling::view_depth(depth).id(),
                motion.id(),
                binding,
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::{camera::MainPassResolutionOverride, ecs::system::RunSystemOnce};

    #[path = "composed_tests.rs"]
    mod composed_tests;

    #[test]
    fn motion_blur_uses_the_main_pass_viewport_and_resets_after_resolution_changes() {
        let (mut app, entity) = super::super::resource_tests::fixture();
        let world = app.world_mut();
        let viewport = UVec4::new(30, 40, 640, 360);
        world.get_mut::<ExtractedView>(entity).unwrap().viewport = viewport;
        world.entity_mut(entity).insert((
            CameraMotionBlur {
                exposure_seconds: 0.01,
                delta_seconds: 0.01,
                samples: 7,
                reset_epoch: 0,
            },
            MainPassResolutionOverride(UVec2::new(320, 180)),
        ));
        world.run_system_once(prepare_views).unwrap();
        let state = world.get::<BlurView>(entity).unwrap();
        let exposure = state.last_uniform.unwrap();
        assert_eq!(exposure.viewport, Vec4::new(30.0, 40.0, 320.0, 180.0));
        assert_eq!(state.viewport().as_vec4(), exposure.viewport);
        assert_eq!(exposure.strength.x, 0.0);

        for (yaw, expected_strength) in [(0.1, 1.0), (0.2, 0.0), (0.3, 1.0)] {
            if yaw == 0.2 {
                world
                    .entity_mut(entity)
                    .insert(MainPassResolutionOverride(UVec2::new(160, 90)));
            }
            let mut view = world.get_mut::<ExtractedView>(entity).unwrap();
            view.world_from_view =
                GlobalTransform::from(Transform::from_rotation(Quat::from_rotation_y(yaw)));
            view.clip_from_world = None;
            world.run_system_once(prepare_views).unwrap();
            let state = world.get::<BlurView>(entity).unwrap();
            let exposure = state.last_uniform.unwrap();
            assert_eq!(state.viewport().as_vec4(), exposure.viewport);
            assert_eq!(exposure.strength.x, expected_strength);
            assert_eq!(
                exposure.viewport,
                if yaw == 0.1 {
                    Vec4::new(30.0, 40.0, 320.0, 180.0)
                } else {
                    Vec4::new(30.0, 40.0, 160.0, 90.0)
                }
            );
        }

        world
            .entity_mut(entity)
            .remove::<MainPassResolutionOverride>();
        world.run_system_once(prepare_views).unwrap();
        let exposure = world.get::<BlurView>(entity).unwrap().last_uniform.unwrap();
        assert_eq!(exposure.viewport, viewport.as_vec4());
        assert_eq!(exposure.strength.x, 0.0);
    }
}
