use super::*;
use assets::block_shapes::PickContext;
use world::block_state_identity::state_identity;

#[test]
fn camera_visibility_respects_compiled_pick_context() {
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        for (rule, context, depth, admitted) in [
            (PickRule::Never, PickContext::default(), None, false),
            (PickRule::Always, PickContext::default(), None, true),
            (PickRule::Liquid, PickContext::default(), Some(0), false),
            (
                PickRule::LiquidSource,
                PickContext::default(),
                Some(0),
                false,
            ),
            (PickRule::HeldLight, PickContext::default(), None, false),
            (PickRule::Creative, PickContext::default(), None, false),
            (
                PickRule::HeldLight,
                PickContext {
                    holding_light: true,
                    ..Default::default()
                },
                None,
                true,
            ),
            (
                PickRule::Creative,
                PickContext {
                    creative: true,
                    ..Default::default()
                },
                None,
                true,
            ),
            (
                PickRule::Liquid,
                PickContext {
                    liquids: true,
                    ..Default::default()
                },
                Some(1),
                true,
            ),
            (
                PickRule::LiquidSource,
                PickContext {
                    liquids: true,
                    ..Default::default()
                },
                Some(0),
                true,
            ),
            (
                PickRule::LiquidSource,
                PickContext {
                    liquids: true,
                    ..Default::default()
                },
                Some(1),
                false,
            ),
        ] {
            let mut target = shapes(SelectionContext::Static, vec![UNIT], vec![]);
            target.pick_rule = rule;
            let (mut store, mut registry, id) = fixture(space, &target);
            if let Some(depth) = depth {
                registry.set_flow_facts(
                    id,
                    crate::FlowBlockFacts {
                        blocks_motion: false,
                        is_solid: false,
                        blocked_faces: 0,
                        allowed_faces: 63,
                        liquid_depth: Some(depth),
                    },
                );
            }
            let mut behind = shapes(SelectionContext::Static, vec![UNIT], vec![]);
            behind.state_identity = [9; 32];
            registry.register(9, [box_to_aabb(UNIT)]).unwrap();
            registry.set_compiled_selection(9, &behind);
            set_block(&mut store, [1, 0, 0], 9);
            let world = PaletteWorld::new(&store, &registry, 0, context);
            let origin = Vec3::new(-2.0, 0.5, 0.5);
            let direction = Vec3::new(1.0, 0.0, 0.0);
            let authority = world
                .block_interaction_ray_current(origin, direction, 4.0)
                .unwrap()
                .unwrap();
            assert_eq!(
                authority.block_pos,
                if admitted { [0; 3] } else { [1, 0, 0] }
            );
            let camera = world
                .camera_visibility_ray(origin, direction, 4.0)
                .unwrap()
                .unwrap();
            assert_eq!(
                camera.block_pos, authority.block_pos,
                "{space:?} {rule:?} {context:?}"
            );
            assert_eq!(camera.distance, authority.distance);
            assert_eq!(camera.face, authority.face);
            assert_eq!(camera.runtime_id, authority.runtime_id);
        }
    }
}

#[test]
fn camera_visibility_reaches_translated_actor_selection() {
    check_moving_rays(|world, origin, direction| {
        world
            .camera_visibility_ray(origin, direction, 4.0)
            .unwrap()
            .map(|hit| hit.block_pos)
    });
}

#[test]
fn outline_aim_reaches_translated_actor_selection() {
    check_moving_rays(|world, origin, direction| {
        world
            .camera_aim_ray(origin, origin + direction * 4.0, 8, true, false)
            .unwrap()
            .map(|hit| hit.block_pos)
    });
}

#[test]
fn camera_visibility_reaches_selection_only_extents() {
    let extent = ShapeBox::new([0.0; 3], [1.5, 1.0, 1.0]);
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::Static, vec![extent], vec![]);
        let (store, registry, _) = fixture(space, &target);
        let world = PaletteWorld::new(&store, &registry, 0, Default::default());
        let origin = Vec3::new(1.25, 0.5, 2.0);
        let direction = Vec3::new(0.0, 0.0, -1.0);
        let authority = world
            .block_interaction_ray_current(origin, direction, 4.0)
            .unwrap()
            .unwrap();
        let camera = world
            .camera_visibility_ray(origin, direction, 4.0)
            .unwrap()
            .unwrap();
        assert_eq!(camera.block_pos, authority.block_pos);
        assert_eq!(camera.distance, authority.distance);
        let aim = world
            .camera_aim_ray(origin, origin + direction * 4.0, 8, true, false)
            .unwrap()
            .unwrap();
        assert_eq!(aim.block_pos, authority.block_pos);
    }
}

/// Checks displaced carried boxes in both ID spaces, all axes and both motion directions.
fn check_moving_rays(mut inspect: impl FnMut(&PaletteWorld<'_>, Vec3, Vec3) -> Option<[i32; 3]>) {
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        for (facing, direction) in PISTON_DIRECTIONS.into_iter().enumerate() {
            for expanding in [false, true] {
                let target = shapes(SelectionContext::MovingBlock, vec![], vec![]);
                let (mut store, mut registry, _) = fixture(space, &target);
                let mut carried = shapes(SelectionContext::Static, vec![UNIT], vec![]);
                carried.state_identity = state_identity("minecraft:carried", []);
                registry.register(9, []).unwrap();
                registry.set_compiled_selection(9, &carried);
                let mut piston = shapes(SelectionContext::Static, vec![UNIT], vec![]);
                piston.state_identity = [20; 32];
                piston.actor_facing = Some(facing as u8);
                registry.register(20, []).unwrap();
                registry.set_compiled_selection(20, &piston);
                let owner = [-2, 0, 0];
                set_block(&mut store, owner, 20);
                set_piston(&mut store, owner, 0.5);
                set_moving(&mut store, "minecraft:carried", owner, expanding);
                let axis = direction.iter().position(|value| *value != 0).unwrap();
                let ray_axis = (axis + 1) % 3;
                let mut origin = Vec3::new(0.5, 0.5, 0.5);
                let sign = direction[axis] * if expanding { -1 } else { 1 };
                origin[axis] = if sign > 0 { 1.25 } else { -0.25 };
                origin[ray_axis] = 2.0;
                let mut ray = Vec3::ZERO;
                ray[ray_axis] = -1.0;
                let world = PaletteWorld::new(&store, &registry, 0, Default::default());
                assert_eq!(
                    world
                        .block_interaction_ray_current(origin, ray, 4.0)
                        .unwrap()
                        .unwrap()
                        .block_pos,
                    [0; 3]
                );
                assert_eq!(
                    inspect(&world, origin, ray),
                    Some([0; 3]),
                    "{space:?} {facing} {expanding}"
                );
            }
        }
    }
}

#[test]
fn outline_aim_keeps_origin_inside_suppression_with_a_selection_halo() {
    let target = shapes(SelectionContext::Static, vec![UNIT], vec![]);
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let (store, mut registry, _) = fixture(space, &target);
        registry.register(9, []).unwrap();
        registry.set_compiled_selection(9, &shapes(SelectionContext::MovingBlock, vec![], vec![]));
        let world = PaletteWorld::new(&store, &registry, 0, Default::default());
        assert!(
            world
                .camera_aim_ray(
                    Vec3::new(0.5, 0.5, 0.5),
                    Vec3::new(0.5, 0.5, 3.5),
                    6,
                    true,
                    false,
                )
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn outline_aim_halo_keeps_neighbouring_extra_liquid_priority() {
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        let (mut store, mut registry, _) = fixture(space, &target);
        add_aim_liquid_halo(&mut registry);
        store
            .update_block(
                SubChunkKey::new(0, 0, 0, 0),
                BlockUpdate::new(0, 0, 0, 1, 9),
                0,
            )
            .unwrap();
        let world = PaletteWorld::new(&store, &registry, 0, Default::default());
        let origin = Vec3::new(0.5, 0.5, -0.25);
        let end = Vec3::new(0.5, 0.5, 2.0);
        let cube = world
            .camera_aim_ray(origin, end, 4, false, true)
            .unwrap()
            .unwrap();
        let outline = world
            .camera_aim_ray(origin, end, 4, true, true)
            .unwrap()
            .unwrap();
        assert_eq!(cube.runtime_id, 9);
        assert_eq!(outline.runtime_id, cube.runtime_id, "{space:?}");
        assert_eq!(outline.distance, cube.distance);
        assert!(outline.targetable);
    }
}

#[test]
fn outline_aim_halo_does_not_make_neighbouring_flow_targetable() {
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        let (mut store, mut registry, _) = fixture(space, &target);
        add_aim_liquid_halo(&mut registry);
        set_block(&mut store, [0; 3], 9);
        let world = PaletteWorld::new(&store, &registry, 0, Default::default());
        let origin = Vec3::new(0.5, 0.5, -0.25);
        let end = Vec3::new(0.5, 0.5, 2.0);
        for outlines in [false, true] {
            let hit = world
                .camera_aim_ray(origin, end, 4, outlines, true)
                .unwrap()
                .unwrap();
            assert_eq!(hit.runtime_id, 9);
            assert!(!hit.targetable, "{space:?} outlines={outlines}");
        }
    }
}

/// Adds a flowing liquid and a displaced selection owner to exercise a nonzero halo.
fn add_aim_liquid_halo(registry: &mut CollisionRegistry) {
    registry
        .register_physics(
            9,
            [],
            0.6,
            1.0,
            1.0,
            0.5,
            crate::BlockPhysicsFlags::WATER,
            crate::SurfaceResponse::None,
        )
        .unwrap();
    registry.set_flow_facts(
        9,
        crate::FlowBlockFacts {
            blocks_motion: false,
            is_solid: false,
            blocked_faces: 0,
            allowed_faces: 63,
            liquid_depth: Some(1),
        },
    );
    let mut liquid = shapes(SelectionContext::Static, vec![UNIT], vec![]);
    liquid.pick_rule = PickRule::Liquid;
    registry.set_compiled_selection(9, &liquid);
    registry.register(20, []).unwrap();
    registry.set_compiled_selection(20, &shapes(SelectionContext::MovingBlock, vec![], vec![]));
}
