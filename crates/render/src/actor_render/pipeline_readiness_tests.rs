use super::*;
use bevy::prelude::Mut;
use std::collections::HashSet;

#[test]
fn multisampled_views_prepare_performance_actor_materials_before_switching() {
    use bevy::ecs::system::RunSystemOnce;

    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.init_resource::<render_scene::ActorPipelineReadiness>();
    world.run_system_once(prepare_actor_pipelines).unwrap();
    let pipeline = world.resource::<ActorPipeline>();
    for material in prewarm_materials() {
        assert!(
            pipeline
                .draw_variant(Msaa::Off, false, false, material)
                .is_some()
        );
        if super::super::motion::eligible(material) {
            assert!(
                pipeline
                    .motion_variant(ActorPipelineKey {
                        msaa: Msaa::Off,
                        hdr: false,
                        enhanced: false,
                        material,
                    })
                    .is_some()
            );
        }
    }
    let warmed = (pipeline.draw_variants.len(), pipeline.motion_ids.len());
    for mut msaa in world.query::<&mut Msaa>().iter_mut(world) {
        *msaa = Msaa::Off;
    }
    world.run_system_once(prepare_actor_pipelines).unwrap();
    let pipeline = world.resource::<ActorPipeline>();
    assert_eq!(
        (pipeline.draw_variants.len(), pipeline.motion_ids.len()),
        warmed
    );
}

#[test]
fn readiness_polls_each_raster_contract_once_and_observes_reload() {
    let (mut app, _) = crate::queue_review_support::app();
    let world = app.world_mut();
    world.init_resource::<ActorPipeline>();
    world.resource_scope(|world, mut pipeline: Mut<ActorPipeline>| {
        let cache = world.resource::<PipelineCache>();
        let view = (Msaa::Off, false, false);
        assert!(!pipeline.ready_with(view, |_| panic!("unrequested view has no pipelines")));
        pipeline.prewarm(cache, view.0, view.1, view.2).unwrap();
        let expected: HashSet<_> = prewarm_materials()
            .flat_map(|material| {
                let color = pipeline.draw_variant(view.0, view.1, view.2, material);
                let motion = super::super::motion::eligible(material)
                    .then(|| {
                        pipeline.motion_variant(ActorPipelineKey {
                            msaa: view.0,
                            hdr: view.1,
                            enhanced: view.2,
                            material,
                        })
                    })
                    .flatten();
                [color, motion].into_iter().flatten()
            })
            .collect();
        let mut probes = HashSet::new();
        let allocated = test_alloc::thread_allocations();
        let mut count = 0;
        assert!(pipeline.ready_with(view, |_| {
            count += 1;
            true
        }));
        assert_eq!(test_alloc::thread_allocations(), allocated);
        assert_eq!(count, expected.len());
        assert!(pipeline.ready_with(view, |id| {
            assert!(probes.insert(id));
            true
        }));
        assert_eq!(probes, expected);
        for missing in expected {
            assert!(!pipeline.ready_with(view, |id| id != missing));
            assert!(pipeline.ready_with(view, |_| true));
        }
        assert!(!pipeline.ready_with((Msaa::Sample4, false, false), |_| true));
    });
}
