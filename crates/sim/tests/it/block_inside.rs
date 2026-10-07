use std::collections::BTreeMap;

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, MovementMode, PlayerState, Simulator, SurfaceResponse, TickResult, Vec3,
    WorldQueryError,
};

/// Explicit collision boxes and per-cell facts; unlisted cells are air.
#[derive(Default)]
struct Cells {
    boxes: Vec<Aabb>,
    facts: BTreeMap<[i32; 3], BlockPhysicsFacts>,
}

fn facts(flags: BlockPhysicsFlags, response: SurfaceResponse) -> BlockPhysicsFacts {
    BlockPhysicsFacts {
        friction: 0.6,
        horizontal_speed_factor: 1.0,
        vertical_speed_factor: 1.0,
        fluid_height_blocks: 0.0,
        flags,
        surface_response: response,
    }
}

fn slowing(flags: BlockPhysicsFlags, horizontal: f64, vertical: f64) -> BlockPhysicsFacts {
    BlockPhysicsFacts {
        horizontal_speed_factor: horizontal,
        vertical_speed_factor: vertical,
        ..facts(flags, SurfaceResponse::None)
    }
}

fn water(response: SurfaceResponse) -> BlockPhysicsFacts {
    BlockPhysicsFacts {
        fluid_height_blocks: 1.0,
        ..facts(
            BlockPhysicsFlags::from_bits(
                BlockPhysicsFlags::WATER.bits() | BlockPhysicsFlags::PASSABLE.bits(),
            )
            .unwrap(),
            response,
        )
    }
}

impl Cells {
    fn floor(top: f64) -> Self {
        Self {
            boxes: vec![Aabb::new(
                Vec3::new(-8.0, 0.0, -8.0),
                Vec3::new(8.0, top, 8.0),
            )],
            ..Self::default()
        }
    }

    fn with(mut self, block: [i32; 3], facts: BlockPhysicsFacts) -> Self {
        self.facts.insert(block, facts);
        self
    }
}

impl CollisionWorld for Cells {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.boxes
                .iter()
                .copied()
                .filter(|shape| shape.intersects(query))
                .collect(),
        ))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let air = facts(BlockPhysicsFlags::PASSABLE, SurfaceResponse::None);
        Ok(BlockPhysicsSample {
            layers: Box::new([self.facts.get(&block).copied().unwrap_or(air)]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn tick(world: &Cells, state: &mut PlayerState, input: MovementInput) -> TickResult {
    Simulator::default().tick(state, input, world).unwrap()
}

fn grounded(position: Vec3) -> PlayerState {
    let mut state = PlayerState::new(position);
    state.on_ground = true;
    state
}

fn jump() -> MovementInput {
    MovementInput {
        jumping: true,
        jump_pressed: true,
        ..MovementInput::default()
    }
}

fn forward() -> MovementInput {
    MovementInput {
        forward: 1.0,
        ..MovementInput::default()
    }
}

/// Honey at the feet cell, or below integer-aligned feet, scales the jump power by 0.6.
#[test]
fn honey_under_the_feet_scales_the_jump_power() {
    let honey = facts(BlockPhysicsFlags::default(), SurfaceResponse::Honey);
    let sunk = Cells::floor(0.9375).with([0, 0, 0], honey);
    let mut state = grounded(Vec3::new(0.5, 0.9375, 0.5));
    assert_eq!(tick(&sunk, &mut state, jump()).movement.y as f32, 0.42_f32 * 0.6);

    let aligned = Cells::floor(1.0).with([0, 0, 0], honey);
    let mut state = grounded(Vec3::new(0.5, 1.0, 0.5));
    assert_eq!(tick(&aligned, &mut state, jump()).movement.y as f32, 0.42_f32 * 0.6);
}

/// Honey the body merely touches beside it leaves the ordinary jump.
#[test]
fn a_honey_wall_beside_the_body_does_not_scale_the_jump() {
    let world = Cells::floor(1.0).with(
        [1, 1, 0],
        facts(BlockPhysicsFlags::default(), SurfaceResponse::Honey),
    );
    let mut state = grounded(Vec3::new(0.75, 1.0, 0.5));
    assert_eq!(tick(&world, &mut state, jump()).movement.y as f32, 0.42_f32);
}

/// Each honey cell the moved body is inside damps x and z by 0.4 and caps the fall at -0.12.
#[test]
fn honey_cells_inside_the_body_cap_the_fall_after_the_move() {
    let honey = facts(BlockPhysicsFlags::default(), SurfaceResponse::Honey);
    let world = Cells::default().with([1, 4, 0], honey);
    let mut state = PlayerState::new(Vec3::new(0.75, 4.2, 0.5));
    state.velocity = Vec3::new(0.0, -0.5, 0.1);
    let mut free = state.clone();
    let result = tick(&world, &mut state, MovementInput::default());
    let expected = tick(&Cells::default(), &mut free, MovementInput::default());
    assert_eq!(result.movement, expected.movement);
    assert_eq!(state.velocity.y as f32, -0.12_f32);
    assert_eq!(state.velocity.z as f32, free.velocity.z as f32 * 0.4);
}

/// Standing on slime or honey damps x and z by |vy| * 0.2 + 0.4 after friction, unless sneaking.
#[test]
fn standing_on_slime_damps_horizontal_velocity_after_friction() {
    let slime = BlockPhysicsFacts {
        friction: 0.8,
        ..facts(BlockPhysicsFlags::default(), SurfaceResponse::Slime)
    };
    let plain = BlockPhysicsFacts {
        friction: 0.8,
        ..facts(BlockPhysicsFlags::default(), SurfaceResponse::None)
    };
    let walk = |surface, sneaking| {
        let world = Cells::floor(1.0).with([0, 0, 0], surface);
        let mut state = grounded(Vec3::new(0.5, 1.0, 0.5));
        let result = tick(
            &world,
            &mut state,
            MovementInput {
                sneaking,
                ..forward()
            },
        );
        (result, state.velocity)
    };
    let (damped, velocity) = walk(slime, false);
    let (undamped, reference) = walk(plain, false);
    assert_eq!(damped.movement, undamped.movement);
    let factor = (velocity.y as f32).abs() * 0.2 + 0.4;
    assert_eq!(velocity.z as f32, reference.z as f32 * factor);
    assert_eq!(walk(slime, true).1, walk(plain, true).1);
}

/// A berry bush scales the move by (0.8, 0.75, 0.8) once and leaves no residual velocity.
#[test]
fn a_berry_bush_slows_the_move_once_and_zeroes_velocity() {
    let bush = slowing(BlockPhysicsFlags::PASSABLE, 0.8, 0.75);
    let world = Cells::default().with([0, 4, 0], bush);
    let mut state = PlayerState::new(Vec3::new(0.5, 4.2, 0.5));
    state.velocity = Vec3::new(0.4, -0.4, 0.4);
    let result = tick(&world, &mut state, MovementInput::default());
    assert_eq!(result.movement.x as f32, 0.4_f32 * 0.8);
    assert_eq!(result.movement.y as f32, -0.4_f32 * 0.75);
    assert_eq!(state.velocity.x, 0.0);
    assert_eq!(state.velocity.z, 0.0);
    assert_eq!(state.velocity.y as f32, -0.08_f32 * 0.98);

    // Ground acceleration inside the bush is not slowed a second time.
    let grounded_bush = Cells::floor(1.0).with([0, 1, 0], bush);
    let mut slowed = grounded(Vec3::new(0.5, 1.0, 0.5));
    let mut open = grounded(Vec3::new(0.5, 1.0, 0.5));
    let slowed = tick(&grounded_bush, &mut slowed, forward());
    let open = tick(&Cells::floor(1.0), &mut open, forward());
    assert_eq!(slowed.movement.z as f32, open.movement.z as f32 * 0.8);
}

/// Powder snow scales the move by (0.9, 1.5, 0.9); overlapping blocks keep the per-axis minimum.
#[test]
fn powder_snow_slows_and_overlapping_stuck_blocks_merge_per_axis() {
    let snow = slowing(BlockPhysicsFlags::POWDER_SNOW, 0.9, 1.5);
    let bush = slowing(BlockPhysicsFlags::PASSABLE, 0.8, 0.75);
    let moved = |world: &Cells| {
        let mut state = PlayerState::new(Vec3::new(0.5, 4.2, 0.5));
        state.velocity = Vec3::new(0.4, -0.4, 0.4);
        tick(world, &mut state, MovementInput::default()).movement
    };
    let alone = moved(&Cells::default().with([0, 4, 0], snow));
    assert_eq!(alone.x as f32, 0.4_f32 * 0.9);
    assert_eq!(alone.y as f32, -0.4_f32 * 1.5);
    let both = moved(&Cells::default().with([0, 4, 0], snow).with([0, 5, 0], bush));
    assert_eq!(both.x as f32, 0.4_f32 * 0.8);
    assert_eq!(both.y as f32, -0.4_f32 * 0.75);
}

/// Stuck blocks also slow gliding and survival flight; only creative flight is immune.
#[test]
fn stuck_blocks_slow_every_travel_mode_except_creative_flight() {
    let web = slowing(BlockPhysicsFlags::COBWEB, 0.25, 0.05);
    let world = Cells::default().with([0, 4, 0], web);
    let moved = |mode, creative_flight| {
        let mut state = PlayerState::new(Vec3::new(0.5, 4.2, 0.5));
        state.velocity = Vec3::new(0.4, 0.0, 0.4);
        tick(
            &world,
            &mut state,
            MovementInput {
                mode,
                creative_flight,
                ..MovementInput::default()
            },
        )
        .movement
        .x
    };
    let open = |mode, creative_flight| {
        let mut state = PlayerState::new(Vec3::new(0.5, 4.2, 0.5));
        state.velocity = Vec3::new(0.4, 0.0, 0.4);
        tick(
            &Cells::default(),
            &mut state,
            MovementInput {
                mode,
                creative_flight,
                ..MovementInput::default()
            },
        )
        .movement
        .x
    };
    for mode in [MovementMode::Gliding, MovementMode::Flying] {
        assert_eq!(moved(mode, false) as f32, open(mode, false) as f32 * 0.25);
    }
    assert_eq!(moved(MovementMode::Flying, true), open(MovementMode::Flying, true));
}

/// Bubble columns push once per cell after the move: inside the column, then at its surface.
#[test]
fn bubble_columns_push_per_cell_inside_then_at_the_surface() {
    let column = |response| {
        Cells::default()
            .with([0, 4, 0], water(response))
            .with([0, 5, 0], water(response))
    };
    let settle = |world: &Cells, mode| {
        let mut state = PlayerState::new(Vec3::new(0.5, 4.5, 0.5));
        tick(
            world,
            &mut state,
            MovementInput {
                mode,
                ..MovementInput::default()
            },
        );
        state.velocity.y as f32
    };
    for mode in [MovementMode::Walking, MovementMode::Swimming] {
        let still = settle(&column(SurfaceResponse::None), mode);
        let up = settle(&column(SurfaceResponse::BubbleUp), mode);
        assert_eq!(up, ((still + 0.06).min(0.7) + 0.1).min(1.8), "{mode:?}");
        let down = settle(&column(SurfaceResponse::BubbleDown), mode);
        assert_eq!(down, ((still - 0.03).max(-0.3) - 0.03).max(-0.9), "{mode:?}");
    }
    let flying = column(SurfaceResponse::BubbleUp);
    assert_eq!(
        settle(&flying, MovementMode::Flying),
        settle(&column(SurfaceResponse::None), MovementMode::Flying)
    );
}

/// Soul sand acceleration friction follows the friction probe, not the deeper support cell.
#[test]
fn soul_sand_acceleration_reads_the_friction_probe_block() {
    let sand = facts(BlockPhysicsFlags::default(), SurfaceResponse::SoulSand);
    let covered = Cells::floor(1.125).with([0, 0, 0], sand);
    let mut on_cover = grounded(Vec3::new(0.5, 1.125, 0.5));
    let mut on_plain = grounded(Vec3::new(0.5, 1.125, 0.5));
    let covered = tick(&covered, &mut on_cover, forward());
    let plain = tick(&Cells::floor(1.125), &mut on_plain, forward());
    assert_eq!(covered.movement, plain.movement);
}
