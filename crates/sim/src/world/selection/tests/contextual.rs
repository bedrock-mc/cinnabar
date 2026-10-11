use super::{
    CollisionIdSpace, PISTON_DIRECTIONS, PaletteWorld, PickRule, SelectionContext, ShapeBox,
    ShapeFlags, UNIT, Vec3, box_to_aabb, fixture, set_block, set_moving, set_piston, shapes,
};
use crate::WorldQueryError;
use assets::block_shapes::CompiledFlow;
use assets::block_shapes::PickContext;
use world::block_state_identity::state_identity;

#[test]
fn unsupported_moving_states_do_not_require_an_unloaded_piston_column() {
    check_unsupported_moving_ray(0.5);
}

#[test]
fn unsupported_moving_states_in_the_halo_do_not_block_valid_rays() {
    check_unsupported_moving_ray(1.5);
}

/// Checks unknown and geometry-free carried states against an unloaded remote owner.
fn check_unsupported_moving_ray(z: f64) {
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        for known_without_visual in [false, true] {
            let mut target = shapes(SelectionContext::MovingBlock, vec![], vec![]);
            target.actor_visual = None;
            let (mut store, mut registry, _) = fixture(space, &target);
            let mut carried = shapes(SelectionContext::Static, vec![], vec![]);
            carried.state_identity = state_identity("minecraft:unsupported", []);
            carried.actor_visual = None;
            if known_without_visual {
                registry.register(9, []).unwrap();
                registry.set_compiled_selection(9, &carried);
            }
            let mut behind = shapes(SelectionContext::Static, vec![UNIT], vec![]);
            behind.state_identity = [21; 32];
            registry.register(21, [box_to_aabb(UNIT)]).unwrap();
            registry.set_compiled_selection(21, &behind);
            let expected = [1, 0, z.floor() as i32];
            set_block(&mut store, expected, 21);
            let owner = [48, 0, 0];
            set_moving(&mut store, "minecraft:unsupported", owner, true);
            let view = PaletteWorld::new(&store, &registry, 0, Default::default());
            let origin = Vec3::new(-2.0, 0.5, z);
            let direction = Vec3::new(1.0, 0.0, 0.0);
            let hit = view
                .block_interaction_ray_current(origin, direction, 4.0)
                .unwrap()
                .unwrap();
            assert_eq!(hit.block_pos, expected);
            assert!(
                !hit.identity
                    .chunks
                    .iter()
                    .any(|revision| revision.chunk.x == 3)
            );
            assert!(
                view.block_interaction_ray(origin, direction, 4.0, &hit.identity)
                    .unwrap()
                    .is_some()
            );
            assert_eq!(
                view.camera_visibility_ray(origin, direction, 4.0)
                    .unwrap()
                    .map(|hit| hit.block_pos),
                Some(expected)
            );
            assert!(view.unsupported_actor_selection_count() > 0);
        }
    }
}

#[test]
fn moving_pick_identity_includes_a_remote_piston_owner() {
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::MovingBlock, vec![], vec![]);
        let (mut store, mut registry, _) = fixture(space, &target);
        let mut carried = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        carried.state_identity = state_identity("minecraft:carried", []);
        registry.register(9, [box_to_aabb(UNIT)]).unwrap();
        registry.set_compiled_selection(9, &carried);
        let mut piston = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        piston.state_identity = [20; 32];
        piston.actor_facing = Some(4);
        registry.register(20, [box_to_aabb(UNIT)]).unwrap();
        registry.set_compiled_selection(20, &piston);
        let owner = [20, 0, 0];
        set_block(&mut store, owner, 20);
        set_piston(&mut store, owner, 0.5);
        set_moving(&mut store, "minecraft:carried", owner, false);
        let origin = Vec3::new(0.75, 0.5, 2.0);
        let direction = Vec3::new(0.0, 0.0, -1.0);
        let hit = PaletteWorld::new(&store, &registry, 0, Default::default())
            .block_interaction_ray_current(origin, direction, 4.0)
            .unwrap()
            .unwrap();
        let owner_chunk = world::ChunkKey::new(0, 1, 0);
        assert!(
            hit.identity
                .chunks
                .iter()
                .any(|revision| revision.chunk == owner_chunk)
        );
        let historical = store.collision_snapshot();
        set_piston(&mut store, owner, 0.25);
        let current = PaletteWorld::new(&store, &registry, 0, Default::default())
            .block_interaction_ray(origin, direction, 4.0, &hit.identity);
        assert!(
            matches!(current, Err(WorldQueryError::StaleCollisionIdentity { chunk }) if chunk == owner_chunk),
            "{current:?}"
        );
        assert!(
            PaletteWorld::new(&historical, &registry, 0, Default::default())
                .block_interaction_ray(origin, direction, 4.0, &hit.identity)
                .unwrap()
                .is_some()
        );
    }
}

#[test]
fn moving_blocks_resolve_carried_static_visuals_in_both_id_spaces() {
    let carried = ShapeBox::new([0.25; 3], [0.75; 3]);
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::MovingBlock, vec![], vec![]);
        let (mut store, mut registry, id) = fixture(space, &target);
        let mut carried_state = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        carried_state.state_identity = state_identity("minecraft:carried", []);
        carried_state.actor_visual = Some(carried);
        registry.register(9, [box_to_aabb(UNIT)]).unwrap();
        registry.set_compiled_selection(9, &carried_state);
        let physics = registry.physics(id).unwrap();
        assert!(
            PaletteWorld::new(&store, &registry, 0, Default::default())
                .selection_shapes_at([0; 3], physics)
                .unwrap()
                .is_empty()
        );
        set_moving(&mut store, "minecraft:unknown", [0, -1, 0], true);
        assert!(
            PaletteWorld::new(&store, &registry, 0, Default::default())
                .selection_shapes_at([0; 3], physics)
                .unwrap()
                .is_empty()
        );
        set_moving(&mut store, "minecraft:carried", [0, -1, 0], true);
        assert_eq!(
            PaletteWorld::new(&store, &registry, 0, Default::default())
                .selection_shapes_at([0; 3], physics)
                .unwrap()
                .as_ref(),
            [box_to_aabb(carried)]
        );
        for (facing, direction) in PISTON_DIRECTIONS.into_iter().enumerate() {
            let mut owner_state = shapes(SelectionContext::Static, vec![UNIT], vec![]);
            owner_state.state_identity = [20 + facing as u8; 32];
            owner_state.actor_facing = Some(facing as u8);
            registry.register(20, [box_to_aabb(UNIT)]).unwrap();
            registry.set_compiled_selection(20, &owner_state);
            let owner = [-2, 0, 0];
            set_block(&mut store, owner, 20);
            set_piston(&mut store, owner, 0.5);
            for expanding in [false, true] {
                set_moving(&mut store, "minecraft:carried", owner, expanding);
                let offset =
                    direction.map(|value| f64::from(value) * if expanding { -0.5 } else { 0.5 });
                let expected =
                    box_to_aabb(carried).translated(Vec3::new(offset[0], offset[1], offset[2]));
                let actual = PaletteWorld::new(&store, &registry, 0, Default::default())
                    .selection_shapes_at([0; 3], registry.physics(id).unwrap())
                    .unwrap();
                assert_eq!(actual.as_ref(), [expected]);
            }
        }
    }
}

#[test]
fn scaffold_and_unknown_portal_choose_only_their_compiled_alternates() {
    let base = ShapeBox::new([0.0; 3], [1.0, 0.125, 1.0]);
    let alternate = ShapeBox::new([0.0, 0.875, 0.0], [1.0; 3]);
    for (context, flag, neighbour, alternate_without_support) in [
        (
            SelectionContext::ScaffoldSupport,
            ShapeFlags::FULL_TOP_SUPPORT,
            [0, -1, 0],
            true,
        ),
        (
            SelectionContext::PortalAxis,
            ShapeFlags::PORTAL,
            [1, 0, 0],
            false,
        ),
    ] {
        for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
            let target = shapes(context, vec![base], vec![alternate]);
            let (mut store, mut registry, id) = fixture(space, &target);
            let mut support = shapes(SelectionContext::Static, vec![UNIT], vec![]);
            support.flags = flag;
            support.state_identity = [9; 32];
            registry.register(9, [box_to_aabb(UNIT)]).unwrap();
            registry.set_compiled_selection(9, &support);
            for supported in [false, true] {
                set_block(&mut store, neighbour, if supported { 9 } else { 0 });
                let expected = if supported != alternate_without_support {
                    alternate
                } else {
                    base
                };
                assert_eq!(
                    PaletteWorld::new(&store, &registry, 0, Default::default())
                        .selection_shapes_at([0; 3], registry.physics(id).unwrap())
                        .unwrap()
                        .as_ref(),
                    [box_to_aabb(expected)]
                );
            }
        }
    }
}

#[test]
fn interaction_ray_enforces_pick_context_instead_of_movement_occupancy() {
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        for (rule, context, depth, expected) in [
            (PickRule::Never, PickContext::default(), None, false),
            (PickRule::Always, PickContext::default(), None, true),
            (
                PickRule::HeldLight,
                PickContext {
                    creative: true,
                    ..Default::default()
                },
                None,
                false,
            ),
            (
                PickRule::HeldLight,
                PickContext {
                    holding_light: true,
                    ..Default::default()
                },
                None,
                true,
            ),
            (PickRule::Creative, PickContext::default(), None, false),
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
            if let Some(depth) = depth {
                target.flow = Some(CompiledFlow {
                    blocks_motion: false,
                    is_solid: false,
                    blocked_faces: 0,
                    allowed_faces: 0x3f,
                    liquid_depth: Some(depth),
                });
            }
            let (store, mut registry, id) = fixture(space, &target);
            if let Some(depth) = depth {
                registry.set_flow_facts(
                    id,
                    crate::world::FlowBlockFacts {
                        blocks_motion: false,
                        is_solid: false,
                        blocked_faces: 0,
                        allowed_faces: 0x3f,
                        liquid_depth: Some(depth),
                    },
                );
            }
            let view = PaletteWorld::new(&store, &registry, 0, context);
            let hit = view
                .block_interaction_ray_current(
                    Vec3::new(-2.0, 0.5, 0.5),
                    Vec3::new(1.0, 0.0, 0.0),
                    4.0,
                )
                .unwrap();
            assert_eq!(hit.is_some(), expected, "{space:?} {rule:?}");
            if let Some(hit) = hit {
                assert_eq!(hit.block_pos, [0; 3]);
            }
        }
    }
}

#[test]
fn moving_ray_hits_geometry_outside_its_owner_cell() {
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
                let hit = PaletteWorld::new(&store, &registry, 0, Default::default())
                    .block_interaction_ray_current(origin, ray, 4.0)
                    .unwrap()
                    .expect("translated moving box must be reachable from the adjacent cell");
                assert_eq!(hit.block_pos, [0; 3], "{space:?} {facing} {expanding}");
                assert_eq!(hit.distance, 1.0);
            }
        }
    }
}

#[test]
fn portal_pick_identity_captures_untraversed_neighbour_column() {
    let base = ShapeBox::new([0.0, 0.0, 0.375], [1.0, 1.0, 0.625]);
    let alternate = ShapeBox::new([0.375, 0.0, 0.0], [0.625, 1.0, 1.0]);
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::PortalAxis, vec![base], vec![alternate]);
        let (mut store, mut registry, id) = fixture(space, &target);
        set_block(&mut store, [0; 3], 0);
        set_block(&mut store, [15, 0, 0], id);
        let mut neighbour = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        neighbour.state_identity = [9; 32];
        neighbour.flags = ShapeFlags::PORTAL;
        registry.register(9, []).unwrap();
        registry.set_compiled_selection(9, &neighbour);
        let origin = Vec3::new(15.5, 0.5, 2.0);
        let direction = Vec3::new(0.0, 0.0, -1.0);
        let hit = PaletteWorld::new(&store, &registry, 0, Default::default())
            .block_interaction_ray_current(origin, direction, 4.0)
            .unwrap()
            .unwrap();
        let historical = store.collision_snapshot();
        set_block(&mut store, [16, 0, 0], 9);
        let view = PaletteWorld::new(&store, &registry, 0, Default::default());
        assert_ne!(
            view.block_interaction_ray_current(origin, direction, 4.0)
                .unwrap()
                .unwrap()
                .distance,
            hit.distance
        );
        let current = view.block_interaction_ray(origin, direction, 4.0, &hit.identity);
        let neighbour_column = world::ChunkKey::new(0, 1, 0);
        assert!(
            matches!(current, Err(WorldQueryError::StaleCollisionIdentity { chunk }) if chunk == neighbour_column),
            "{current:?}"
        );
        assert!(
            hit.identity
                .chunks
                .iter()
                .any(|revision| revision.chunk == neighbour_column)
        );
        assert_eq!(
            PaletteWorld::new(&historical, &registry, 0, Default::default())
                .block_interaction_ray(origin, direction, 4.0, &hit.identity)
                .unwrap()
                .unwrap()
                .distance,
            hit.distance
        );
    }
}

#[test]
fn ray_halo_includes_compiled_selection_without_movement_extent() {
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let extended = ShapeBox::new([0.0; 3], [1.5, 1.0, 1.0]);
        let target = shapes(SelectionContext::Static, vec![extended], vec![]);
        let (store, registry, _) = fixture(space, &target);
        let hit = PaletteWorld::new(&store, &registry, 0, Default::default())
            .block_interaction_ray_current(
                Vec3::new(1.25, 0.5, 2.0),
                Vec3::new(0.0, 0.0, -1.0),
                4.0,
            )
            .unwrap()
            .expect("selection bounds can exceed movement bounds");
        assert_eq!(hit.block_pos, [0; 3]);
    }
}

#[test]
fn moving_ray_includes_carried_visual_extent_registered_before_actor() {
    for space in [CollisionIdSpace::Sequential, CollisionIdSpace::Hashed] {
        let target = shapes(SelectionContext::Static, vec![], vec![]);
        let (mut store, mut registry, id) = fixture(space, &target);
        let mut carried = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        carried.state_identity = state_identity("minecraft:carried", []);
        carried.actor_visual = Some(ShapeBox::new([0.0; 3], [1.75, 1.0, 1.0]));
        registry.register(9, []).unwrap();
        registry.set_compiled_selection(9, &carried);
        registry.set_compiled_selection(id, &shapes(SelectionContext::MovingBlock, vec![], vec![]));
        let mut piston = shapes(SelectionContext::Static, vec![UNIT], vec![]);
        piston.state_identity = [20; 32];
        piston.actor_facing = Some(4);
        registry.register(20, []).unwrap();
        registry.set_compiled_selection(20, &piston);
        set_block(&mut store, [-2, 0, 0], 20);
        set_piston(&mut store, [-2, 0, 0], 0.5);
        set_moving(&mut store, "minecraft:carried", [-2, 0, 0], false);
        let hit = PaletteWorld::new(&store, &registry, 0, Default::default())
            .block_interaction_ray_current(
                Vec3::new(2.125, 0.5, 2.0),
                Vec3::new(0.0, 0.0, -1.0),
                4.0,
            )
            .unwrap()
            .expect("carried visual plus actor translation must reach two cells away");
        assert_eq!(hit.block_pos, [0; 3]);
    }
}
