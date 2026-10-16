use super::*;

#[test]
fn direct_block_outline_respects_the_nearer_actor() {
    for (actor_z, has_outline) in [(Some(7.5), false), (Some(5.5), true), (None, true)] {
        let (mut world, _) = fixture();
        if let Some(z) = actor_z {
            actor_use_tests::spawn(&mut world, z);
        }
        let origin = frame_origin(&world, Vec3::new(4.5, 2.5, 8.5), Quat::IDENTITY);
        world.insert_resource(origin);
        world.init_resource::<client_state::RuntimeSettings>();
        world.init_resource::<render_scene::BlockSelectionFrameResource>();
        world
            .run_system_cached(crate::block_selection::publish)
            .unwrap();
        let frame = world.resource::<render_scene::BlockSelectionFrameResource>();
        assert_eq!(
            !frame.outline.is_empty(),
            has_outline,
            "actor z={actor_z:?}"
        );
    }
}
