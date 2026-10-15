use super::*;

/// Queues repeated non-particle identities in a deliberately unsorted order around particle ties.
fn queue_scene(
    app: &mut App,
    view: RetainedViewEntity,
    entity: Entity,
    draw: DrawFunctionId,
    count: usize,
) {
    let mut phases = app
        .world_mut()
        .resource_mut::<ViewSortedRenderPhases<Transparent3d>>();
    let phase = phases.get_mut(&view).unwrap();
    phase.add_retained(ordered_item(entity, draw, -6.0));
    for index in 0..count {
        let mut item = ordered_item(entity, draw, -(((index * 31) % count) as f32));
        item.batch_range = index as u32..index as u32 + 1;
        item.indexed = index % 2 == 0;
        crate::transparent_phase::add(phase, item);
    }
}

/// Records allocation capacities and entry addresses outside the measured systems.
fn storage_witness(app: &App, view: RetainedViewEntity) -> Vec<(usize, usize)> {
    let phase = &app
        .world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>()[&view];
    let draws = &app.world().resource::<ParticlePhase>().0[&view];
    let mut witness = vec![
        (
            phase.items.capacity(),
            std::ptr::from_ref(phase.items.get_index(0).unwrap().1) as usize,
        ),
        (
            draws.order.source.capacity(),
            draws.order.source.as_ptr() as usize,
        ),
        (
            draws.order.destinations.capacity(),
            draws.order.destinations.as_ptr() as usize,
        ),
        (draws.streams.capacity(), draws.streams.as_ptr() as usize),
    ];
    witness.extend(draws.streams.iter().map(|stream| {
        (
            stream.instances.capacity(),
            stream.instances.as_ptr() as usize,
        )
    }));
    witness
}

/// Builds the baseline before invoking the shared merge caller.
fn queued_reference(app: &App, view: RetainedViewEntity) -> SortedRenderPhase<Transparent3d> {
    let phases = app
        .world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>();
    let pending = app.world().resource::<ParticlePhase>();
    per_instance_reference(&phases[&view], &pending.0[&view])
}

#[test]
fn warm_queue_and_shared_merge_reuse_storage_and_remove_stale_views() {
    const SCENE_DRAWS: usize = 1024;
    const PARTICLES: usize = 128;
    let (mut app, view) = crate::queue_review_support::app();
    app.add_render_command::<Transparent3d, SetItemPipeline>();
    let other = app
        .world()
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .id::<SetItemPipeline>();
    let entity = app.world_mut().spawn_empty().id();
    let instances = (0..PARTICLES)
        .map(|index| ParticleInstance {
            center_light: [0.0, 0.0, -(((index * 17) % PARTICLES) as f32), 0.0],
            ..default()
        })
        .collect::<Vec<_>>();
    queue_lists(
        &mut app,
        frame(DrawLists {
            opaque: vec![ParticleInstance::default(); 3],
            blend: instances.clone(),
            add: instances,
            ..default()
        }),
    );
    crate::queue_review_support::clear(&mut app, view);
    let queue = app.world_mut().register_system(queue_particles);
    let merge = app
        .world_mut()
        .register_system(crate::transparent_phase::sort);
    queue_scene(&mut app, view, entity, other, SCENE_DRAWS);
    app.world_mut().run_system(queue).unwrap();
    let reference = queued_reference(&app, view);
    app.world_mut().run_system(merge).unwrap();
    assert_eq!(
        expanded(
            &app.world()
                .resource::<ViewSortedRenderPhases<Transparent3d>>()[&view]
        ),
        expanded(&reference)
    );
    let warmed = storage_witness(&app, view);

    crate::queue_review_support::clear(&mut app, view);
    queue_scene(&mut app, view, entity, other, SCENE_DRAWS);
    let (_, queued) = test_alloc::measure(|| app.world_mut().run_system(queue).unwrap());
    let reference = queued_reference(&app, view);
    let (_, merged) = test_alloc::measure(|| app.world_mut().run_system(merge).unwrap());
    assert_eq!(
        queued.calls, 0,
        "warm particle queue must reuse its per-view buffers"
    );
    assert_eq!(
        merged.calls, 0,
        "shared merge must retain the phase map and sort scratch"
    );
    assert_eq!(storage_witness(&app, view), warmed);
    let phase = &app
        .world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>()[&view];
    assert_eq!(expanded(phase), expanded(&reference));
    let mut coverage: Vec<_> = expanded(phase)
        .into_iter()
        .filter_map(|draw| draw.instance)
        .collect();
    coverage.sort_unstable();
    assert_eq!(
        coverage,
        (3..(3 + PARTICLES * 2) as u32).collect::<Vec<_>>()
    );
    let pending = &app.world().resource::<ParticlePhase>().0[&view];
    assert!(!pending.has_pending());
    assert!(
        pending
            .streams
            .iter()
            .all(|stream| stream.template.is_none() && stream.instances.is_empty())
    );
    assert!(pending.order.source.is_empty() && pending.order.destinations.is_empty());

    // A smaller live scene releases oversized scratch and cannot replay the previous instance ranges.
    crate::queue_review_support::clear(&mut app, view);
    queue_scene(&mut app, view, entity, other, 2);
    app.insert_resource(frame(DrawLists {
        blend: vec![ParticleInstance::default()],
        ..default()
    }));
    app.world_mut().run_system(queue).unwrap();
    let source_count = app
        .world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>()[&view]
        .items
        .len();
    let reference = queued_reference(&app, view);
    app.world_mut().run_system(merge).unwrap();
    let phase = &app
        .world()
        .resource::<ViewSortedRenderPhases<Transparent3d>>()[&view];
    assert_eq!(expanded(phase), expanded(&reference));
    assert_eq!(
        expanded(phase)
            .into_iter()
            .filter_map(|draw| draw.instance)
            .collect::<Vec<_>>(),
        [0]
    );
    let pending = &app.world().resource::<ParticlePhase>().0[&view];
    assert_eq!(pending.streams.len(), 1);
    assert!(pending.streams[0].instances.capacity() <= MERGE_RETAINED_CAPACITY_FACTOR);
    assert!(pending.order.source.capacity() <= source_count * MERGE_RETAINED_CAPACITY_FACTOR);
    assert!(
        pending.order.destinations.capacity() <= phase.items.len() * MERGE_RETAINED_CAPACITY_FACTOR
    );
    assert!(phase.items.capacity() <= phase.items.len() * MERGE_RETAINED_CAPACITY_FACTOR);

    crate::queue_review_support::clear(&mut app, view);
    app.insert_resource(ParticleGpuFrame::default());
    app.world_mut().run_system(queue).unwrap();
    app.world_mut().run_system(merge).unwrap();
    assert_eq!(
        crate::queue_review_support::items(&app, view).len(),
        1,
        "only the retained consumer survives an empty particle frame"
    );
    assert!(app.world().resource::<ParticlePhase>().0.is_empty());
    assert_eq!(app.world().resource::<ParticlePhase>().0.capacity(), 0);

    app.insert_resource(frame(DrawLists {
        opaque: vec![ParticleInstance::default()],
        ..default()
    }));
    let (_, opaque_only) = test_alloc::measure(|| app.world_mut().run_system(queue).unwrap());
    assert_eq!(
        opaque_only.calls, 0,
        "opaque-only frames need no side-stream storage"
    );
    app.world_mut().run_system(merge).unwrap();
    assert!(app.world().resource::<ParticlePhase>().0.is_empty());

    crate::queue_review_support::clear(&mut app, view);
    app.insert_resource(frame(DrawLists {
        blend: vec![ParticleInstance::default()],
        ..default()
    }));
    app.world_mut().run_system(queue).unwrap();
    app.world_mut()
        .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
        .remove(&view);
    app.world_mut().run_system(merge).unwrap();
    assert!(app.world().resource::<ParticlePhase>().0.is_empty());
    app.world_mut()
        .resource_mut::<ViewSortedRenderPhases<Transparent3d>>()
        .prepare_for_new_frame(view);
    app.world_mut().run_system(merge).unwrap();
    assert!(crate::queue_review_support::items(&app, view).is_empty());
}
