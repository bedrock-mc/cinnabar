use super::*;
use bevy::ecs::system::RunSystemOnce;

#[test]
fn recurring_particle_updates_allocate_no_gpu_staging_buffers() {
    let Some(mut app) = crate::upload_allocation_tests::app(ParticleRenderPlugin) else {
        return;
    };
    let world = app.sub_app_mut(RenderApp).world_mut();
    world.insert_resource(ParticleGpuFrame {
        opaque: Arc::from([ParticleInstance::default()]),
        blend: Arc::from([ParticleInstance::default()]),
        add: Arc::from([ParticleInstance::default()]),
        ..Default::default()
    });
    world.run_system_once(prepare_particle_resources).unwrap();
    assert_eq!(world.resource::<ParticleGpu>().opaque_range, 0..1);
    assert_eq!(world.resource::<ParticleGpu>().blend_range, 1..2);
    assert_eq!(world.resource::<ParticleGpu>().add_range, 2..3);
    let initial_buffers = crate::upload_allocation_tests::buffers(world);
    for position in [1.0, 2.0, 3.0] {
        {
            let mut frame = world.resource_mut::<ParticleGpuFrame>();
            Arc::make_mut(&mut frame.opaque)[0].center_light[2] = position;
            Arc::make_mut(&mut frame.blend)[0].center_light[0] = position;
            Arc::make_mut(&mut frame.add)[0].center_light[1] = position;
        }
        world.run_system_once(prepare_particle_resources).unwrap();
        assert_eq!(world.resource::<ParticleGpu>().opaque_range, 0..1);
        assert_eq!(world.resource::<ParticleGpu>().blend_range, 1..2);
        assert_eq!(world.resource::<ParticleGpu>().add_range, 2..3);
        assert_eq!(
            crate::upload_allocation_tests::buffers(world),
            initial_buffers,
            "changed particles must not allocate another Metal staging buffer"
        );
    }
}
