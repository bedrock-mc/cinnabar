use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

struct Blocks {
    floor: bool,
    flags: BlockPhysicsFlags,
    response: SurfaceResponse,
    horizontal_factor: f64,
    fluid_height: f64,
}

impl Blocks {
    fn plain() -> Self {
        Self {
            floor: true,
            flags: BlockPhysicsFlags::default(),
            response: SurfaceResponse::None,
            horizontal_factor: 1.0,
            fluid_height: 0.0,
        }
    }
}

impl CollisionWorld for Blocks {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(Vec3::new(-64.0, 0.0, -64.0), Vec3::new(64.0, 1.0, 64.0));
        Ok(CollisionQuery::synthetic(
            (self.floor && floor.intersects(query))
                .then_some(floor)
                .into_iter()
                .collect(),
        ))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: self.horizontal_factor,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: self.fluid_height,
                flags: self.flags,
                surface_response: self.response,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn run(world: &Blocks, mut state: PlayerState, input: MovementInput, ticks: usize) -> PlayerState {
    for _ in 0..ticks {
        Simulator::default().tick(&mut state, input, world).unwrap();
    }
    state
}

fn standing() -> PlayerState {
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.on_ground = true;
    state
}

#[test]
fn honey_lowers_the_jump_impulse() {
    let jump = MovementInput {
        jumping: true,
        jump_pressed: true,
        ..MovementInput::default()
    };
    let normal = run(&Blocks::plain(), standing(), jump, 1);
    let honey = Blocks {
        response: SurfaceResponse::Honey,
        ..Blocks::plain()
    };
    let sticky = run(&honey, standing(), jump, 1);
    assert!(sticky.movement.y > 0.0);
    assert!(sticky.movement.y < normal.movement.y * 0.75);
}

#[test]
fn soul_speed_offsets_the_soul_sand_slowdown() {
    let sand = Blocks {
        response: SurfaceResponse::SoulSand,
        horizontal_factor: 0.543,
        ..Blocks::plain()
    };
    let walk = |soul_speed| {
        run(
            &sand,
            standing(),
            MovementInput {
                forward: 1.0,
                soul_speed,
                ..MovementInput::default()
            },
            1,
        )
        .movement
        .z
        .abs()
    };
    assert!(walk(3) > walk(0));
}

#[test]
fn depth_strider_speeds_water_travel() {
    let water = Blocks {
        floor: false,
        flags: BlockPhysicsFlags::WATER,
        fluid_height: 1.0,
        ..Blocks::plain()
    };
    let swim = |depth_strider| {
        let mut state = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
        state.on_ground = false;
        run(
            &water,
            state,
            MovementInput {
                forward: 1.0,
                depth_strider,
                ..MovementInput::default()
            },
            5,
        )
        .position
        .z
        .abs()
    };
    assert!(swim(3) > swim(0));
}

#[test]
fn a_passable_slowing_block_slows_an_overlapping_body_like_powder_snow() {
    let bush = Blocks {
        floor: false,
        flags: BlockPhysicsFlags::PASSABLE,
        horizontal_factor: 0.5,
        ..Blocks::plain()
    };
    let drift = |world: &Blocks| {
        let mut state = PlayerState::new(Vec3::new(0.5, 50.0, 0.5));
        state.velocity.x = 0.5;
        run(world, state, MovementInput::default(), 1).movement.x
    };
    let free = Blocks {
        floor: false,
        ..Blocks::plain()
    };
    assert!(drift(&bush) < drift(&free) * 0.75);
}
