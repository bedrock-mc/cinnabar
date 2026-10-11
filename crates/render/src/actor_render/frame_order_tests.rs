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
        transparent.prepare_for_new_frame(view.retained_view_entity);
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
        .values()
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
    for (prepared, source) in gpu.instances.iter().zip(frame.rig.instances.iter()) {
        let mut prepared = *prepared;
        prepared.material &= !crate::actor::material::LATE_DISSOLVE_COLOR;
        assert_eq!(&prepared, source);
    }
    assert!(gpu.bind_group.is_some(), "bindings are ready for execution");
    let expected_ranges = (0..gpu.sorted.ranges.len())
        .map(|index| index as u32..index as u32 + 1)
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
        .filter(|span| has_opaque && !super::super::phase::sorted(span.material))
        .collect::<Vec<_>>();
    for range in queued_ranges {
        for draw in &gpu.sorted.ranges[range.start as usize..range.end as usize] {
            executed.extend(
                gpu.sorted.indices[draw.clone()]
                    .iter()
                    .map(|&index| gpu.spans[index]),
            );
        }
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
        .init_resource::<crate::WorldFullbright>()
        .init_resource::<crate::NametagSceneResource>()
        .init_resource::<ExecutedFrames>()
        .add_systems(Render, reset_phases.in_set(RenderSystems::PrepareViews))
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

#[test]
fn always_depth_dissolve_mask_queues_before_its_equal_depth_color() {
    use bevy::render::render_phase::SortedPhaseItem;
    let mut app = render_app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let mut frame = frame(&[false; 2]);
    let instances = Arc::make_mut(&mut frame.rig.instances);
    instances[0].material =
        assets::EntityRenderMaterial::DissolveDepth.word(Some(assets::EntityRenderMaterialState {
            depth_always: true,
            ..Default::default()
        }));
    instances[1].material = assets::EntityRenderMaterial::DissolveColor as u32;
    let manifest = Arc::make_mut(&mut frame.rig.manifest);
    manifest[1].identity = ActorRenderIdentity {
        layer: u8::MAX,
        ..manifest[0].identity
    };
    world.insert_resource(frame);
    world.run_schedule(Render);
    let retained = world
        .query::<&ExtractedView>()
        .single(world)
        .unwrap()
        .retained_view_entity;
    let mut phases = world.resource_mut::<ViewSortedRenderPhases<Transparent3d>>();
    let items = &mut phases.get_mut(&retained).unwrap().items;
    assert_eq!(items.len(), 1, "mask and color must share one draw item");
    Transparent3d::sort(items);
    let ranges = items
        .values()
        .map(|item| {
            let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } = &item.extra_index
            else {
                panic!("missing draw span");
            };
            range.clone()
        })
        .collect::<Vec<_>>();
    assert_eq!(ranges.len(), 1, "the pair remains one item while sorting");
    assert_eq!(ranges[0], 0..1);
    assert_eq!(world.resource::<ActorGpu>().sorted.indices, [0, 1]);
}

#[test]
fn an_ordinary_actors_dissolve_color_is_not_promoted_by_another_actors_mask() {
    let mut app = render_app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let mut frame = frame(&[false; 4]);
    let instances = Arc::make_mut(&mut frame.rig.instances);
    instances[0].material =
        assets::EntityRenderMaterial::DissolveDepth.word(Some(assets::EntityRenderMaterialState {
            depth_always: true,
            ..Default::default()
        }));
    instances[1].material = assets::EntityRenderMaterial::DissolveColor as u32;
    instances[2].material = assets::EntityRenderMaterial::DissolveDepth as u32;
    instances[3].material = assets::EntityRenderMaterial::DissolveColor as u32;
    let manifest = Arc::make_mut(&mut frame.rig.manifest);
    manifest[1].identity = ActorRenderIdentity {
        layer: u8::MAX,
        ..manifest[0].identity
    };
    manifest[3].identity = ActorRenderIdentity {
        layer: u8::MAX,
        ..manifest[2].identity
    };
    world.insert_resource(frame);
    world.run_schedule(Render);
    let gpu = world.resource::<ActorGpu>();
    assert!(super::super::phase::sorted(gpu.instances[0].material));
    assert!(super::super::phase::sorted(gpu.instances[1].material));
    assert!(!super::super::phase::sorted(gpu.instances[2].material));
    assert!(!super::super::phase::sorted(gpu.instances[3].material));
}

#[test]
fn a_depth_writing_blend_cannot_split_a_sorted_dissolve_pair() {
    let mut app = render_app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let mut frame = frame(&[false, true, false]);
    let instances = Arc::make_mut(&mut frame.rig.instances);
    instances[0].material =
        assets::EntityRenderMaterial::DissolveDepth.word(Some(assets::EntityRenderMaterialState {
            depth_always: true,
            ..Default::default()
        }));
    instances[2].material = assets::EntityRenderMaterial::DissolveColor as u32;
    let manifest = Arc::make_mut(&mut frame.rig.manifest);
    manifest[2].identity = ActorRenderIdentity {
        layer: u8::MAX,
        ..manifest[0].identity
    };
    world.insert_resource(frame);
    world.run_schedule(Render);
    let retained = world
        .query::<&ExtractedView>()
        .single(world)
        .unwrap()
        .retained_view_entity;
    let phases = world.resource::<ViewSortedRenderPhases<Transparent3d>>();
    assert_eq!(
        phases.get(&retained).unwrap().items.len(),
        2,
        "mask and color must be one draw item before the equal-distance blend"
    );
    use bevy::render::render_phase::SortedPhaseItem;
    let order = {
        let mut phases = world.resource_mut::<ViewSortedRenderPhases<Transparent3d>>();
        Transparent3d::sort(&mut phases.get_mut(&retained).unwrap().items);
        phases
            .get(&retained)
            .unwrap()
            .items
            .values()
            .map(|item| {
                let PhaseItemExtraIndex::IndirectParametersIndex { range, .. } = &item.extra_index
                else {
                    panic!("missing draw plan");
                };
                range.start as usize
            })
            .collect::<Vec<_>>()
    };
    let gpu = world.resource::<ActorGpu>();
    let spans = order
        .iter()
        .flat_map(|&index| {
            gpu.sorted.indices[gpu.sorted.ranges[index].clone()]
                .iter()
                .copied()
        })
        .collect::<Vec<_>>();
    assert_eq!(
        spans,
        [0, 2, 1],
        "the color finishes before the intervening blend can write depth"
    );
}
