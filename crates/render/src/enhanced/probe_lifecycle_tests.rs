use super::*;
#[test]
fn actor_clock_stamps_do_not_refresh_a_stationary_reflection() {
    let mut frame = crate::ActorRenderFrame::default();
    frame.rig.instances = Arc::from([crate::ActorGpuInstance::default()]);
    let unchanged = actor_signature(&frame);
    frame.rig.frame_generation += 1;
    Arc::make_mut(&mut frame.rig.instances)[0].partial_tick = 0.5;
    assert_eq!(actor_signature(&frame), unchanged);
    Arc::make_mut(&mut frame.rig.instances)[0].world_from_actor[0][3] = 5.0;
    assert_ne!(actor_signature(&frame), unchanged);
}

#[test]
fn stale_render_feedback_cannot_complete_a_relocated_probe() {
    let mut origin = ProbeOrigin {
        epoch: 1,
        ..Default::default()
    };
    origin.reset_faces();
    let previous_frame = origin.clone();
    previous_frame.complete_face(3);
    assert_eq!(origin.face_mask(), 1 << 3);
    origin.epoch = 2;
    origin.reset_faces();
    previous_frame.complete_face(1);
    assert_eq!(origin.face_mask(), 0);
    origin.complete_face(4);
    assert_eq!(origin.face_mask(), 1 << 4);
}

#[test]
fn cube_capture_axes_are_orthogonal_and_cover_the_sphere() {
    let mut sum = Vec3::ZERO;
    for face in 0..6 {
        let (direction, up) = face_basis(face);
        assert_eq!(direction.dot(up), 0.0);
        sum += direction;
    }
    assert_eq!(sum, Vec3::ZERO);
}

#[test]
fn probe_origin_hysteresis_avoids_eight_block_reflection_drops() {
    assert!(!should_relocate(Vec3::ZERO, Vec3::new(15.99, 0.0, 0.0)));
    assert!(!should_relocate(Vec3::ZERO, Vec3::new(0.0, 0.0, -16.0)));
    assert!(should_relocate(Vec3::ZERO, Vec3::new(16.01, 0.0, 0.0)));
}

#[test]
fn dimension_changes_discard_completed_reflections_at_the_same_coordinates() {
    let mut app = App::new();
    app.init_resource::<Cameras>()
        .init_resource::<ProbeOrigin>()
        .init_resource::<Assets<Image>>()
        .init_resource::<crate::AtmosphereViewInputs>()
        .add_systems(Update, sync_cameras);
    let camera = app
        .world_mut()
        .spawn((
            Camera::default(),
            GlobalTransform::IDENTITY,
            EnhancedRendering::default(),
        ))
        .id();
    app.update();
    for dimension in [-42, 700, 0] {
        let previous = app.world().resource::<ProbeOrigin>().clone();
        for face in 0..CUBE_FACE_COUNT {
            previous.complete_face(face);
        }
        app.update();
        assert_eq!(app.world().resource::<ProbeOrigin>().epoch, previous.epoch);
        assert_eq!(previous.face_mask(), (1 << CUBE_FACE_COUNT) - 1);
        app.world_mut()
            .resource_mut::<crate::AtmosphereViewInputs>()
            .dimension = dimension;
        app.update();
        let current = app.world().resource::<ProbeOrigin>();
        assert_eq!(current.source, Some(camera));
        assert_eq!(current.position, previous.position);
        assert_ne!(
            current.epoch, previous.epoch,
            "a new dimension must retire the old reflection epoch"
        );
        assert_eq!(current.face_mask(), 0);
        previous.complete_face(0);
        assert_eq!(
            current.face_mask(),
            0,
            "late feedback from the old dimension is rejected"
        );
        assert_eq!(
            app.world().resource::<Cameras>().entities.len(),
            CUBE_FACE_COUNT as usize
        );
    }
}

#[test]
fn cloud_visibility_changes_discard_completed_reflections() {
    let mut app = App::new();
    app.init_resource::<Cameras>()
        .init_resource::<ProbeOrigin>()
        .init_resource::<Assets<Image>>()
        .init_resource::<crate::CloudVisibility>()
        .add_systems(Update, sync_cameras);
    let source = app
        .world_mut()
        .spawn((
            Camera::default(),
            GlobalTransform::IDENTITY,
            EnhancedRendering {
                waving: false,
                ..default()
            },
        ))
        .id();
    app.update();
    for visible in [false, true, false] {
        let previous = app.world().resource::<ProbeOrigin>().clone();
        for face in 0..CUBE_FACE_COUNT {
            previous.complete_face(face);
        }
        app.update();
        assert_eq!(app.world().resource::<ProbeOrigin>().epoch, previous.epoch);
        app.world_mut().resource_mut::<crate::CloudVisibility>().0 = visible;
        app.update();
        let current = app.world().resource::<ProbeOrigin>();
        assert_ne!(
            current.epoch, previous.epoch,
            "cloud preference changes retire old reflection faces"
        );
        assert_eq!(current.face_mask(), 0);
        previous.complete_face(0);
        assert_eq!(
            current.face_mask(),
            0,
            "old cloudy render feedback is rejected"
        );
        let world = app.world_mut();
        let mut captures = world.query_filtered::<&EnhancedRendering, With<ProbeFace>>();
        assert_eq!(captures.iter(world).count(), CUBE_FACE_COUNT as usize);
        assert!(
            captures
                .iter(world)
                .all(|settings| settings.volumetric_clouds == visible)
        );
        assert!(
            world
                .entity(source)
                .get::<EnhancedRendering>()
                .unwrap()
                .volumetric_clouds
        );
    }
}
