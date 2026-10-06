use std::sync::Arc;

use bevy::{
    app::SubApp,
    asset::Assets,
    core_pipeline::core_3d::{Opaque3d, Transparent3d},
    diagnostic::FrameCount,
    ecs::{schedule::Schedule, system::RunSystemOnce},
    prelude::*,
    render::{
        ExtractSchedule, Render, RenderApp, RenderStartup, RenderSystems,
        batching::gpu_preprocessing::GpuPreprocessingMode,
        render_phase::{
            DrawFunctions, PhaseItemExtraIndex, ViewBinnedRenderPhases, ViewSortedRenderPhases,
        },
        view::{ExtractedView, ViewUniforms, prepare_view_uniforms},
    },
};
use render_model::{ActorRigVertex, EntityRigId};

use super::super::{
    ActorGpu, ActorRenderPlugin, DrawActorCommands, DrawTransparentActorCommands,
    prepare_actor_bind_group, prepare_actor_resources, submit_actor_presented_frame,
};
use crate::actor::{
    ActorDrawManifestEntry, ActorRenderFrame, ActorRenderIdentity, ActorRigGeometrySpan,
    ActorRigRoute, gpu::ActorDrawTracker,
};

#[derive(Resource, Default)]
struct ExecutedFrames(Vec<(u64, Vec<u32>)>);

fn reset_phases(
    views: Query<&ExtractedView>,
    mut opaque: ResMut<ViewBinnedRenderPhases<Opaque3d>>,
    mut transparent: ResMut<ViewSortedRenderPhases<Transparent3d>>,
) {
    for view in &views {
        opaque.prepare_for_new_frame(view.retained_view_entity, GpuPreprocessingMode::None);
        transparent.insert_or_clear(view.retained_view_entity);
    }
}

// Replay the queued ranges through the same span ledger without a graphics adapter.
fn inspect_executed_frame(world: &mut World) {
    let (view_entity, view) = world
        .query::<(Entity, &ExtractedView)>()
        .single(world)
        .unwrap();
    let retained = view.retained_view_entity;
    let transparent_draw = world
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .id::<DrawTransparentActorCommands>();
    let opaque_draw = world
        .resource::<DrawFunctions<Opaque3d>>()
        .read()
        .id::<DrawActorCommands>();
    let queued_ranges = world
        .resource::<ViewSortedRenderPhases<Transparent3d>>()
        .get(&retained)
        .unwrap()
        .items
        .iter()
        .filter(|item| item.draw_function == transparent_draw)
        .map(|item| {
            let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } = &item.extra_index
            else {
                panic!("actor draw must retain its prepared span range");
            };
            assert_eq!(item.batch_range, 0..1);
            range.clone()
        })
        .collect::<Vec<_>>();
    let gpu = world.resource::<ActorGpu>();
    let frame = world.resource::<ActorRenderFrame>();
    assert_eq!(gpu.frame_generation, frame.rig.frame_generation);
    assert_eq!(gpu.instances.as_ref(), frame.rig.instances.as_ref());
    assert!(gpu.bind_group.is_some(), "bindings are ready for execution");
    let expected_ranges = gpu
        .spans
        .iter()
        .enumerate()
        .filter(|(_, span)| {
            crate::actor::material::state(span.material).is_some_and(|state| state.blend)
        })
        .map(|(index, _)| index as u32..index as u32 + 1)
        .collect::<Vec<_>>();
    assert_eq!(queued_ranges, expected_ranges);
    let has_opaque = world
        .resource::<ViewBinnedRenderPhases<Opaque3d>>()
        .get(&retained)
        .unwrap()
        .non_mesh_items
        .keys()
        .any(|(key, _)| key.draw_function == opaque_draw);
    let mut executed = gpu
        .spans
        .iter()
        .copied()
        .filter(|span| {
            has_opaque
                && !crate::actor::material::state(span.material).is_some_and(|state| state.blend)
        })
        .collect::<Vec<_>>();
    for range in queued_ranges {
        executed.extend_from_slice(&gpu.spans[range.start as usize..range.end as usize]);
    }
    let mut indices = executed
        .iter()
        .flat_map(|span| span.first..span.first + span.count)
        .collect::<Vec<_>>();
    indices.sort_unstable();
    assert_eq!(indices, (0..gpu.instance_count).collect::<Vec<_>>());
    let tracker = world.resource::<ActorDrawTracker>();
    for span in executed {
        tracker.record_draw(view_entity.to_bits(), span);
    }
    let drawn = tracker
        .take_drawn()
        .expect("all current spans were executed");
    assert_eq!(drawn.frame_generation, frame.rig.frame_generation);
    assert_eq!(drawn.manifest.as_ref(), frame.rig.manifest.as_ref());
    assert!(drawn.is_exact());
    world
        .resource_mut::<ExecutedFrames>()
        .0
        .push((drawn.frame_generation, indices));
}

fn render_app() -> App {
    let (mut app, _) = crate::queue_review_support::app();
    let mut render = SubApp::new();
    *render.world_mut() = std::mem::take(app.world_mut());
    render
        .add_schedule(Schedule::new(RenderStartup))
        .add_schedule(Render::base_schedule())
        .add_schedule(Schedule::new(ExtractSchedule))
        .init_resource::<DrawFunctions<Opaque3d>>()
        .init_resource::<ViewBinnedRenderPhases<Opaque3d>>()
        .init_resource::<ViewUniforms>()
        .init_resource::<FrameCount>()
        .init_resource::<crate::WorldLighting>()
        .init_resource::<crate::NametagSceneResource>()
        .init_resource::<ExecutedFrames>()
        .add_systems(Render, reset_phases.in_set(RenderSystems::ManageViews))
        .add_systems(
            Render,
            prepare_view_uniforms.in_set(RenderSystems::PrepareResources),
        )
        .add_systems(
            Render,
            inspect_executed_frame
                .in_set(RenderSystems::Render)
                .before(submit_actor_presented_frame),
        );
    app.insert_resource(Assets::<Shader>::default());
    app.insert_sub_app(RenderApp, render);
    app.add_plugins(ActorRenderPlugin);
    app.finish();
    app.sub_app_mut(RenderApp)
        .world_mut()
        .run_schedule(RenderStartup);
    app
}

fn frame(blended: &[bool]) -> ActorRenderFrame {
    let mut frame = ActorRenderFrame::default();
    frame.rig.frame_generation = 1;
    frame.rig.geometry_revision = 1;
    frame.rig.maximum_vertex_count = 3;
    frame.rig.instances = blended
        .iter()
        .map(|&blend| crate::actor::ActorGpuInstance {
            material: assets::EntityRenderMaterial::Default.word(Some(
                assets::EntityRenderMaterialState {
                    blend,
                    ..Default::default()
                },
            )),
            ..Default::default()
        })
        .collect::<Vec<_>>()
        .into();
    frame.instance_pages = vec![1; blended.len()].into();
    frame.rig.previous_bones = Arc::from([[
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ]]);
    frame.rig.current_bones = Arc::clone(&frame.rig.previous_bones);
    frame.rig.manifest = (0..blended.len())
        .map(|index| ActorDrawManifestEntry {
            identity: ActorRenderIdentity {
                session_id: 1,
                dimension: 0,
                runtime_id: index as u64 + 10,
                spawn_revision: 1,
                ingress_sequence: 1,
                source_tick: None,
                movement_revision: 0,
                pose_generation: 1,
                layer: 0,
            },
            rig: EntityRigId(0),
            completed_tick: 1,
            reset_generation: 1,
            route: ActorRigRoute::Compiled,
            instance_index: index as u32,
            previous_bone_base: 0,
            current_bone_base: 0,
            bone_count: 1,
        })
        .collect::<Vec<_>>()
        .into();
    frame.rig.geometry_vertices =
        crate::actor::ActorRigVertexSegments::from_vertices([ActorRigVertex::default(); 3]);
    frame.rig.geometry_spans = Arc::from([ActorRigGeometrySpan {
        first_vertex: 0,
        vertex_count: 3,
    }]);
    let mut artwork = crate::actor::ActorArtworkPages::default();
    artwork.identity = [1; 32];
    artwork.entity_identity = [2; 32];
    artwork.pages = Arc::from([crate::actor::ActorTexturePage {
        width: 16,
        height: 16,
        layers: 1,
        color_mask: false,
        multitexture: false,
        rgba8: vec![255; 1024].into(),
    }]);
    frame.artwork = Arc::new(artwork);
    frame
}

#[test]
fn actor_render_first_frame_queues_before_bind_groups_are_prepared() {
    let mut app = render_app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    assert!(world.resource::<ActorGpu>().bind_group.is_none());
    world.insert_resource(frame(&[false, true]));
    world.run_schedule(Render);
    assert_eq!(world.resource::<ExecutedFrames>().0, [(1, vec![0, 1])]);
}

#[test]
fn actor_render_changed_spans_queue_and_present_the_current_frame() {
    let mut app = render_app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let mut frame = frame(&[false, false, true]);
    world.insert_resource(frame.clone());
    world.run_system_once(prepare_actor_resources).unwrap();
    world.run_system_once(prepare_view_uniforms).unwrap();
    world.run_system_once(prepare_actor_bind_group).unwrap();
    assert!(world.resource::<ActorGpu>().bind_group.is_some());
    assert_eq!(world.resource::<ActorGpu>().spans.len(), 2);

    frame.rig.frame_generation = 2;
    let blended_material = frame.rig.instances[2].material;
    Arc::make_mut(&mut frame.rig.instances)[0].material = blended_material;
    for entry in Arc::make_mut(&mut frame.rig.manifest) {
        entry.completed_tick = 2;
        entry.identity.pose_generation = 2;
    }
    world.insert_resource(frame);
    world.run_schedule(Render);
    assert_eq!(world.resource::<ActorGpu>().spans.len(), 3);
    assert_eq!(world.resource::<ExecutedFrames>().0, [(2, vec![0, 1, 2])]);
}
