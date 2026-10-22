use bevy::ecs::system::RunSystemOnce;
use render_api::PoseGeneration;
use std::sync::Arc;

use bevy::{
    app::SubApp,
    asset::Assets,
    core_pipeline::core_3d::{Opaque3d, Transparent3d},
    diagnostic::FrameCount,
    ecs::schedule::Schedule,
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
use {
    crate::actor::gpu::{ActorDrawBatch, ActorDrawSpan, ActorDrawTracker},
    render_model::{
        ActorDrawManifestEntry, ActorRenderIdentity, ActorRigGeometrySpan, ActorRigRoute,
    },
    render_scene::ActorRenderFrame,
};

#[derive(Resource, Default)]
struct ExecutedFrames(Vec<(u64, Vec<u32>)>);

#[derive(Resource, Default)]
struct PreviousReceipt(Option<(ActorDrawBatch, Vec<ActorDrawSpan>)>);

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

/// Replays queued command partitions and checks their immutable receipt handles without a GPU.
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
    for (index, (prepared, source)) in gpu
        .instances
        .iter()
        .zip(frame.rig.instances.iter())
        .enumerate()
    {
        let mut receipt = gpu.manifest[index].clone();
        assert_eq!(receipt.previous_bone_base, prepared.previous_bone_base);
        assert_eq!(receipt.current_bone_base, prepared.current_bone_base);
        receipt.previous_bone_base = frame.rig.manifest[index].previous_bone_base;
        receipt.current_bone_base = frame.rig.manifest[index].current_bone_base;
        assert_eq!(receipt, frame.rig.manifest[index]);
        let mut prepared = *prepared;
        prepared.previous_bone_base = source.previous_bone_base;
        prepared.current_bone_base = source.current_bone_base;
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
    let batch = gpu
        .draw_batch(view_entity.to_bits())
        .expect("queued identity matches prepared data");
    let mut commands = Vec::new();
    let opaque = gpu
        .spans
        .iter()
        .copied()
        .filter(|span| has_opaque && !super::super::phase::sorted(span.material))
        .collect::<Vec<_>>();
    if !opaque.is_empty() {
        commands.push(opaque);
    }
    for range in queued_ranges {
        let mut executed = Vec::new();
        for draw in &gpu.sorted.ranges[range.start as usize..range.end as usize] {
            executed.extend(
                gpu.sorted.indices[draw.clone()]
                    .iter()
                    .map(|&index| gpu.spans[index]),
            );
        }
        commands.push(executed);
    }
    let executed: Vec<_> = commands.iter().flatten().copied().collect();
    let mut indices = executed
        .iter()
        .flat_map(|span| span.first..span.first + span.count)
        .collect::<Vec<_>>();
    indices.sort_unstable();
    assert_eq!(
        indices,
        gpu.main_manifest
            .iter()
            .map(|entry| entry.instance_index)
            .collect::<Vec<_>>()
    );
    let tracker = world.resource::<ActorDrawTracker>();
    if let Some((old, spans)) = &world.resource::<PreviousReceipt>().0 {
        assert!(
            !tracker.record_batch(old, view_entity.to_bits(), spans),
            "prior commands cannot credit the new queue"
        );
    }
    for command in &commands {
        assert!(tracker.record_batch(&batch, view_entity.to_bits(), command));
    }
    let drawn = tracker
        .take_drawn()
        .expect("all current spans were executed");
    assert_eq!(drawn.frame_generation, frame.rig.frame_generation);
    assert_eq!(drawn.manifest.as_ref(), gpu.main_manifest.as_ref());
    assert!(drawn.is_exact());
    world.resource_mut::<PreviousReceipt>().0 = Some((batch, executed));
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
        .init_resource::<render_scene::WorldLighting>()
        .init_resource::<render_scene::WorldFullbright>()
        .init_resource::<render_scene::NametagSceneResource>()
        .init_resource::<ExecutedFrames>()
        .init_resource::<PreviousReceipt>()
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
        .map(|&blend| render_model::ActorGpuInstance {
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
                pose_generation: PoseGeneration::new(1),
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
        render_model::ActorRigVertexSegments::from_vertices([ActorRigVertex::default(); 3]);
    frame.rig.geometry_spans = Arc::from([ActorRigGeometrySpan {
        first_vertex: 0,
        vertex_count: 3,
    }]);
    let mut artwork = render_model::ActorArtworkPages::default();
    artwork.identity = [1; 32];
    artwork.entity_identity = [2; 32];
    artwork.pages = Arc::from([render_model::ActorTexturePage {
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
fn distinct_equal_count_rigs_keep_layer_boundaries_and_receipts() {
    let mut app = render_app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let mut frame = frame(&[false; 4]);
    frame.rig.geometry_spans = (0..4)
        .map(|index| ActorRigGeometrySpan {
            first_vertex: index * 3,
            vertex_count: 3,
        })
        .collect::<Vec<_>>()
        .into();
    frame.rig.geometry_vertices =
        render_model::ActorRigVertexSegments::from_vertices([ActorRigVertex::default(); 12]);
    for (index, (instance, entry)) in Arc::make_mut(&mut frame.rig.instances)
        .iter_mut()
        .zip(Arc::make_mut(&mut frame.rig.manifest))
        .enumerate()
    {
        instance.geometry_id = 3 - index as u32;
        entry.rig = EntityRigId(instance.geometry_id);
        entry.identity.layer = (index / 2) as u8;
    }
    world.insert_resource(frame.clone());
    world.run_schedule(Render);
    assert_eq!(
        world.resource::<ExecutedFrames>().0,
        [(1, vec![0, 1, 2, 3])]
    );
    let gpu = world.resource::<ActorGpu>();
    assert_eq!(gpu.spans.len(), 2);
    assert_eq!((gpu.spans[0].first, gpu.spans[0].count), (0, 2));
    assert_eq!((gpu.spans[1].first, gpu.spans[1].count), (2, 2));
    assert_eq!(
        gpu.manifest
            .iter()
            .map(|entry| entry.identity)
            .collect::<Vec<_>>(),
        frame
            .rig
            .manifest
            .iter()
            .map(|entry| entry.identity)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        gpu.instances
            .iter()
            .map(|instance| instance.geometry_id)
            .collect::<Vec<_>>(),
        [3, 2, 1, 0]
    );
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
        entry.identity.pose_generation = PoseGeneration::new(2);
    }
    world.insert_resource(frame);
    world.run_schedule(Render);
    assert_eq!(world.resource::<ActorGpu>().spans.len(), 3);
    assert_eq!(world.resource::<ExecutedFrames>().0, [(2, vec![0, 1, 2])]);
}

#[test]
fn queued_batch_capture_rejects_wrong_views_and_replaced_prepared_revisions() {
    let mut app = render_app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let mut frame = frame(&[false, true]);
    for generation in [1, 2] {
        frame.rig.frame_generation = generation;
        world.insert_resource(frame.clone());
        world.run_schedule(Render);
    }
    assert_eq!(
        world.resource::<ExecutedFrames>().0,
        [(1, vec![0, 1]), (2, vec![0, 1])]
    );
    let view = world
        .query_filtered::<Entity, With<ExtractedView>>()
        .single(world)
        .unwrap()
        .to_bits();
    {
        let mut gpu = world.resource_mut::<ActorGpu>();
        assert!(gpu.draw_batch(view).is_some());
        assert!(gpu.draw_batch(view ^ 1).is_none());
        gpu.artwork_identity[0] ^= 1;
        assert!(gpu.draw_batch(view).is_none());
        gpu.artwork_identity[0] ^= 1;
        gpu.skin_revision += 1;
        assert!(gpu.draw_batch(view).is_none());
        gpu.skin_revision -= 1;
        gpu.geometry_revision += 1;
        assert!(gpu.draw_batch(view).is_none());
        gpu.geometry_revision -= 1;
        gpu.frame_generation += 1;
        assert!(gpu.draw_batch(view).is_none());
        gpu.frame_generation -= 1;
        let manifest = Arc::clone(&gpu.main_manifest);
        gpu.main_manifest = gpu.main_manifest.to_vec().into();
        assert!(
            gpu.draw_batch(view).is_none(),
            "equal manifest contents are a distinct prepared identity"
        );
        gpu.main_manifest = manifest;
        assert!(gpu.draw_batch(view).is_some());
    }
    world.run_system_once(prepare_actor_resources).unwrap();
    assert!(
        world.resource::<ActorGpu>().draw_batch(view).is_none(),
        "preparation retires the queued handle"
    );

    let mut state = bevy::ecs::system::SystemState::<(
        super::super::phase::QueueActorParams<'_, '_>,
        Local<u64>,
    )>::new(world);
    let (params, mut generation) = state.get_mut(world).unwrap();
    *generation = u64::MAX;
    super::super::phase::queue_actors(params, generation);
    assert!(world.resource::<ActorGpu>().draw_batch(view).is_none());
    assert!(
        world.resource::<ActorDrawTracker>().take_drawn().is_none(),
        "saturated queue generation fails closed"
    );
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

#[test]
fn actor_instances_and_both_bone_endpoints_take_one_pool_lock() {
    use crate::upload_staging::BufferUploadStaging;
    let mut app = render_app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    let staging = BufferUploadStaging::new(world.resource());
    world.insert_resource(staging);
    world.insert_resource(frame(&[false, false, true]));
    let before = world.resource::<BufferUploadStaging>().stats();
    world.run_system_once(prepare_actor_resources).unwrap();
    let after = world.resource::<BufferUploadStaging>().stats();
    assert_eq!(after.staged_writes - before.staged_writes, 3);
    assert_eq!(after.batches - before.batches, 1);
    assert_eq!(after.fallback_writes, 0);
}

#[test]
fn coverage_only_suffix_keeps_exact_receipts_for_only_the_visible_main_pass() {
    use render_model::{
        ActorRigGeometry, ActorRigRenderInput, ActorRigSubmission, RenderBoneTransform,
    };
    let mut scene = render_scene::ActorRenderScene::default();
    let fixture = frame(&[false]);
    let rasters: Vec<_> = [16u16, 32]
        .into_iter()
        .map(|size| render_model::EquipmentRaster {
            width: size,
            height: size,
            rgba8: vec![255; usize::from(size) * usize::from(size) * 4].into(),
        })
        .collect();
    let (artwork, locations) =
        render_model::ActorArtworkPages::default().with_equipment_rasters(&rasters);
    scene.configure_artwork(artwork);
    for id in [100, 101] {
        scene
            .insert_geometry(
                ActorRigGeometry::new(
                    render_model::equipment_rig_id(id),
                    vec![
                        ActorRigVertex {
                            normal: [0., 1., 0.],
                            ..Default::default()
                        };
                        3
                    ],
                    vec![[0.; 3]],
                )
                .unwrap(),
            )
            .unwrap();
    }
    let pose: Arc<[_]> =
        Arc::from([
            RenderBoneTransform::from_model_space([0., 0., 0., 1.], [0., 0., 0., 1.]).unwrap(),
        ]);
    let mut assignments = std::collections::HashMap::new();
    let submissions: Vec<_> = [(1, 0, 100, 1), (2, 0, 101, 2), (2, 1, 100, 1)]
        .into_iter()
        .map(|(id, layer, rig, page)| {
            let identity = ActorRenderIdentity {
                runtime_id: id,
                layer,
                ..fixture.rig.manifest[0].identity
            };
            assignments.insert(identity, locations[page - 1].unwrap());
            ActorRigSubmission {
                input: ActorRigRenderInput {
                    identity,
                    rig: render_model::equipment_rig_id(rig),
                    previous_bones: Arc::clone(&pose),
                    current_bones: Arc::clone(&pose),
                    completed_tick: 1,
                    reset_generation: 1,
                },
                material: Default::default(),
                culling_bounds: Default::default(),
                world_from_actor: [
                    [1., 0., 0., id as f32],
                    [0., 1., 0., 0.],
                    [0., 0., 1., layer as f32],
                ],
                texture_layer: 0,
                route: ActorRigRoute::Compiled,
                tint: 0,
                overlay_rgba8: 0,
                uv_anim: [0., 0., 1., 1.],
                light: 0,
            }
        })
        .collect();
    let unpartitioned = scene
        .update_rigs_with_artwork(1., None, submissions, &[], &assignments)
        .clone();
    assert_eq!(unpartitioned.rig.manifest.len(), 3);
    assert_eq!(
        unpartitioned.rig.manifest[0].identity.runtime_id, 1,
        "the builder overrides submission order"
    );
    let source = scene.set_mask_only(vec![1]).clone();
    for (index, entry) in source.rig.manifest.iter().enumerate() {
        assert_eq!(entry.instance_index as usize, index);
        let original = unpartitioned
            .rig
            .manifest
            .iter()
            .position(|old| old.identity == entry.identity)
            .unwrap();
        assert_eq!(
            source.rig.instances[index],
            unpartitioned.rig.instances[original]
        );
        assert_eq!(source.instances[index], unpartitioned.instances[original]);
        assert_eq!(
            source.instance_pages[index],
            unpartitioned.instance_pages[original]
        );
    }
    let mut app = render_app();
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.insert_resource(source.clone());
    world.run_schedule(Render);
    assert_eq!(
        world.resource::<ExecutedFrames>().0,
        [(source.rig.frame_generation, vec![0, 1])]
    );
    let gpu = world.resource::<ActorGpu>();
    assert_eq!(gpu.main_manifest.as_ref(), &gpu.manifest[..2]);
    assert!(
        gpu.main_manifest
            .iter()
            .all(|entry| entry.identity.runtime_id == 2)
    );
    assert_eq!(gpu.manifest.len(), 3);
    assert!(!gpu.mask_spans.is_empty());
}
