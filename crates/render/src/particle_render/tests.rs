use super::*;
use bevy::ecs::system::RunSystemOnce;
use bevy::render::render_phase::{DrawFunctionId, SortedRenderPhase};
use particles::DrawLists;

/// Uploads and queues all material lists through the production systems.
fn queue_lists(app: &mut App, frame: ParticleGpuFrame) {
    app.init_resource::<ParticlePhase>()
        .init_resource::<ParticlePipeline>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Opaque as u8 }>>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Blend as u8 }>>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Add as u8 }>>();
    app.world_mut().run_system_once(init_particle_gpu).unwrap();
    app.insert_resource(frame);
    app.world_mut()
        .run_system_once(prepare_particle_resources)
        .unwrap();
    app.world_mut().run_system_once(queue_particles).unwrap();
}

/// The extracted frame for camera-sorted draw lists.
fn frame(lists: DrawLists) -> ParticleGpuFrame {
    ParticleGpuFrame {
        opaque: lists.opaque.into(),
        blend: lists.blend.into(),
        add: lists.add.into(),
        ..Default::default()
    }
}

/// Queues a depth-writing particle through the production upload and queue systems.
pub(crate) fn queue_opaque(app: &mut App) -> (DrawFunctionId, RenderPipelineDescriptor) {
    queue_lists(
        app,
        ParticleGpuFrame {
            opaque: vec![ParticleInstance::default()].into(),
            ..Default::default()
        },
    );
    let draw = app
        .world()
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .id::<DrawParticles<{ ParticleMode::Opaque as u8 }>>();
    let id = app
        .world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>()
        .values()
        .flat_map(|phase| phase.items.values())
        .find(|item| item.draw_function == draw)
        .unwrap()
        .pipeline;
    let mut cache = app.world_mut().resource_mut::<PipelineCache>();
    (
        draw,
        crate::queue_review_support::queued_descriptor(&mut cache, id).clone(),
    )
}

#[test]
fn additive_particles_compose_at_their_depth_with_only_cutout_particles_present() {
    for distance in [-10.0, -2.0] {
        let (mut app, view) = crate::queue_review_support::app();
        queue_lists(
            &mut app,
            ParticleGpuFrame {
                opaque: vec![ParticleInstance {
                    center_light: [100.0, 0.0, -1.0, 0.0],
                    ..Default::default()
                }]
                .into(),
                add: vec![ParticleInstance {
                    center_light: [0.0, 0.0, distance, 0.0],
                    ..Default::default()
                }]
                .into(),
                ..Default::default()
            },
        );
        let functions = app
            .world()
            .resource::<DrawFunctions<Transparent3d>>()
            .read();
        let add = functions.id::<DrawParticles<{ ParticleMode::Add as u8 }>>();
        let terrain = functions.id::<DrawParticles<{ ParticleMode::Blend as u8 }>>();
        drop(functions);
        let pending = &app.world().resource::<ParticlePhase>().0[&view];
        let item = pending.streams[0].template.as_ref().unwrap();
        let extra = Transparent3d {
            sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
            entity: item.entity,
            pipeline: item.pipeline,
            draw_function: terrain,
            distance: -5.0,
            batch_range: 0..1,
            extra_index: PhaseItemExtraIndex::None,
            indexed: false,
        };
        crate::transparent_phase::add(
            app.world_mut()
                .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
                .get_mut(&view)
                .unwrap(),
            extra,
        );
        app.world_mut()
            .run_system_once(crate::transparent_phase::sort)
            .unwrap();
        let items = crate::queue_review_support::items(&app, view);
        let mut pixel = 0.2;
        let mut depth = 0.0;
        for item in items {
            if item.draw_function == terrain {
                pixel = 0.1 * 0.5 + pixel * 0.5;
                depth = 0.5;
            } else if item.draw_function == add {
                let particle_depth = if distance < -5.0 { 0.25 } else { 0.75 };
                if particle_depth >= depth {
                    pixel += 0.6;
                }
            }
        }
        let expected: f32 = if distance < -5.0 { 0.45 } else { 0.75 };
        assert!(
            (pixel - expected).abs() < 0.0001,
            "additive color must compose through translucent terrain at its own depth"
        );
    }
}

#[test]
fn blended_and_additive_particles_keep_their_translucent_pipeline() {
    let (mut app, _) = crate::queue_review_support::app();
    queue_opaque(&mut app);
    for material in [ParticleMode::Blend, ParticleMode::Add] {
        let world = app.world_mut();
        let id = world.resource_scope(|world, mut pipeline: Mut<ParticlePipeline>| {
            pipeline
                .variants
                .specialize(
                    world.resource::<PipelineCache>(),
                    ParticlePipelineKey {
                        msaa: Msaa::Off,
                        hdr: false,
                        material,
                    },
                )
                .unwrap()
        });
        let mut cache = world.resource_mut::<PipelineCache>();
        let descriptor = crate::queue_review_support::queued_descriptor(&mut cache, id);
        assert_eq!(
            descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap()
                .write_mask,
            ColorWrites::COLOR
        );
        assert_eq!(
            descriptor
                .depth_stencil
                .as_ref()
                .unwrap()
                .depth_write_enabled,
            Some(false)
        );
        assert!(
            descriptor.fragment.as_ref().unwrap().targets[0]
                .as_ref()
                .unwrap()
                .blend
                .is_some()
        );
    }
}

#[test]
fn particles_on_both_sides_of_glass_compose_in_depth_order() {
    for additive in [false, true] {
        let (mut app, view) = crate::queue_review_support::app();
        let particles = [-10.0, -2.0]
            .into_iter()
            .zip([0.4, 0.6])
            .map(|(z, red)| ParticleInstance {
                center_light: [0.0, 0.0, z, 0.0],
                color: [red, 0.0, 0.0, 0.5],
                ..Default::default()
            })
            .collect::<Vec<_>>();
        let lists = if additive {
            DrawLists {
                add: particles.clone(),
                ..Default::default()
            }
        } else {
            DrawLists {
                blend: particles.clone(),
                ..Default::default()
            }
        };
        queue_lists(&mut app, frame(lists));
        let functions = app
            .world()
            .resource::<DrawFunctions<Transparent3d>>()
            .read();
        let blend = functions.id::<DrawParticles<{ ParticleMode::Blend as u8 }>>();
        drop(functions);
        add_glass(&mut app, view, blend);
        let ordered: Vec<_> = crate::queue_review_support::items(&app, view)
            .into_iter()
            .filter(|item| !item.batch_range.is_empty())
            .map(|item| {
                if item.distance == -5.0 {
                    ("glass", 0..1)
                } else {
                    let range = match &item.extra_index {
                        PhaseItemExtraIndex::IndirectParametersIndex { range, .. } => range.clone(),
                        _ => 0..particles.len() as u32,
                    };
                    ("particle", range)
                }
            })
            .collect();
        eprintln!(
            "PARTICLE_PHASE {} {}",
            if additive { "add" } else { "blend" },
            serde_json::to_string(&ordered).unwrap()
        );
        let mut pixel = 0.0;
        for (kind, range) in &ordered {
            if *kind == "glass" {
                pixel = 0.1 * 0.5 + pixel * 0.5;
            } else {
                for particle in &particles[range.start as usize..range.end as usize] {
                    pixel = particle.color[0] * 0.5 + if additive { pixel } else { pixel * 0.5 };
                }
            }
        }
        let expected = if additive { 0.45 } else { 0.375 };
        assert!(
            (pixel - expected).abs() < 0.0001,
            "both particle depths must compose across the glass"
        );
    }
}

#[test]
fn adjacent_particle_instances_share_a_draw_without_crossing_glass() {
    let (mut app, view) = crate::queue_review_support::app();
    queue_lists(
        &mut app,
        frame(DrawLists {
            blend: [-12.0, -10.0, -3.0, -2.0]
                .map(|z| ParticleInstance {
                    center_light: [0.0, 0.0, z, 0.0],
                    ..Default::default()
                })
                .to_vec(),
            ..Default::default()
        }),
    );
    let functions = app
        .world()
        .resource::<DrawFunctions<Transparent3d>>()
        .read();
    let blend = functions.id::<DrawParticles<{ ParticleMode::Blend as u8 }>>();
    drop(functions);
    add_glass(&mut app, view, blend);
    let ranges = crate::queue_review_support::items(&app, view)
        .into_iter()
        .filter(|item| !item.batch_range.is_empty())
        .filter_map(|item| match &item.extra_index {
            PhaseItemExtraIndex::IndirectParametersIndex { range, .. } => Some(range.clone()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(ranges, [0..2, 2..4]);
}

#[test]
fn queued_particle_ranges_use_the_extracted_frame_before_gpu_preparation() {
    let (mut app, view) = crate::queue_review_support::app();
    queue_lists(
        &mut app,
        frame(DrawLists {
            blend: vec![ParticleInstance::default()],
            ..Default::default()
        }),
    );
    crate::queue_review_support::clear(&mut app, view);
    app.insert_resource(frame(DrawLists {
        opaque: vec![ParticleInstance::default()],
        blend: [-10.0, -8.0, -4.0, -2.0]
            .map(|z| ParticleInstance {
                center_light: [0.0, 0.0, z, 0.0],
                ..Default::default()
            })
            .to_vec(),
        ..Default::default()
    }));
    app.world_mut().run_system_once(queue_particles).unwrap();
    app.world_mut()
        .run_system_once(crate::transparent_phase::sort)
        .unwrap();
    let mut ranges: Vec<_> = crate::queue_review_support::items(&app, view)
        .iter()
        .filter_map(|item| match &item.extra_index {
            PhaseItemExtraIndex::IndirectParametersIndex { range, .. } => Some(range.clone()),
            _ => None,
        })
        .collect();
    ranges.sort_by_key(|range| range.start);
    assert_eq!(ranges.len(), 1);
    assert_eq!(ranges[0], 1..5);
}

/// Adds translucent terrain between the particles before the shared phase merge.
fn add_glass(app: &mut App, view: RetainedViewEntity, glass: DrawFunctionId) {
    let pending = &app.world().resource::<ParticlePhase>().0[&view];
    let first = pending.streams[0].template.as_ref().unwrap();
    let (entity, pipeline) = (first.entity, first.pipeline);
    {
        let mut phases = app
            .world_mut()
            .resource_mut::<ViewSortedRenderPhases<Transparent3d>>();
        crate::transparent_phase::add(
            phases.get_mut(&view).unwrap(),
            Transparent3d {
                sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
                entity,
                pipeline,
                draw_function: glass,
                distance: -5.0,
                batch_range: 0..1,
                extra_index: PhaseItemExtraIndex::None,
                indexed: false,
            },
        );
    }
    app.world_mut()
        .run_system_once(crate::transparent_phase::sort)
        .unwrap();
}

/// Copies a phase item so the reference can retain each instance as a separate draw.
fn copy_item(item: &Transparent3d) -> Transparent3d {
    Transparent3d {
        sorting_info: item.sorting_info,
        distance: item.distance,
        pipeline: item.pipeline,
        entity: item.entity,
        draw_function: item.draw_function,
        batch_range: item.batch_range.clone(),
        extra_index: item.extra_index.clone(),
        indexed: item.indexed,
    }
}

/// Recreates the original per-instance queue and delegates its stable sort to Bevy.
fn per_instance_reference(
    phase: &SortedRenderPhase<Transparent3d>,
    pending: &ParticleViewDraws,
) -> SortedRenderPhase<Transparent3d> {
    let mut reference = SortedRenderPhase::default();
    for insertion in 0..=phase.items.len() {
        for stream in &pending.streams[..pending.active] {
            if stream.insertion != insertion {
                continue;
            }
            // GPU indices retain the original material-list order before the per-view fallback.
            let mut instances = stream.instances.as_slice().to_vec();
            instances.sort_by_key(|&(_, index)| index);
            for (distance, index) in instances {
                let mut item = copy_item(stream.template.as_ref().unwrap());
                item.distance = distance.0;
                item.extra_index = PhaseItemExtraIndex::IndirectParametersIndex {
                    range: index..index + 1,
                    batch_set_index: None,
                };
                crate::transparent_phase::add(&mut reference, item);
            }
        }
        if let Some((_, item)) = phase.items.get_index(insertion) {
            crate::transparent_phase::add(&mut reference, copy_item(item));
        }
    }
    reference.sort();
    reference
}

#[derive(Debug, PartialEq, Eq)]
struct ExpandedDraw {
    entity: (Entity, MainEntity),
    pipeline: bevy::render::render_resource::CachedRenderPipelineId,
    draw: DrawFunctionId,
    indexed: bool,
    instance: Option<u32>,
    batch: Range<u32>,
}

/// Expands particle runs to compare draw identity, exact coverage, and intervening consumers.
fn expanded(phase: &SortedRenderPhase<Transparent3d>) -> Vec<ExpandedDraw> {
    phase
        .items
        .values()
        .flat_map(|item| {
            let instances = match &item.extra_index {
                PhaseItemExtraIndex::IndirectParametersIndex { range, .. } => {
                    range.clone().map(Some).collect::<Vec<_>>()
                }
                _ => vec![None],
            };
            instances.into_iter().map(move |instance| ExpandedDraw {
                entity: item.entity,
                pipeline: item.pipeline,
                draw: item.draw_function,
                indexed: item.indexed,
                instance,
                batch: item.batch_range.clone(),
            })
        })
        .collect()
}

/// Creates distinct draw identities without a GPU or a running render schedule.
fn ordering_fixture() -> (Entity, [DrawFunctionId; 3]) {
    let mut app = App::new();
    app.init_resource::<DrawFunctions<Transparent3d>>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Blend as u8 }>>()
        .add_render_command::<Transparent3d, DrawParticles<{ ParticleMode::Add as u8 }>>()
        .add_render_command::<Transparent3d, SetItemPipeline>();
    let functions = app
        .world()
        .resource::<DrawFunctions<Transparent3d>>()
        .read();
    let draws = [
        functions.id::<DrawParticles<{ ParticleMode::Blend as u8 }>>(),
        functions.id::<DrawParticles<{ ParticleMode::Add as u8 }>>(),
        functions.id::<SetItemPipeline>(),
    ];
    drop(functions);
    (app.world_mut().spawn_empty().id(), draws)
}

/// Builds an authored-distance item whose entities may be repeated throughout the phase.
fn ordered_item(entity: Entity, draw: DrawFunctionId, distance: f32) -> Transparent3d {
    Transparent3d {
        sorting_info: bevy::core_pipeline::core_3d::TransparentSortingInfo3d::AlwaysOnTop,
        distance,
        pipeline: bevy::render::render_resource::CachedRenderPipelineId::INVALID,
        entity: (entity, entity.into()),
        draw_function: draw,
        batch_range: 0..1,
        extra_index: PhaseItemExtraIndex::None,
        indexed: false,
    }
}

#[test]
fn merged_streams_match_the_stable_reference_across_consumers_and_materials() {
    let (entity, [blend, add, other]) = ordering_fixture();
    let mut phase = SortedRenderPhase::default();
    // Several consumers share the same ECS entity, including a retained draw.
    phase.add_retained(ordered_item(entity, other, -8.0));
    for distance in [-13.0, -8.0, -3.0, 2.0] {
        crate::transparent_phase::add(&mut phase, ordered_item(entity, other, distance));
    }
    let mut pending = ParticleViewDraws::default();
    pending.push(
        phase.items.len(),
        ordered_item(entity, blend, 0.0),
        [-12.0, -11.0, -8.0, -6.0, -2.0, 0.0]
            .into_iter()
            .enumerate()
            .map(|(index, distance)| (FloatOrd(distance), index as u32)),
    );
    pending.push(
        phase.items.len(),
        ordered_item(entity, add, 0.0),
        [-10.0, -8.0, -7.0, -1.0]
            .into_iter()
            .enumerate()
            .map(|(index, distance)| (FloatOrd(distance), index as u32 + 6)),
    );
    for distance in [-8.0, -5.0, -1.0] {
        crate::transparent_phase::add(&mut phase, ordered_item(entity, other, distance));
    }
    let reference = per_instance_reference(&phase, &pending);
    pending.merge(&mut phase);
    assert_eq!(expanded(&phase), expanded(&reference));
    let mut coverage: Vec<_> = expanded(&phase)
        .into_iter()
        .filter_map(|draw| draw.instance)
        .collect();
    coverage.sort_unstable();
    assert_eq!(coverage, (0..10).collect::<Vec<_>>());
    assert!(phase.items.len() < reference.items.len());
    assert_eq!(
        phase
            .items
            .values()
            .filter(|item| item.draw_function == other)
            .count(),
        8
    );
    assert!(phase.items.values().all(|item| item.batch_range == (0..1)));
}

#[test]
fn equal_depths_keep_queue_ties_and_authored_sorting_information() {
    let (entity, [blend, add, other]) = ordering_fixture();
    let mut phase = SortedRenderPhase::default();
    let mut before = ordered_item(entity, other, 0.0);
    before.sorting_info = bevy::core_pipeline::core_3d::TransparentSortingInfo3d::Sorted {
        mesh_center: Vec3::splat(90.0),
        depth_bias: 12.0,
    };
    crate::transparent_phase::add(&mut phase, before);
    let mut pending = ParticleViewDraws::default();
    pending.push(
        1,
        ordered_item(entity, blend, 0.0),
        [(FloatOrd(-0.0), 3), (FloatOrd(0.0), 4)].into_iter(),
    );
    pending.push(
        1,
        ordered_item(entity, add, 0.0),
        [(FloatOrd(0.0), 5)].into_iter(),
    );
    crate::transparent_phase::add(&mut phase, ordered_item(entity, other, -0.0));
    let reference = per_instance_reference(&phase, &pending);
    pending.merge(&mut phase);
    assert_eq!(expanded(&phase), expanded(&reference));
    assert_eq!(
        expanded(&phase)
            .into_iter()
            .map(|draw| (draw.draw, draw.instance))
            .collect::<Vec<_>>(),
        [
            (other, None),
            (blend, Some(3)),
            (blend, Some(4)),
            (add, Some(5)),
            (other, None)
        ]
    );
    assert!(matches!(
        phase.items.values().next().unwrap().sorting_info,
        bevy::core_pipeline::core_3d::TransparentSortingInfo3d::Sorted {
            depth_bias: 12.0,
            ..
        }
    ));
}

#[test]
fn gaps_and_changed_pipelines_never_share_a_particle_run() {
    let (entity, [blend, _, _]) = ordering_fixture();
    let mut phase = SortedRenderPhase::default();
    let mut pending = ParticleViewDraws::default();
    pending.push(
        0,
        ordered_item(entity, blend, 0.0),
        [(FloatOrd(-9.0), 1), (FloatOrd(-8.0), 3)].into_iter(),
    );
    let mut changed = ordered_item(entity, blend, 0.0);
    changed.pipeline = bevy::render::render_resource::CachedRenderPipelineId::new(7);
    pending.push(
        0,
        changed,
        [(FloatOrd(-7.0), 4), (FloatOrd(-6.0), 5)].into_iter(),
    );
    let reference = per_instance_reference(&phase, &pending);
    pending.merge(&mut phase);
    assert_eq!(expanded(&phase), expanded(&reference));
    let ranges: Vec<_> = phase
        .items
        .values()
        .map(|item| match &item.extra_index {
            PhaseItemExtraIndex::IndirectParametersIndex { range, .. } => range.clone(),
            _ => panic!("expected an instance range"),
        })
        .collect();
    assert_eq!(ranges, [1..2, 3..4, 4..6]);
}

#[test]
fn fallback_uses_phase_order_for_non_finite_depths_and_preserves_ties() {
    let (entity, [blend, add, other]) = ordering_fixture();
    let mut phase = SortedRenderPhase::default();
    for distance in [f32::NAN, f32::NEG_INFINITY, f32::INFINITY, 0.0] {
        crate::transparent_phase::add(&mut phase, ordered_item(entity, other, distance));
    }
    let mut pending = ParticleViewDraws::default();
    for (draw, offset) in [(blend, 0), (add, 6)] {
        pending.push(
            phase.items.len(),
            ordered_item(entity, draw, 0.0),
            [
                f32::INFINITY,
                f32::NAN,
                -3.0,
                f32::NEG_INFINITY,
                f32::NAN,
                0.0,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, depth)| (FloatOrd(depth), offset + index as u32)),
        );
    }
    crate::transparent_phase::add(&mut phase, ordered_item(entity, other, f32::NAN));
    let reference = per_instance_reference(&phase, &pending);
    pending.merge(&mut phase);
    assert_eq!(expanded(&phase), expanded(&reference));
    assert_eq!(
        expanded(&phase)
            .iter()
            .filter_map(|draw| draw.instance)
            .count(),
        12
    );
}

#[test]
fn each_camera_orders_the_extracted_instances_independently() {
    use bevy::prelude::{GlobalTransform, Mat4, Transform, UVec4};
    let (mut app, first) = crate::queue_review_support::app();
    let second_entity = app.world_mut().spawn_empty().id();
    let second = RetainedViewEntity::new(second_entity.into(), None, 0);
    app.world_mut().entity_mut(second_entity).insert((
        MainEntity::from(second_entity),
        Msaa::Off,
        crate::render_test_support::camera(false),
        ExtractedView {
            retained_view_entity: second,
            clip_from_view: Mat4::IDENTITY,
            world_from_view: GlobalTransform::from(Transform::from_rotation(
                bevy::prelude::Quat::from_rotation_y(std::f32::consts::PI),
            )),
            clip_from_world: None,
            target_format: render_scene::SCENE_COLOR_FORMAT,
            viewport: UVec4::new(0, 0, 1, 1),
            color_grading: default(),
            invert_culling: false,
        },
    ));
    app.world_mut()
        .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
        .prepare_for_new_frame(second);
    queue_lists(
        &mut app,
        frame(DrawLists {
            opaque: vec![ParticleInstance::default()],
            blend: [-10.0, -8.0, -2.0]
                .map(|z| ParticleInstance {
                    center_light: [0.0, 0.0, z, 0.0],
                    ..default()
                })
                .to_vec(),
            add: vec![ParticleInstance {
                center_light: [0.0, 0.0, -5.0, 0.0],
                ..default()
            }],
            ..default()
        }),
    );
    let phases = app
        .world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>();
    let pending = app.world().resource::<ParticlePhase>();
    let references: Vec<_> = [first, second]
        .into_iter()
        .map(|view| {
            (
                view,
                per_instance_reference(&phases[&view], &pending.0[&view]),
            )
        })
        .collect();
    assert!(
        phases.values().all(|phase| phase.items.len() == 1),
        "translucent instances must stay outside the global phase sort"
    );
    app.world_mut()
        .run_system_once(crate::transparent_phase::sort)
        .unwrap();
    for (view, reference) in references {
        let phase = &app
            .world()
            .resource::<ViewSortedRenderPhases<Transparent3d>>()[&view];
        assert_eq!(expanded(phase), expanded(&reference));
        assert_eq!(
            phase.items.values().next().unwrap().distance,
            f32::NEG_INFINITY
        );
    }
    let phases = app
        .world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>();
    let coverage = |view| {
        expanded(&phases[&view])
            .into_iter()
            .filter_map(|draw| draw.instance)
            .collect::<Vec<_>>()
    };
    assert_eq!(coverage(first), [1, 2, 4, 3]);
    assert_eq!(coverage(second), [3, 4, 2, 1]);
    assert!(
        app.world()
            .resource::<ParticlePhase>()
            .0
            .values()
            .all(|draws| !draws.has_pending())
    );
}

#[test]
fn replacement_empty_frames_removed_views_and_panorama_leave_no_particle_streams() {
    use bevy::prelude::Update;
    let (mut app, view) = crate::queue_review_support::app();
    queue_lists(
        &mut app,
        frame(DrawLists {
            blend: vec![ParticleInstance::default(); 32],
            ..default()
        }),
    );
    app.world_mut()
        .run_system_once(crate::transparent_phase::sort)
        .unwrap();
    assert_eq!(crate::queue_review_support::items(&app, view).len(), 1);
    crate::queue_review_support::clear(&mut app, view);
    assert!(crate::queue_review_support::items(&app, view).is_empty());
    app.insert_resource(frame(DrawLists {
        opaque: vec![ParticleInstance::default()],
        add: vec![ParticleInstance::default(); 2],
        ..default()
    }));
    app.world_mut().run_system_once(queue_particles).unwrap();
    app.world_mut()
        .run_system_once(crate::transparent_phase::sort)
        .unwrap();
    assert_eq!(crate::queue_review_support::items(&app, view).len(), 2);
    crate::queue_review_support::clear(&mut app, view);
    app.insert_resource(ParticleGpuFrame::default());
    app.world_mut().run_system_once(queue_particles).unwrap();
    app.world_mut()
        .run_system_once(crate::transparent_phase::sort)
        .unwrap();
    assert!(crate::queue_review_support::items(&app, view).is_empty());
    assert!(app.world().resource::<ParticlePhase>().0.is_empty());
    app.insert_resource(frame(DrawLists {
        blend: vec![ParticleInstance::default()],
        ..default()
    }));
    app.world_mut().run_system_once(queue_particles).unwrap();
    app.world_mut()
        .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
        .remove(&view);
    app.world_mut()
        .run_system_once(crate::transparent_phase::sort)
        .unwrap();
    assert!(app.world().resource::<ParticlePhase>().0.is_empty());
    app.world_mut()
        .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
        .prepare_for_new_frame(view);
    app.init_resource::<render_scene::panorama::PanoramaScene>()
        .add_systems(
            Update,
            (
                queue_particles.run_if(render_scene::panorama::world_passes_enabled),
                crate::transparent_phase::sort,
            )
                .chain(),
        );
    app.world_mut()
        .resource_mut::<render_scene::panorama::PanoramaScene>()
        .set_game_visible(false);
    app.update();
    assert!(crate::queue_review_support::items(&app, view).is_empty());
    assert!(app.world().resource::<ParticlePhase>().0.is_empty());
    app.world_mut()
        .resource_mut::<render_scene::panorama::PanoramaScene>()
        .set_game_visible(true);
    app.update();
    assert_eq!(crate::queue_review_support::items(&app, view).len(), 1);
}

#[test]
fn empty_streams_and_unavailable_gpu_pipelines_preserve_other_consumers() {
    let (entity, [blend, _, other]) = ordering_fixture();
    let mut phase = SortedRenderPhase::default();
    crate::transparent_phase::add(&mut phase, ordered_item(entity, other, -5.0));
    let mut pending = ParticleViewDraws::default();
    pending.push(1, ordered_item(entity, blend, 0.0), std::iter::empty());
    assert!(pending.streams.is_empty());
    pending.merge(&mut phase);
    assert_eq!(expanded(&phase).len(), 1);
    let mut pending = ParticleViewDraws::default();
    // No compiled pipeline is available for INVALID; queue coverage still belongs to this frame.
    pending.push(
        1,
        ordered_item(entity, blend, 0.0),
        [(FloatOrd(-6.0), 4), (FloatOrd(-2.0), 5)].into_iter(),
    );
    let reference = per_instance_reference(&phase, &pending);
    pending.merge(&mut phase);
    assert_eq!(expanded(&phase), expanded(&reference));
}

#[test]
fn particle_keys_cannot_replace_later_retained_draws_and_expire_with_the_phase() {
    let (entity, [blend, _, other]) = ordering_fixture();
    let view = RetainedViewEntity::new(entity.into(), None, 0);
    let mut phases = ViewSortedRenderPhases::<Transparent3d>::default();
    phases.prepare_for_new_frame(view);
    let phase = phases.get_mut(&view).unwrap();
    let reserved = (
        Entity::PLACEHOLDER,
        MainEntity::from(Entity::from_raw_u32(2).unwrap()),
    );
    phase
        .items
        .insert(reserved, ordered_item(entity, other, -1.0));
    let mut pending = ParticleViewDraws::default();
    pending.push(
        0,
        ordered_item(entity, blend, 0.0),
        [
            (FloatOrd(-5.0), 0),
            (FloatOrd(-4.0), 2),
            (FloatOrd(-3.0), 4),
        ]
        .into_iter(),
    );
    let reference = per_instance_reference(phase, &pending);
    pending.merge(phase);
    assert_eq!(expanded(phase), expanded(&reference));
    assert_eq!(phase.items[&reserved].draw_function, other);
    assert_eq!(phase.transient_items.len(), 3);
    phases.prepare_for_new_frame(view);
    assert_eq!(phases[&view].items.len(), 1);
    assert_eq!(phases[&view].items[&reserved].draw_function, other);
}

#[path = "storage_tests.rs"]
mod storage_tests;

/// The particle atlas allows the pack's raw view only while a pack draws; switching recreates it.
#[cfg(feature = "shaderpacks")]
#[test]
fn the_particle_atlas_allows_raw_views_only_while_a_pack_draws() {
    use crate::actor::gpu::{ArtworkRawViews, permits_raw_view};
    use bevy::render::renderer::{RenderDevice, RenderQueue};
    let (device, queue) = wgpu::Device::noop(&wgpu::DeviceDescriptor::default());
    let mut world = World::new();
    world.insert_resource(RenderDevice::from(device));
    world.insert_resource(RenderQueue::new(queue));
    world.run_system_once(init_particle_gpu).unwrap();
    let side = 16;
    world.insert_resource(ParticleGpuFrame {
        base: Some(vec![255; (side * side * 4) as usize].into()),
        atlas_side: side,
        ..Default::default()
    });
    world.run_system_once(prepare_particle_resources).unwrap();
    assert!(world.resource::<ParticleGpu>().atlas_texture().is_none());
    for pack in [true, false] {
        world.insert_resource(ArtworkRawViews(pack));
        world.run_system_once(prepare_particle_resources).unwrap();
        let gpu = world.resource::<ParticleGpu>();
        let texture = gpu.texture.as_ref().unwrap();
        let device = world.resource::<RenderDevice>().wgpu_device();
        assert_eq!(permits_raw_view(device, texture), pack);
        assert_eq!(gpu.atlas_texture().is_some(), pack);
    }
}
