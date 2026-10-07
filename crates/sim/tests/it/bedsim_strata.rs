//! Movement strata whose contract is fixed by the pinned `bedsim v0.1.3`
//! reference simulator rather than by a Cinnabar-local inference.
//!
//! Every expectation below cites the exact reference behaviour it ports so a
//! reviewer can re-derive it without rerunning the generator.

use sim::{
    Aabb, AxisCollisions, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery,
    CollisionWorld, MovementInput, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

#[derive(Clone, Copy)]
struct StrataWorld {
    facts: BlockPhysicsFacts,
    floor: bool,
}

impl CollisionWorld for StrataWorld {
    /// Assigns the homogeneous floor's known material to each fixture collider.
    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<sim::ProvenancedCollider>>, WorldQueryError> {
        let boxes = self.collision_boxes(query)?;
        Ok(CollisionQuery {
            value: boxes
                .value
                .into_iter()
                .map(|aabb| sim::ProvenancedCollider {
                    aabb,
                    block: Some([0, 0, 0]),
                    runtime_id: None,
                })
                .collect(),
            identity: boxes.identity,
        })
    }

    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-8.0, 0.0, -8.0), Vec3::new(8.0, 1.0, 8.0));
        Ok(CollisionQuery::synthetic(
            (self.floor && floor.intersects(query))
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([self.facts]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn world(flags: BlockPhysicsFlags, response: SurfaceResponse, floor: bool) -> StrataWorld {
    StrataWorld {
        facts: BlockPhysicsFacts {
            friction: 0.6,
            horizontal_speed_factor: 1.0,
            vertical_speed_factor: 1.0,
            fluid_height_blocks: 0.0,
            flags,
            surface_response: response,
        },
        floor,
    }
}

/// `bedsim v0.1.3` `simulation.go` `simulateMovement`: a climbable block sets
/// the upward climb velocity when the player is *either* pressing jump *or*
/// horizontally collided on the previous tick
/// (`state.PressingJump || state.CollideX || state.CollideZ`). Walking into a
/// ladder therefore ascends it without any jump input.
#[test]
fn retained_horizontal_collision_climbs_a_ladder_without_a_jump_input() {
    let ladder = world(BlockPhysicsFlags::CLIMBABLE, SurfaceResponse::None, false);

    for collided in [
        AxisCollisions {
            x: true,
            y: false,
            z: false,
        },
        AxisCollisions {
            x: false,
            y: false,
            z: true,
        },
    ] {
        let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
        state.velocity.y = -1.0;
        state.collisions = collided;
        let tick = Simulator::default()
            .tick(&mut state, MovementInput::default(), &ladder)
            .unwrap();
        assert!(tick.environment.on_climbable);
        assert!(
            (tick.movement.y - 0.2).abs() <= f64::from(f32::EPSILON),
            "retained collision {collided:?} must climb at +0.2, got {}",
            tick.movement.y
        );
    }
}

/// The same clause must not fire without a retained horizontal collision: a
/// player merely falling past a ladder is clamped to the `-ClimbSpeed` descent
/// rather than pulled upward.
#[test]
fn climbing_requires_a_retained_collision_or_a_held_jump() {
    let ladder = world(BlockPhysicsFlags::CLIMBABLE, SurfaceResponse::None, false);
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.velocity.y = -1.0;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &ladder)
        .unwrap();
    assert!((tick.movement.y + 0.2).abs() <= f64::from(f32::EPSILON));
}

/// A tick's resolved axis collisions must survive into the next tick so the
/// climb clause above can read them. Nothing else in the simulator retained
/// them before.
#[test]
fn resolved_axis_collisions_are_retained_for_the_next_tick() {
    let solid = world(BlockPhysicsFlags::default(), SurfaceResponse::None, true);
    let mut state = PlayerState::new(Vec3::new(0.0, 1.6, 0.0));
    state.velocity.y = -1.0;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &solid)
        .unwrap();
    assert!(tick.collisions.y);
    assert_eq!(state.collisions, tick.collisions);
}

/// The same reference function refuses to damp while sneaking
/// (`!state.Sneaking` at entry and `!state.PressingSneak` inside).
///
/// Comparing against an otherwise identical ordinary surface isolates the slime
/// stratum exactly: sneak edge clipping applies to both, so any difference at
/// all would be the walk damping leaking through.
#[test]
fn sneaking_on_slime_does_not_damp_horizontal_velocity() {
    let sneak = MovementInput {
        sneaking: true,
        ..MovementInput::default()
    };
    let mut slime_state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    slime_state.on_ground = true;
    slime_state.velocity = Vec3::new(0.3, 0.0, 0.3);
    let slime = Simulator::default()
        .tick(
            &mut slime_state,
            sneak,
            &world(BlockPhysicsFlags::default(), SurfaceResponse::Slime, true),
        )
        .unwrap();

    let mut plain_state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    plain_state.on_ground = true;
    plain_state.velocity = Vec3::new(0.3, 0.0, 0.3);
    let plain = Simulator::default()
        .tick(
            &mut plain_state,
            sneak,
            &world(BlockPhysicsFlags::default(), SurfaceResponse::None, true),
        )
        .unwrap();

    assert_eq!(slime.movement, plain.movement);
    assert_eq!(slime_state.velocity, plain_state.velocity);
}

/// `bedsim v0.1.3` `landOnBlock` returns a zeroed vertical velocity whenever
/// `state.PressingSneak` is set, for *every* surface. The bed arm previously
/// ignored sneaking and bounced the player anyway.
#[test]
fn sneaking_suppresses_the_bed_bounce() {
    let bed = world(BlockPhysicsFlags::default(), SurfaceResponse::Bed, true);
    let mut state = PlayerState::new(Vec3::new(0.0, 1.2, 0.0));
    state.velocity.y = -0.7;
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                sneaking: true,
                ..MovementInput::default()
            },
            &bed,
        )
        .unwrap();
    assert!(tick.collisions.y);
    let expected = (0.0 - 0.08) * 0.98;
    assert!(
        (state.velocity.y - expected).abs() <= f64::from(f32::EPSILON),
        "sneaking must suppress the bed bounce exactly: expected {expected}, got {}",
        state.velocity.y
    );
}

/// `bedsim v0.1.3` `landOnBlock` snaps a slime rebound smaller than `1e-4`
/// straight to zero so a grazing landing cannot leave residual jitter.
#[test]
fn a_negligible_slime_rebound_snaps_to_zero() {
    let slime = world(BlockPhysicsFlags::default(), SurfaceResponse::Slime, true);
    // The descent must be large enough to register a resolved Y collision
    // (>= 1e-5) yet small enough that the mirrored rebound stays under 1e-4.
    let mut state = PlayerState::new(Vec3::new(0.0, 1.0, 0.0));
    state.velocity.y = -5.0e-5;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &slime)
        .unwrap();
    assert!(tick.collisions.y);
    assert_eq!(
        tick.velocity.y,
        f64::from(-0.08_f32 * 0.98_f32),
        "a sub-1e-4 rebound must be zeroed before gravity, got {}",
        tick.velocity.y
    );
}
