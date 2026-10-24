use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

struct Contact(Option<SurfaceResponse>);

impl CollisionWorld for Contact {
    /// Keeps the fall airborne so only the contact response changes its distance.
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    /// Places one honey or bubble cell below the falling player.
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let response = if block == [0, 0, 0] { self.0 } else { None };
        let bubble = matches!(
            response,
            Some(SurfaceResponse::BubbleUp | SurfaceResponse::BubbleDown)
        );
        Ok(BlockPhysicsSample {
            identity: CollisionQuery::synthetic(()).identity,
            layers: Box::new([BlockPhysicsFacts {
                friction: sim::DEFAULT_SURFACE_FRICTION,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if bubble { 1.0 } else { 0.0 },
                flags: if bubble {
                    BlockPhysicsFlags::WATER
                } else {
                    BlockPhysicsFlags::PASSABLE
                },
                surface_response: response.unwrap_or(SurfaceResponse::None),
            }]),
        })
    }
}

/// Builds a nonzero distance through ordinary falling rather than editing retained state.
fn falling() -> PlayerState {
    let mut state = PlayerState::new(Vec3::new(0.5, 30.0, 0.5));
    for _ in 0..8 {
        Simulator::default()
            .tick(&mut state, MovementInput::default(), &Contact(None))
            .unwrap();
    }
    assert!(state.fall_distance() > 1.0);
    state
}

#[test]
fn fall_lifecycle_without_a_move_preserves_airborne_distance() {
    for immobile in [false, true] {
        let mut state = falling();
        let before = state.fall_distance();
        let result = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    immobile,
                    skip_travel: !immobile,
                    ..Default::default()
                },
                &Contact(None),
            )
            .unwrap();
        assert_eq!(result.movement, Vec3::ZERO);
        assert_eq!(state.fall_distance(), before, "immobile={immobile}");
    }
}

#[test]
fn fall_lifecycle_entering_bubbles_clears_the_current_drop() {
    for response in [SurfaceResponse::BubbleUp, SurfaceResponse::BubbleDown] {
        let mut state = falling();
        state.set_position(Vec3::new(0.5, 1.2, 0.5));
        state.velocity = Vec3::new(0.0, -0.4, 0.0);
        Simulator::default()
            .tick(
                &mut state,
                MovementInput::default(),
                &Contact(Some(response)),
            )
            .unwrap();
        assert_eq!(state.fall_distance(), 0.0, "{response:?}");
    }
}

#[test]
fn fall_lifecycle_honey_clears_distance_only_while_sliding_at_the_edge() {
    for (x, slides) in [(-0.24, true), (0.5, false)] {
        let mut state = falling();
        state.set_position(Vec3::new(x, 0.8, 0.5));
        state.velocity = Vec3::new(0.0, -0.2, 0.0);
        Simulator::default()
            .tick(
                &mut state,
                MovementInput::default(),
                &Contact(Some(SurfaceResponse::Honey)),
            )
            .unwrap();
        assert_eq!(state.fall_distance() == 0.0, slides, "x={x}");
    }
}

#[test]
fn fall_lifecycle_riding_keeps_the_descent_action_eligible() {
    for request in [false, true] {
        let input = MovementInput {
            mode: sim::MovementMode::Riding,
            descent_request: Some(request),
            ..Default::default()
        };
        assert_eq!(sim::descends_through_block(&input, true), request);
        assert!(!sim::descends_through_block(&input, false));
    }
}

struct Water;

impl CollisionWorld for Water {
    /// Keeps liquid sensing independent of solid collision resolution.
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    /// Places a full water cell below the entry position.
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let wet = block == [0, 0, 0];
        Ok(BlockPhysicsSample {
            identity: CollisionQuery::synthetic(()).identity,
            layers: Box::new([BlockPhysicsFacts {
                flags: if wet {
                    BlockPhysicsFlags::WATER
                } else {
                    BlockPhysicsFlags::PASSABLE
                },
                fluid_height_blocks: if wet { 1.0 } else { 0.0 },
                friction: sim::DEFAULT_SURFACE_FRICTION,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                surface_response: SurfaceResponse::None,
            }]),
        })
    }
}

#[test]
fn fall_lifecycle_water_entry_clears_retained_distance_before_adding_this_drop() {
    let mut state = falling();
    state.set_position(Vec3::new(0.5, 0.8, 0.5));
    state.velocity = Vec3::new(0.0, -0.4, 0.0);
    let result = Simulator::default()
        .tick(&mut state, MovementInput::default(), &Water)
        .unwrap();
    assert_eq!(state.fall_distance(), -result.movement.y as f32);
}

#[test]
fn fall_lifecycle_immobile_descent_clears_distance_before_the_new_contact_scan() {
    let mut state = falling();
    state.update_climber_contact(Some(false), Some(false), Some(false), Some(true));
    let result = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                immobile: true,
                descent_request: Some(true),
                ..Default::default()
            },
            &Contact(None),
        )
        .unwrap();
    assert!(result.environment.descending_through_block);
    assert!(!result.environment.over_descending_block);
    assert_eq!(state.fall_distance(), 0.0);
}

struct WaterAndHoney;

impl CollisionWorld for WaterAndHoney {
    /// Keeps the fall airborne between adjacent water and honey cells.
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    /// Places honey beside the water cell so the moved body enters both at once.
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut sample = Water.block_physics(block)?;
        if block == [1, 0, 0] {
            sample.layers[0].surface_response = SurfaceResponse::Honey;
        }
        Ok(sample)
    }
}

#[test]
fn fall_lifecycle_new_water_contact_excludes_the_honey_fall_reset() {
    let mut state = falling();
    state.set_position(Vec3::new(0.76, 0.8, 0.5));
    state.velocity = Vec3::new(0.0, -0.4, 0.0);
    let result = Simulator::default()
        .tick(&mut state, MovementInput::default(), &WaterAndHoney)
        .unwrap();
    assert_eq!(state.fall_distance(), -result.movement.y as f32);
}

struct Waterlogged;

impl CollisionWorld for Waterlogged {
    /// Keeps secondary-layer water contact independent of solid collision.
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    /// Keeps water in the secondary layer of the entry cell.
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut sample = Water.block_physics(block)?;
        if block == [0, 0, 0] {
            let dry = Water.block_physics([2, 0, 0])?;
            sample.layers = Box::new([*dry.primary(), *sample.primary()]);
        }
        Ok(sample)
    }
}

#[test]
fn fall_lifecycle_water_entry_reads_secondary_material_layers() {
    let mut state = falling();
    state.set_position(Vec3::new(0.5, 0.8, 0.5));
    state.velocity = Vec3::new(0.0, -0.4, 0.0);
    let result = Simulator::default()
        .tick(&mut state, MovementInput::default(), &Waterlogged)
        .unwrap();
    assert_eq!(state.fall_distance(), -result.movement.y as f32);
}

struct MixedLiquids;

impl CollisionWorld for MixedLiquids {
    /// Keeps both liquid probes independent of solid collision.
    fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    /// Supplies water and lava material layers in the same contact cell.
    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut sample = Water.block_physics(block)?;
        if block == [0, 0, 0] {
            let water = *sample.primary();
            let lava = BlockPhysicsFacts {
                flags: BlockPhysicsFlags::LAVA,
                ..water
            };
            sample.layers = Box::new([water, lava]);
        }
        Ok(sample)
    }
}

#[test]
fn fall_lifecycle_lava_takes_precedence_over_water_when_both_probes_touch() {
    let mut state = falling();
    let before = state.fall_distance();
    state.set_position(Vec3::new(0.5, -0.2, 0.5));
    state.velocity = Vec3::new(0.0, -0.4, 0.0);
    let result = Simulator::default()
        .tick(&mut state, MovementInput::default(), &MixedLiquids)
        .unwrap();
    assert_eq!(
        state.fall_distance(),
        before * 0.5 - result.movement.y as f32
    );
}
