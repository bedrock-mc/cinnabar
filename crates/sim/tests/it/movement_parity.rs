use std::collections::BTreeMap;

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementEffects, MovementInput, PlayerState, ProvenancedCollider, Simulator, SurfaceResponse,
    Vec3, WorldQueryError,
};

#[derive(Default)]
struct TestWorld {
    blocks: BTreeMap<[i32; 3], BlockPhysicsFacts>,
    colliders: Vec<ProvenancedCollider>,
    climbable: bool,
}

impl TestWorld {
    /// Adds a solid block with an explicit material and source cell.
    fn solid(&mut self, block: [i32; 3], height: f64, response: SurfaceResponse) {
        let min = Vec3::new(block[0].into(), block[1].into(), block[2].into());
        self.colliders.push(ProvenancedCollider {
            aabb: Aabb::new(min, min + Vec3::new(1.0, height, 1.0)),
            block: Some(block),
            runtime_id: None,
        });
        self.blocks.insert(block, facts(response));
    }
}

impl CollisionWorld for TestWorld {
    /// Exposes the same filtered shapes through the compatibility query.
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let colliders = self.collision_boxes_with_provenance(query)?;
        Ok(CollisionQuery {
            value: colliders
                .value
                .into_iter()
                .map(|entry| entry.aabb)
                .collect(),
            identity: colliders.identity,
        })
    }

    /// Preserves cell provenance while selecting the queried shapes.
    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.colliders
                .iter()
                .copied()
                .filter(|entry| entry.aabb.intersects(query))
                .collect(),
        ))
    }

    /// Returns sparse fixture materials with an optional ladder volume.
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut value = self
            .blocks
            .get(&block)
            .copied()
            .unwrap_or_else(|| facts(SurfaceResponse::None));
        if self.climbable {
            value.flags = BlockPhysicsFlags::CLIMBABLE;
        }
        Ok(BlockPhysicsSample {
            layers: Box::new([value]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

/// Makes ordinary block facts with one independently selected surface response.
fn facts(surface_response: SurfaceResponse) -> BlockPhysicsFacts {
    BlockPhysicsFacts {
        friction: 0.6,
        horizontal_speed_factor: 1.0,
        vertical_speed_factor: 1.0,
        fluid_height_blocks: 0.0,
        flags: BlockPhysicsFlags::default(),
        surface_response,
    }
}

#[test]
fn thin_support_uses_its_own_ground_friction() {
    let mut movements = Vec::new();
    for lower_friction in [0.6, 0.98] {
        let mut world = TestWorld::default();
        world.solid([0, 0, 0], 1.0, SurfaceResponse::None);
        world.blocks.get_mut(&[0, 0, 0]).unwrap().friction = lower_friction;
        world.solid([0, 1, 0], 0.25, SurfaceResponse::None);
        let mut state = PlayerState::new(Vec3::new(0.5, 1.25, 0.5));
        state.on_ground = true;
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    forward: 1.0,
                    move_vector_is_raw: true,
                    ..Default::default()
                },
                &world,
            )
            .unwrap();
        movements.push(tick.movement.z);
    }
    assert_eq!(movements[0], movements[1]);
    assert_eq!(movements[0], f64::from(0.098_000_005_f32));
}

#[test]
fn covered_slime_cannot_bounce_a_stone_landing() {
    let mut world = TestWorld::default();
    world.solid([0, 0, 0], 1.0, SurfaceResponse::Slime);
    world.solid([0, 1, 0], 1.0, SurfaceResponse::None);
    let mut state = PlayerState::new(Vec3::new(0.5, 3.5, 0.5));
    state.velocity.y = -3.0;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert_eq!(tick.position.y, 2.0);
    assert!(tick.on_ground);
    assert_eq!(tick.velocity.y, f64::from(-0.08_f32 * 0.98_f32));
    assert_eq!(tick.environment.surface_response, SurfaceResponse::None);
}

#[test]
fn landing_material_does_not_erase_a_contacting_bubble_column() {
    let mut world = TestWorld::default();
    world.solid([0, 0, 0], 1.0, SurfaceResponse::None);
    world.blocks.insert(
        [0, 1, 0],
        BlockPhysicsFacts {
            flags: BlockPhysicsFlags::WATER,
            fluid_height_blocks: 1.0,
            ..facts(SurfaceResponse::BubbleUp)
        },
    );
    let mut state = PlayerState::new(Vec3::new(0.5, 1.1, 0.5));
    state.velocity.y = -0.3;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(tick.collisions.y);
    assert_eq!(tick.environment.surface_response, SurfaceResponse::BubbleUp);
    assert_eq!(tick.velocity.y, 0.1);
}

#[test]
fn shallow_slime_landing_is_below_restitution_threshold() {
    let mut world = TestWorld::default();
    world.solid([0, 0, 0], 1.0, SurfaceResponse::Slime);
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.velocity.y = -0.02;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(tick.collisions.y);
    assert_eq!(tick.velocity.y, f64::from(-0.08_f32 * 0.98_f32));
}

#[test]
fn landing_without_collider_provenance_does_not_guess_a_material() {
    let mut world = TestWorld::default();
    world.solid([0, 0, 0], 1.0, SurfaceResponse::Slime);
    world.colliders[0].block = None;
    let mut state = PlayerState::new(Vec3::new(0.5, 1.5, 0.5));
    state.velocity.y = -0.7;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert_eq!(tick.velocity.y, f64::from(-0.08_f32 * 0.98_f32));
    assert_eq!(tick.environment.surface_response, SurfaceResponse::None);
}

#[test]
fn landing_material_uses_nearest_collider_at_equal_height() {
    let mut world = TestWorld::default();
    world.solid([0, 0, 0], 1.0, SurfaceResponse::Slime);
    world.solid([1, 0, 0], 1.0, SurfaceResponse::None);
    for (x, response) in [(0.9, SurfaceResponse::Slime), (1.1, SurfaceResponse::None)] {
        let mut state = PlayerState::new(Vec3::new(x, 1.5, 0.5));
        state.velocity.y = -0.7;
        let tick = Simulator::default()
            .tick(&mut state, MovementInput::default(), &world)
            .unwrap();
        assert_eq!(tick.environment.surface_response, response);
        assert_eq!(tick.velocity.y > 0.0, response == SurfaceResponse::Slime);
    }
}

#[test]
fn ladder_wall_collision_retains_climb_speed_after_travel() {
    let mut world = TestWorld {
        climbable: true,
        ..Default::default()
    };
    world.solid([0, 0, 1], 8.0, SurfaceResponse::None);
    let mut state = PlayerState::new(Vec3::new(0.5, 2.0, 0.7));
    state.velocity = Vec3::new(0.0, -0.1, 0.2);
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(tick.collisions.z);
    assert_eq!(tick.velocity.y, f64::from(0.2_f32));
}

#[test]
fn levitation_runs_before_vertical_drag_with_float_order_preserved() {
    let mut state = PlayerState::new(Vec3::new(0.5, 4.0, 0.5));
    state.velocity.y = -0.4;
    Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                effects: MovementEffects {
                    levitation: Some(0),
                    ..Default::default()
                },
                ..Default::default()
            },
            &TestWorld::default(),
        )
        .unwrap();
    assert_eq!((state.velocity.y as f32).to_bits(), 0xbe9b_8bae);
}

#[test]
fn horizontal_epsilon_is_applied_per_axis_before_drag() {
    for tiny in [1.0e-8_f32, f32::EPSILON, -f32::EPSILON] {
        let mut state = PlayerState::new(Vec3::new(0.5, 4.0, 0.5));
        state.velocity = Vec3::new(f64::from(tiny), 0.1, 0.01);
        Simulator::default()
            .tick(&mut state, MovementInput::default(), &TestWorld::default())
            .unwrap();
        assert_eq!(state.velocity.x, 0.0);
        assert_eq!(state.velocity.z, f64::from(0.01_f32 * 0.91_f32));
    }
    let mut state = PlayerState::new(Vec3::new(0.5, 4.0, 0.5));
    let above = f32::from_bits(f32::EPSILON.to_bits() + 1);
    state.velocity = Vec3::new(f64::from(above), 0.1, f64::from(-above));
    Simulator::default()
        .tick(&mut state, MovementInput::default(), &TestWorld::default())
        .unwrap();
    assert_eq!(state.velocity.x, f64::from(above * 0.91_f32));
    assert_eq!(state.velocity.z, f64::from(-above * 0.91_f32));
}

#[test]
fn swift_sneak_scales_raw_crouch_and_crawl_controls_and_caps_at_full_speed() {
    for mode in [sim::MovementMode::Walking, sim::MovementMode::Crawling] {
        for (level, factor) in [(0, 0.3_f32), (1, 0.45), (3, 0.75), (5, 1.0)] {
            let mut state = PlayerState::new(Vec3::new(0.5, 4.0, 0.5));
            let output = Simulator::default()
                .tick_with_controls(
                    &mut state,
                    MovementInput {
                        forward: 0.5,
                        move_vector_is_raw: true,
                        sneaking: mode == sim::MovementMode::Walking,
                        swift_sneak: level,
                        mode,
                        ..Default::default()
                    },
                    &TestWorld::default(),
                )
                .unwrap();
            assert!(
                (output.controls.move_vector[1] - f64::from(0.5 * factor)).abs()
                    <= f64::from(f32::EPSILON)
            );
        }
    }
}

#[test]
fn requested_displacement_survives_wall_clipping_and_clears_when_immobile() {
    let mut world = TestWorld::default();
    world.solid([0, 0, 1], 8.0, SurfaceResponse::None);
    let mut state = PlayerState::new(Vec3::new(0.5, 2.0, 0.7));
    state.velocity = Vec3::new(0.125, 0.0, 0.25);
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(tick.collisions.z);
    assert_eq!(state.requested_movement, Vec3::new(0.125, 0.0, 0.25));
    assert!(state.movement.z < state.requested_movement.z);
    Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                immobile: true,
                ..Default::default()
            },
            &world,
        )
        .unwrap();
    assert_eq!(state.requested_movement, Vec3::ZERO);
}
