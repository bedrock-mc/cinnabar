use super::{CameraMotionBlur, extract_settings};
use bevy::{
    prelude::*,
    render::{MainWorld, sync_world::RenderEntity},
};

#[derive(Component, Debug, PartialEq)]
struct RetainedScene(u64);

fn worlds() -> (World, Entity, Entity) {
    let mut render = World::new();
    let render_entity = render.spawn((RetainedScene(42), Msaa::Sample4)).id();
    let mut main = MainWorld::default();
    let camera = main
        .spawn((Camera3d::default(), RenderEntity::from(render_entity)))
        .id();
    render.insert_resource(main);
    (render, camera, render_entity)
}

fn settings() -> CameraMotionBlur {
    CameraMotionBlur {
        exposure_seconds: 0.01,
        samples: 7,
        reset_epoch: 0,
        delta_seconds: 0.02,
    }
}

#[test]
fn motion_blur_extraction_toggles_only_its_component_and_preserves_camera_resources() {
    let (mut render, camera, entity) = worlds();
    let mut extract = IntoSystem::into_system(extract_settings);
    extract.initialize(&mut render);
    extract.run((), &mut render).unwrap();
    assert!(render.get::<CameraMotionBlur>(entity).is_none());

    render
        .resource_mut::<MainWorld>()
        .entity_mut(camera)
        .insert(settings());
    extract.run((), &mut render).unwrap();
    assert_eq!(render.get::<CameraMotionBlur>(entity), Some(&settings()));
    let changed = CameraMotionBlur {
        reset_epoch: 1,
        ..settings()
    };
    render
        .resource_mut::<MainWorld>()
        .entity_mut(camera)
        .insert(changed);
    extract.run((), &mut render).unwrap();
    assert_eq!(render.get::<CameraMotionBlur>(entity), Some(&changed));

    render
        .resource_mut::<MainWorld>()
        .entity_mut(camera)
        .remove::<CameraMotionBlur>();
    extract.run((), &mut render).unwrap();
    assert!(render.get::<CameraMotionBlur>(entity).is_none());
    assert_eq!(
        render.get::<RetainedScene>(entity),
        Some(&RetainedScene(42))
    );
    assert_eq!(render.get::<Msaa>(entity), Some(&Msaa::Sample4));
    assert_eq!(
        render
            .resource::<MainWorld>()
            .get::<RenderEntity>(camera)
            .unwrap()
            .id(),
        entity
    );

    render
        .resource_mut::<MainWorld>()
        .entity_mut(camera)
        .insert(settings());
    extract.run((), &mut render).unwrap();
    assert_eq!(render.get::<CameraMotionBlur>(entity), Some(&settings()));
    assert_eq!(
        render.get::<RetainedScene>(entity),
        Some(&RetainedScene(42))
    );
}

#[test]
fn motion_blur_extraction_allocates_nothing_for_off_or_unchanged_enabled_views() {
    let (mut render, camera, entity) = worlds();
    let mut extract = IntoSystem::into_system(extract_settings);
    extract.initialize(&mut render);
    extract.run((), &mut render).unwrap();
    let allocated = crate::alloc_count::thread_allocations();
    extract.run((), &mut render).unwrap();
    assert_eq!(crate::alloc_count::thread_allocations(), allocated);

    render
        .resource_mut::<MainWorld>()
        .entity_mut(camera)
        .insert(settings());
    extract.run((), &mut render).unwrap();
    extract.run((), &mut render).unwrap();
    render.clear_trackers();
    let allocated = crate::alloc_count::thread_allocations();
    extract.run((), &mut render).unwrap();
    assert_eq!(crate::alloc_count::thread_allocations(), allocated);
    assert!(
        !render
            .entity(entity)
            .get_ref::<CameraMotionBlur>()
            .unwrap()
            .is_changed()
    );

    render
        .resource_mut::<MainWorld>()
        .entity_mut(camera)
        .remove::<CameraMotionBlur>();
    extract.run((), &mut render).unwrap();
    extract.run((), &mut render).unwrap();
    let allocated = crate::alloc_count::thread_allocations();
    extract.run((), &mut render).unwrap();
    assert_eq!(crate::alloc_count::thread_allocations(), allocated);
    assert!(render.get::<CameraMotionBlur>(entity).is_none());
}
