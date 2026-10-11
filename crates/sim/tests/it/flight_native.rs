//! Frozen float regressions for vanilla flight controls and constants. These are
//! derived cases, not captured live trajectories.

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, MovementMode, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

struct FlightWorld {
    water: bool,
}

impl CollisionWorld for FlightWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: if self.water { 1.0 } else { 0.0 },
                flags: if self.water {
                    BlockPhysicsFlags::WATER
                } else {
                    BlockPhysicsFlags::default()
                },
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn native_float(bits: u32) -> f64 {
    f64::from(f32::from_bits(bits))
}

fn flying() -> MovementInput {
    MovementInput {
        mode: MovementMode::Flying,
        creative_flight: true,
        ..MovementInput::default()
    }
}

#[test]
fn spectator_flight_traverses_solid_terrain_and_does_not_retain_ground_contact() {
    let world = FlightSurface {
        height: 20.0,
        surface_block_y: 0,
        friction: 0.6,
    };
    let mut state = PlayerState::new(Vec3::new(0.0, 10.0, 0.0));
    state.on_ground = true;
    let start = state.position;
    let input = MovementInput {
        spectator: true,
        forward: 1.0,
        jumping: true,
        ..flying()
    };
    for _ in 0..20 {
        let result = Simulator::default()
            .tick(&mut state, input, &world)
            .unwrap();
        assert!(!result.on_ground);
        assert_eq!(result.collisions, sim::AxisCollisions::default());
    }
    assert!(state.position.y > start.y);
    assert!(state.position.z > start.z + 1.0);
}

/// Spectator movement must not depend on terrain arriving at its current position.
struct UnloadedSpectatorWorld;

impl CollisionWorld for UnloadedSpectatorWorld {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Err(WorldQueryError::UnloadedChunk(world::ChunkKey {
            dimension: 0,
            x: 0,
            z: 0,
        }))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Err(WorldQueryError::UnloadedChunk(world::ChunkKey {
            dimension: 0,
            x: 0,
            z: 0,
        }))
    }
}

#[test]
fn spectator_hovers_and_descends_without_reading_unloaded_terrain() {
    let mut state = PlayerState::new(Vec3::new(0.5, 10.0, 0.5));
    let start = state.position;
    let input = MovementInput {
        spectator: true,
        ..flying()
    };
    for _ in 0..20 {
        let result = Simulator::default()
            .tick(&mut state, input, &UnloadedSpectatorWorld)
            .unwrap();
        assert_eq!(result.position, start);
        assert!(result.world_identity.chunks.is_empty());
    }
    let result = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                sneaking: true,
                forward: 1.0,
                ..input
            },
            &UnloadedSpectatorWorld,
        )
        .unwrap();
    assert!(result.position.y < start.y);
    assert!(result.position.z > start.z);
    assert!(!result.environment.in_water && !result.environment.in_lava);
}

struct FlightSurface {
    height: f64,
    surface_block_y: i32,
    friction: f64,
}

impl CollisionWorld for FlightSurface {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        let floor = Aabb::new(
            Vec3::new(-64.0, -1.0, -64.0),
            Vec3::new(64.0, self.height, 64.0),
        );
        Ok(CollisionQuery::synthetic(if floor.intersects(query) {
            vec![floor]
        } else {
            Vec::new()
        }))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: if block[1] == self.surface_block_y {
                    self.friction
                } else {
                    0.6
                },
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 0.0,
                flags: BlockPhysicsFlags::PASSABLE,
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

#[test]
fn flight_ground_friction_uses_start_contact_and_native_support_probe() {
    // Flying horizontal movement samples friction at the starting feet minus
    // 0.1 before collision.
    let moving = MovementInput {
        forward: 1.0,
        fly_speed: Some(0.0),
        ..flying()
    };
    let cases = [
        (
            "creative ground hover",
            1.0,
            0,
            0.6,
            true,
            flying(),
            0x3e51_a9fd,
            true,
        ),
        (
            "other ground hover",
            1.0,
            0,
            0.6,
            true,
            MovementInput {
                creative_flight: false,
                ..flying()
            },
            0x3ed1_a9fd,
            true,
        ),
        (
            "moving on ground",
            1.0,
            0,
            0.6,
            true,
            moving,
            0x3f0b_c6a9,
            true,
        ),
        (
            "takeoff keeps starting friction",
            1.0,
            0,
            0.6,
            true,
            MovementInput {
                jumping: true,
                ..moving
            },
            0x3f0b_c6a9,
            false,
        ),
        (
            "landing keeps starting air friction",
            1.0,
            0,
            0.6,
            false,
            MovementInput {
                sneaking: true,
                ..moving
            },
            0x3f68_f5c3,
            true,
        ),
        (
            "fractional support",
            1.25,
            1,
            0.98,
            true,
            moving,
            0x3f64_4d02,
            true,
        ),
    ];
    for (name, height, surface_block_y, friction, grounded, input, expected, final_grounded) in
        cases
    {
        let mut state = PlayerState::new(Vec3::new(
            0.5,
            height + if grounded { 0.0 } else { 0.1 },
            0.5,
        ));
        state.on_ground = grounded;
        state.velocity.z = 1.0;
        let output = Simulator::default()
            .tick(
                &mut state,
                input,
                &FlightSurface {
                    height,
                    surface_block_y,
                    friction,
                },
            )
            .unwrap();
        assert_eq!(output.movement.z, 1.0, "{name}");
        assert_eq!(output.velocity.z, native_float(expected), "{name}");
        assert_eq!(output.on_ground, final_grounded, "{name}");
    }
}

#[test]
fn vertical_controls_and_hover_precede_movement_with_independent_drag() {
    // Vertical fly input precedes movement; fly drag retains y independently.
    // Each pair is (movement, velocity).
    let cases = [
        (
            "creative idle",
            flying(),
            1.0,
            [[0x3ec0_0000, 0x3e66_6667], [0x3dac_cccd, 0x3d4f_5c2a]],
        ),
        (
            "other idle",
            MovementInput {
                creative_flight: false,
                ..flying()
            },
            1.0,
            [[0x3f80_0000, 0x3f19_999a], [0x3f19_999a, 0x3eb8_51ec]],
        ),
        (
            "held jump",
            MovementInput {
                jumping: true,
                ..flying()
            },
            0.0,
            [[0x3e19_999a, 0x3db8_51ec], [0x3e75_c290, 0x3e13_74bd]],
        ),
        (
            "held sneak",
            MovementInput {
                sneaking: true,
                ..flying()
            },
            0.0,
            [[0xbe61_47ae, 0xbe07_2b02], [0xbeb4_3958, 0xbe58_44d1]],
        ),
        (
            "simultaneous vertical controls",
            MovementInput {
                jumping: true,
                sneaking: true,
                ..flying()
            },
            1.0,
            [[0, 0], [0, 0]],
        ),
        (
            "custom vertical ability",
            MovementInput {
                jumping: true,
                vertical_fly_speed: Some(2.0),
                ..flying()
            },
            0.0,
            [[0x3e99_999a, 0x3e38_51ec], [0x3ef5_c290, 0x3e93_74bd]],
        ),
        (
            "zero vertical ability",
            MovementInput {
                jumping: true,
                vertical_fly_speed: Some(0.0),
                ..flying()
            },
            1.0,
            [[0x3f80_0000, 0x3f19_999a], [0x3f19_999a, 0x3eb8_51ec]],
        ),
    ];
    for (name, input, velocity_y, expected) in cases {
        let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
        state.velocity.y = velocity_y;
        for [movement, velocity] in expected {
            let output = Simulator::default()
                .tick(&mut state, input, &FlightWorld { water: false })
                .unwrap();
            assert_eq!(output.movement.y, native_float(movement), "{name}");
            assert_eq!(output.velocity.y, native_float(velocity), "{name}");
        }
    }
}

#[test]
fn horizontal_hover_friction_does_not_modify_vertical_flight_drag() {
    // Hover classification reads processed controls before the travel impulse.
    // At the native threshold the ordinary horizontal retention applies.
    let cases = [
        (true, 0.0, [0x3e0b_c6a8, 0x3e66_6667, 0x3dd1_a9fd]),
        (false, 0.0, [0x3e8b_c6a8, 0x3f19_999a, 0x3e51_a9fd]),
        (true, 0.01, [0x3eba_5e36, 0x3f19_999a, 0x3e8b_c6a9]),
    ];
    for (creative_flight, forward, expected) in cases {
        let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
        state.velocity = Vec3::new(0.4, 1.0, 0.3);
        let output = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    creative_flight,
                    forward,
                    ..flying()
                },
                &FlightWorld { water: false },
            )
            .unwrap();
        assert_eq!(
            output.velocity,
            Vec3::new(
                native_float(expected[0]),
                native_float(expected[1]),
                native_float(expected[2]),
            ),
            "creative={creative_flight}, forward={forward}"
        );
    }
}

#[test]
fn horizontal_ability_speed_and_sprint_scale_without_flight_sneak_slowdown() {
    // Horizontal fly speed reads float ability 6 and the sprint multiplier.
    // Flying descent does not slow the processed horizontal axis.
    let cases = [
        (None, false, false, [0x3d48_b43a, 0x3d36_a402]),
        (None, false, true, [0x3d48_b43a, 0x3d36_a402]),
        (None, true, false, [0x3dc8_b43a, 0x3db6_a402]),
        (Some(0.1), false, false, [0x3dc8_b43a, 0x3db6_a402]),
        (Some(0.0), true, false, [0, 0]),
    ];
    for (fly_speed, sprinting, sneaking, expected) in cases {
        let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
        let output = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    fly_speed,
                    sprinting,
                    sneaking,
                    forward: 1.0,
                    ..flying()
                },
                &FlightWorld { water: false },
            )
            .unwrap();
        assert_eq!(output.movement.z, native_float(expected[0]));
        assert_eq!(output.velocity.z, native_float(expected[1]));
    }
}

#[test]
fn ability_flight_in_water_keeps_flight_controls_and_drag() {
    // Flight dispatch bypasses liquid travel, held liquid jump and water sink.
    let mut dry = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
    let mut wet = dry.clone();
    for (jumping, sneaking) in [(true, false), (true, false), (false, true), (false, false)] {
        let input = MovementInput {
            forward: 1.0,
            jumping,
            sneaking,
            ..flying()
        };
        let dry_tick = Simulator::default()
            .tick(&mut dry, input, &FlightWorld { water: false })
            .unwrap();
        let wet_tick = Simulator::default()
            .tick(&mut wet, input, &FlightWorld { water: true })
            .unwrap();
        assert!(!dry_tick.environment.in_water);
        assert!(wet_tick.environment.in_water);
        assert_eq!(wet_tick.movement, dry_tick.movement);
        assert_eq!(wet_tick.velocity, dry_tick.velocity);
        assert_eq!(wet_tick.position, dry_tick.position);
    }
}
