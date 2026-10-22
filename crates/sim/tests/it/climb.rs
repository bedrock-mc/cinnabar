use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, PlayerState, ProvenancedCollider, Simulator, SurfaceResponse, Vec3,
    WorldQueryError,
};

struct ClimbWorld {
    flags: BlockPhysicsFlags,
}

/// A held climb jump updates motion without moving during the skipped travel tick.
#[test]
fn skipped_travel_preserves_climb_jump_motion() {
    let world = ClimbWorld {
        flags: BlockPhysicsFlags::CLIMBABLE,
    };
    let mut player = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let target = player.position;
    let output = Simulator::default()
        .tick(
            &mut player,
            MovementInput {
                skip_travel: true,
                jumping: true,
                ..Default::default()
            },
            &world,
        )
        .unwrap();
    assert_eq!(output.position, target);
    assert_eq!(output.movement, Vec3::ZERO);
    assert_eq!(output.velocity.y, f64::from(0.2_f32));
    assert_eq!(player.jump_delay, 0);
}

impl CollisionWorld for ClimbWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: self.flags,
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

#[test]
fn ladder_ascend_descend_and_sneak_hold_use_climb_velocity_clamps() {
    let world = ClimbWorld {
        flags: BlockPhysicsFlags::CLIMBABLE,
    };
    let mut ascending = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let up = Simulator::default()
        .tick(
            &mut ascending,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(up.environment.on_climbable);
    assert!(up.movement.y > 0.0);
    assert!(up.movement.y <= f64::from(0.2_f32));

    let mut descending = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    descending.velocity.y = -1.0;
    let down = Simulator::default()
        .tick(&mut descending, MovementInput::default(), &world)
        .unwrap();
    assert!((down.movement.y + 0.2).abs() <= f64::from(f32::EPSILON));

    let mut holding = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    holding.velocity.y = -1.0;
    let held = Simulator::default()
        .tick(
            &mut holding,
            MovementInput {
                sneaking: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert_eq!(held.movement.y, 0.0);
}

/// A held jump inside supported scaffolding ascends at the scaffold speed, not the ladder one.
#[test]
fn scaffolding_ascends_at_its_own_speed() {
    let world = ClimbWorld {
        flags: BlockPhysicsFlags::SCAFFOLDING,
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(tick.environment.in_scaffolding);
    assert_eq!(tick.movement.y as f32, 0.15_f32);
}

/// Scaffold cells with unit collision, solid cells, and air everywhere else.
struct ScaffoldWorld {
    scaffolds: Vec<[i32; 3]>,
    solids: Vec<[i32; 3]>,
}

impl ScaffoldWorld {
    fn cells(&self) -> impl Iterator<Item = [i32; 3]> + '_ {
        self.scaffolds.iter().chain(&self.solids).copied()
    }
}

fn unit(block: [i32; 3]) -> Aabb {
    let min = Vec3::new(
        f64::from(block[0]),
        f64::from(block[1]),
        f64::from(block[2]),
    );
    Aabb::new(min, Vec3::new(min.x + 1.0, min.y + 1.0, min.z + 1.0))
}

impl CollisionWorld for ScaffoldWorld {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.cells()
                .map(unit)
                .filter(|shape| shape.intersects(query))
                .collect(),
        ))
    }

    fn collision_boxes_with_provenance(
        &self,
        query: Aabb,
    ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.cells()
                .filter(|block| unit(*block).intersects(query))
                .map(|block| ProvenancedCollider {
                    aabb: unit(block),
                    block: Some(block),
                    runtime_id: Some(1),
                    cell_has_scaffolding: None,
                })
                .collect(),
        ))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let flags = if self.scaffolds.contains(&block) {
            BlockPhysicsFlags::SCAFFOLDING
        } else if self.solids.contains(&block) {
            BlockPhysicsFlags::default()
        } else {
            BlockPhysicsFlags::PASSABLE
        };
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags,
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn sneaking() -> MovementInput {
    MovementInput {
        sneaking: true,
        ..MovementInput::default()
    }
}

/// Sneaking over supported scaffolding descends at a fixed speed with drag but no gravity.
#[test]
fn sneaking_descends_a_supported_scaffold_column_at_a_fixed_speed() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 2.0, 0.5));
    state.on_ground = true;
    for _ in 0..2 {
        let tick = Simulator::default()
            .tick(&mut state, sneaking(), &world)
            .unwrap();
        assert_eq!(tick.movement.y as f32, -0.15_f32);
        assert_eq!(tick.velocity.y as f32, -0.15_f32 * 0.98_f32);
    }
}

/// An immobile tick still runs the block climber and the descent request in place.
#[test]
fn an_immobile_tick_still_reports_block_climber_contact() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    for sneak in [false, true] {
        let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    immobile: true,
                    sneaking: sneak,
                    ..MovementInput::default()
                },
                &world,
            )
            .unwrap();
        let environment = tick.environment;
        assert!(!environment.climber_unsampled);
        assert!(environment.in_scaffolding && environment.in_ascending_block);
        assert!(environment.over_scaffolding && environment.over_descending_block);
        assert_eq!(environment.descending_through_block, sneak);
        assert_eq!(tick.position, Vec3::new(0.5, 1.0, 0.5));
        assert_eq!(tick.velocity, Vec3::ZERO);
    }
}

/// Scaffolding everywhere, behind a movement origin that is not available yet.
struct UnavailableOrigin;

impl CollisionWorld for UnavailableOrigin {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    /// Rejects the origin as an absent terrain column would.
    fn ensure_movement_origin(&self, _feet: Vec3) -> Result<(), WorldQueryError> {
        Err(WorldQueryError::CoordinateOutOfRange)
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        ClimbWorld {
            flags: BlockPhysicsFlags::SCAFFOLDING,
        }
        .block_physics(block)
    }
}

/// Unreadable terrain after contact was measured keeps publishing that
/// contact, with this tick's descent request.
#[test]
fn unreadable_terrain_publishes_the_retained_contact() {
    let column = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                immobile: true,
                ..MovementInput::default()
            },
            &column,
        )
        .unwrap();
    for sneak in [true, false] {
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    immobile: true,
                    sneaking: sneak,
                    ..MovementInput::default()
                },
                &UnavailableOrigin,
            )
            .unwrap();
        let environment = tick.environment;
        assert!(!environment.climber_unsampled);
        assert!(environment.in_scaffolding && environment.over_descending_block);
        assert_eq!(environment.descending_through_block, sneak);
    }
}

/// An immobile spectator, or an immobile tick without an available movement
/// origin, measures no climber contact.
#[test]
fn an_immobile_tick_measures_no_contact_for_a_spectator_or_an_unavailable_origin() {
    let column = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    let frozen = MovementInput {
        immobile: true,
        sneaking: true,
        ..MovementInput::default()
    };
    assert_unmeasured(
        MovementInput {
            spectator: true,
            ..frozen
        },
        &column,
    );
    assert_unmeasured(frozen, &UnavailableOrigin);
}

/// Runs one tick in place and checks that it measured no climber contact.
fn assert_unmeasured(input: MovementInput, world: &impl CollisionWorld) {
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let tick = Simulator::default().tick(&mut state, input, world).unwrap();
    let environment = tick.environment;
    assert!(environment.climber_unsampled, "{input:?}");
    assert!(!environment.in_scaffolding && !environment.descending_through_block);
    assert!(tick.world_identity.chunks.is_empty());
}

/// A placement, as a correction applies, keeps the retained contact: the next
/// descent action reads it wherever the feet now are.
#[test]
fn a_placement_keeps_the_retained_contact() {
    let supported = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0]],
        solids: vec![[0, -1, 0]],
    };
    let unsupported = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [5, 0, 0]],
        solids: Vec::new(),
    };
    for (feet, descends) in [
        (Vec3::new(0.5, 1.0, 0.5), true),
        (Vec3::new(5.5, 1.0, 0.5), true),
    ] {
        let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
        state.on_ground = true;
        Simulator::default()
            .tick(&mut state, MovementInput::default(), &supported)
            .unwrap();
        state.set_position(feet);
        let tick = Simulator::default()
            .tick(&mut state, sneaking(), &unsupported)
            .unwrap();
        assert_eq!(
            tick.environment.descending_through_block, descends,
            "{feet:?}"
        );
    }
}

/// The climber scans the resolved collision faces: a box stopped by a wall at
/// x = 1 does not reach the column behind it.
#[test]
fn the_climber_scans_the_resolved_collision_box() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0]],
        solids: vec![[0, 1, 0], [0, 2, 0], [1, 0, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(1.45, 1.0, 0.5));
    state.on_ground = true;
    state.velocity.x = -0.3;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    assert!(tick.collisions.x);
    assert!(!tick.environment.over_scaffolding);
}

/// Without retained contact, an unreadable block under the feet-layer scaffold
/// leaves the ascent available instead of failing the tick.
#[test]
fn an_unreadable_support_does_not_block_the_scaffold_ascent() {
    struct UnknownSupport(ScaffoldWorld);
    impl CollisionWorld for UnknownSupport {
        fn collision_boxes(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            self.0.collision_boxes(query)
        }

        fn collision_boxes_with_provenance(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
            self.0.collision_boxes_with_provenance(query)
        }

        fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
            if block == [0, 0, 0] {
                return Err(WorldQueryError::UnknownRuntimeId {
                    runtime_id: 7,
                    block,
                });
            }
            self.0.block_physics(block)
        }
    }
    let world = UnknownSupport(ScaffoldWorld {
        scaffolds: vec![[0, 1, 0]],
        solids: Vec::new(),
    });
    let mut state = PlayerState::new(Vec3::new(0.5, 1.75, 0.5));
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert_eq!(tick.movement.y as f32, 0.15_f32);
}

/// An identity conflict under the feet-layer scaffold still fails the tick
/// rather than falling back to the cached scaffold.
#[test]
fn an_identity_conflict_under_the_scaffold_still_fails_the_tick() {
    struct ConflictingSupport(ScaffoldWorld);
    impl CollisionWorld for ConflictingSupport {
        fn collision_boxes(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            self.0.collision_boxes(query)
        }

        fn collision_boxes_with_provenance(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<ProvenancedCollider>>, WorldQueryError> {
            self.0.collision_boxes_with_provenance(query)
        }

        /// The support cell reports a different collision registry.
        fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
            let mut sample = self.0.block_physics(block)?;
            if block == [0, 0, 0] {
                sample.identity.registry.preg_sha256 = [1; 32];
            }
            Ok(sample)
        }
    }
    let world = ConflictingSupport(ScaffoldWorld {
        scaffolds: vec![[0, 1, 0]],
        solids: vec![[0, 0, 0]],
    });
    let mut state = PlayerState::new(Vec3::new(0.5, 1.75, 0.5));
    let result = Simulator::default().tick(
        &mut state,
        MovementInput {
            jumping: true,
            ..MovementInput::default()
        },
        &world,
    );
    assert_eq!(
        result,
        Err(sim::SimulationError::World(
            WorldQueryError::RegistryIdentityMismatch
        ))
    );
}

/// A tick with suppressed travel still starts the descent, without moving.
#[test]
fn skipped_travel_still_starts_the_scaffold_descent() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 2.0, 0.5));
    state.on_ground = true;
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                skip_travel: true,
                ..sneaking()
            },
            &world,
        )
        .unwrap();
    assert!(tick.environment.descending_through_block);
    assert_eq!(tick.position.y, 2.0);
    assert_eq!(tick.velocity.y as f32, -0.15_f32);
}

/// A scaffold bridge over air is not a descending block, so sneaking keeps its support.
#[test]
fn sneaking_on_a_scaffold_bridge_over_air_stays_on_top() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0]],
        solids: Vec::new(),
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.on_ground = true;
    let tick = Simulator::default()
        .tick(&mut state, sneaking(), &world)
        .unwrap();
    assert_eq!(tick.position.y, 1.0);
    assert!(tick.on_ground);
}

/// Jumping inside scaffolding climbs instead of jumping and starts the jump delay.
#[test]
fn jumping_inside_scaffolding_climbs_without_a_ground_jump() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.on_ground = true;
    let output = Simulator::default()
        .tick_with_controls(
            &mut state,
            MovementInput {
                jumping: true,
                jump_pressed: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(!output.jump_initiated);
    assert_eq!(output.tick_result.movement.y as f32, 0.15_f32);
    assert_eq!(state.jump_delay, sim::JUMP_DELAY_TICKS - 1);
}

/// The environment query reports the same feet-layer scaffolding contact as the tick.
#[test]
fn the_environment_query_reports_scaffolding_contact() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 1, 0]],
        solids: vec![[0, 0, 0]],
    };
    let environment = Simulator::default()
        .movement_environment(
            Vec3::new(0.5, 1.0, 0.5),
            sim::MovementMode::Walking,
            false,
            &world,
        )
        .unwrap();
    assert!(environment.value.in_scaffolding);
}

/// Scaffolding over air is no ascending block, yet a held jump inside it still climbs.
#[test]
fn climbing_inside_a_scaffold_bridge_over_air_still_ascends() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 1, 0]],
        solids: Vec::new(),
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.75, 0.5));
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(tick.environment.in_scaffolding);
    assert!(!tick.environment.in_ascending_block);
    assert_eq!(tick.movement.y as f32, 0.15_f32);
}

/// The block climber scans the layer below the feet every tick, not only while sneaking.
#[test]
fn standing_on_supported_scaffolding_reports_the_layer_below_without_sneaking() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.on_ground = true;
    let tick = Simulator::default()
        .tick(&mut state, MovementInput::default(), &world)
        .unwrap();
    let environment = tick.environment;
    assert!(environment.over_scaffolding);
    assert!(environment.over_descending_block);
    assert!(!environment.in_scaffolding);
    assert!(!environment.in_ascending_block);
    assert!(!environment.descending_through_block);
    assert_eq!(tick.position.y, 1.0);
}

/// Sneaking over supported scaffolding reports the descent it starts and the
/// contact the block climber measures after the move.
#[test]
fn sneaking_over_supported_scaffolding_reports_the_descent() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0], [0, 1, 0]],
        solids: vec![[0, -1, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    let tick = Simulator::default()
        .tick(&mut state, sneaking(), &world)
        .unwrap();
    let environment = tick.environment;
    assert_eq!(tick.position.y as f32, 0.85_f32);
    assert!(environment.descending_through_block);
    // The feet now sit in the lowest scaffold, over its solid support.
    assert!(environment.in_scaffolding && environment.in_ascending_block);
    assert!(!environment.over_scaffolding && !environment.over_descending_block);
}

/// The block climber measures contact after the move, so climbing out of a
/// scaffold reports the scaffold below the new feet.
#[test]
fn climbing_out_of_scaffolding_reports_the_contact_after_the_move() {
    let world = ScaffoldWorld {
        scaffolds: vec![[0, 1, 0]],
        solids: vec![[0, 0, 0]],
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.95, 0.5));
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                jumping: true,
                ..MovementInput::default()
            },
            &world,
        )
        .unwrap();
    assert!(tick.position.y > 2.0, "{}", tick.position.y);
    let environment = tick.environment;
    assert!(!environment.in_scaffolding && !environment.in_ascending_block);
    assert!(environment.over_scaffolding && environment.over_descending_block);
}

/// The descent action reads the contact retained from the previous tick, so a
/// support removed in between still starts the descent once while the block
/// climber already reports the unsupported scaffold.
#[test]
fn the_descent_reads_the_previous_tick_contact() {
    let supported = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0]],
        solids: vec![[0, -1, 0]],
    };
    let unsupported = ScaffoldWorld {
        scaffolds: vec![[0, 0, 0]],
        solids: Vec::new(),
    };
    let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
    state.on_ground = true;
    let standing = Simulator::default()
        .tick(&mut state, MovementInput::default(), &supported)
        .unwrap();
    assert!(standing.environment.over_descending_block);
    assert_eq!(state.position.y, 1.0);
    let frozen = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                immobile: true,
                ..sneaking()
            },
            &unsupported,
        )
        .unwrap();
    assert!(frozen.environment.descending_through_block);
    assert!(frozen.environment.over_scaffolding);
    assert!(!frozen.environment.over_descending_block);
}

fn powder_snow() -> ClimbWorld {
    ClimbWorld {
        flags: BlockPhysicsFlags::from_bits(
            BlockPhysicsFlags::POWDER_SNOW.bits() | BlockPhysicsFlags::PASSABLE.bits(),
        )
        .unwrap(),
    }
}

/// A snow-standing wearer climbs out of powder snow at the climbing-block speed.
#[test]
fn a_held_jump_climbs_powder_snow_only_for_a_snow_standing_wearer() {
    for can_stand_on_snow in [false, true] {
        let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    jumping: true,
                    can_stand_on_snow,
                    ..MovementInput::default()
                },
                &powder_snow(),
            )
            .unwrap();
        let environment = tick.environment;
        assert_eq!(environment.in_ascending_block, can_stand_on_snow);
        assert_eq!(environment.over_descending_block, can_stand_on_snow);
        assert_eq!(environment.on_climbable, can_stand_on_snow);
        assert!(!environment.in_scaffolding && !environment.over_scaffolding);
        assert_eq!(tick.movement.y > 0.0, can_stand_on_snow);
    }
}

/// Powder snow is climbable for a snow-standing wearer: descent is clamped as on a ladder.
#[test]
fn powder_snow_clamps_a_snow_standing_fall_like_a_ladder() {
    for can_stand_on_snow in [false, true] {
        let mut state = PlayerState::new(Vec3::new(0.5, 1.0, 0.5));
        state.velocity.y = -1.0;
        let tick = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    can_stand_on_snow,
                    ..MovementInput::default()
                },
                &powder_snow(),
            )
            .unwrap();
        // The fixture's unit speed factors leave the clamped request unscaled.
        let expected = if can_stand_on_snow { -0.2_f32 } else { -1.0 };
        assert!(
            (tick.movement.y as f32 - expected).abs() <= 1.0e-6,
            "{can_stand_on_snow}: {}",
            tick.movement.y
        );
    }
}

/// Sneaking over powder snow descends through it for a snow-standing wearer.
#[test]
fn sneaking_over_powder_snow_descends_for_a_snow_standing_wearer() {
    struct SnowBelow;
    impl CollisionWorld for SnowBelow {
        fn collision_boxes(
            &self,
            _query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Ok(CollisionQuery::synthetic(Vec::new()))
        }

        fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
            let flags = if block[1] < 2 {
                BlockPhysicsFlags::POWDER_SNOW.bits() | BlockPhysicsFlags::PASSABLE.bits()
            } else {
                BlockPhysicsFlags::PASSABLE.bits()
            };
            ClimbWorld {
                flags: BlockPhysicsFlags::from_bits(flags).unwrap(),
            }
            .block_physics(block)
        }
    }
    let mut state = PlayerState::new(Vec3::new(0.5, 2.0, 0.5));
    let tick = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                sneaking: true,
                can_stand_on_snow: true,
                ..MovementInput::default()
            },
            &SnowBelow,
        )
        .unwrap();
    assert!(tick.environment.descending_through_block);
    assert_eq!(tick.movement.y as f32, -0.15_f32);
    // After the move the feet are inside the snow, still over more of it.
    assert!(tick.environment.in_ascending_block);
    assert!(tick.environment.over_descending_block);
}
