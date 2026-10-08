//! Focused regressions for 1.26.50.26 vanilla liquid movement and its constants.
//! These use a synthetic fully wet world;
//! they do not claim captured vanilla trajectories or live server acceptance.

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementInput, MovementMode, PlayerState, Simulator, SurfaceResponse, Vec3, WorldQueryError,
};

struct Submerged;

struct Surface {
    secondary_water: bool,
}

impl CollisionWorld for Surface {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut sample = Submerged.block_physics(block)?;
        if block[1] >= 5 {
            let air = BlockPhysicsFacts {
                flags: BlockPhysicsFlags::PASSABLE,
                fluid_height_blocks: 0.0,
                ..sample.layers[0]
            };
            sample.layers = if self.secondary_water {
                Box::new([air, sample.layers[0]])
            } else {
                Box::new([air])
            };
        } else {
            // The native guard tests material cells even above their visual
            // liquid surface. A shallow depth must still permit steering.
            sample.layers[0].fluid_height_blocks = 0.1;
        }
        Ok(sample)
    }
}

impl CollisionWorld for Submerged {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, _block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        Ok(BlockPhysicsSample {
            layers: Box::new([BlockPhysicsFacts {
                friction: 0.6,
                horizontal_speed_factor: 1.0,
                vertical_speed_factor: 1.0,
                fluid_height_blocks: 1.0,
                flags: BlockPhysicsFlags::WATER,
                surface_response: SurfaceResponse::None,
            }]),
            identity: CollisionQuery::synthetic(()).identity,
        })
    }
}

fn native_float(bits: u32) -> f64 {
    f64::from(f32::from_bits(bits))
}

#[test]
fn current_water_drag_keeps_vertical_retention_and_sprint_without_a_swim_pose() {
    // Sprint is entity flag 3; y uses the independent water retention.
    // Water gravity then applies outside the swim pose.
    // Frozen f32 outputs for velocity (0.4, 0.2, 0.3), airborne Depth Strider.
    let cases = [
        (false, 0, [0x3ea3_d70b, 0x3e1e_b853, 0x3e75_c290]),
        (true, 0, [0x3eb8_51eb, 0x3e1e_b853, 0x3e8a_3d71]),
        (false, 3, [0x3e89_d496, 0x3e1e_b853, 0x3e4e_bee1]),
        (true, 3, [0x3e94_1207, 0x3e1e_b853, 0x3e5e_1b0a]),
    ];
    for (sprinting, depth_strider, expected) in cases {
        let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
        state.velocity = Vec3::new(0.4, 0.2, 0.3);
        let output = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    sprinting,
                    depth_strider,
                    ..MovementInput::default()
                },
                &Submerged,
            )
            .unwrap();
        assert!(output.environment.in_water);
        assert_eq!(
            output.velocity,
            Vec3::new(
                native_float(expected[0]),
                native_float(expected[1]),
                native_float(expected[2]),
            ),
            "sprint={sprinting}, depth_strider={depth_strider}"
        );
    }
}

#[test]
fn held_swim_jump_ascends_independently_of_look_pitch() {
    // A held swim jump uses the default liquid impulse regardless of pitch;
    // swimming skips gravity.
    for pitch_degrees in [-90.0, 0.0, 90.0] {
        let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
        state.swim_amount = 1.0;
        state.swim_pose_active = true;
        let output = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    mode: MovementMode::Swimming,
                    jumping: true,
                    pitch_degrees,
                    ..MovementInput::default()
                },
                &Submerged,
            )
            .unwrap();
        assert_eq!(output.movement.y, native_float(0x3d23_d70a));
        assert_eq!(output.velocity.y, native_float(0x3d03_126f));
    }
}

#[test]
fn swimming_dive_steers_before_vertical_water_drag_without_gravity() {
    // Vanilla selects the faster steering rate below its dive
    // threshold. Pitch 90 degrees reaches the exact -1 table cardinal.
    let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
    let output = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                mode: MovementMode::Swimming,
                pitch_degrees: 90.0,
                ..MovementInput::default()
            },
            &Submerged,
        )
        .unwrap();
    assert_eq!(output.movement.y, native_float(0xbdae_147b));
    assert_eq!(output.velocity.y, native_float(0xbd8b_4396));
}

#[test]
fn ordinary_water_sneak_adds_downward_motion_before_drag_and_gravity() {
    // Sneaking in water adds the descent input,
    // independently of horizontal sneak slowdown and the swimming pose.
    let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
    let output = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                sneaking: true,
                ..MovementInput::default()
            },
            &Submerged,
        )
        .unwrap();
    assert_eq!(output.movement.y, native_float(0xbd23_d70a));
    assert_eq!(output.velocity.y, native_float(0xbd17_8d50));
}

#[test]
fn swimming_upward_steering_stops_at_the_tick_captured_attach_cell_boundary() {
    // The bounding-box update floors attach7 and copies its material's liquid byte.
    // The body remains wet at y=4.4, independently of that point's cell.
    for (height, initial, movement, retained) in [
        (
            0.599,
            0.25,
            native_float(0x3e97_0a3d),
            native_float(0x3e71_a9fb),
        ),
        (0.6, 0.25, 0.0, 0.0),
        (0.6, -0.25, 0.0, 0.0),
    ] {
        let mut state = PlayerState::new(Vec3::new(0.5, 4.4, 0.5));
        state.velocity.y = initial;
        let output = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    mode: MovementMode::Swimming,
                    pitch_degrees: -90.0,
                    liquid_attach_height: Some(height),
                    ..MovementInput::default()
                },
                &Surface {
                    secondary_water: false,
                },
            )
            .unwrap();
        assert!(output.environment.in_water);
        assert_eq!(output.movement.y, movement, "attach height={height}");
        assert_eq!(output.velocity.y, retained, "attach height={height}");
    }
}

#[test]
fn swimming_surface_guard_uses_primary_material_and_still_allows_diving() {
    for (pitch_degrees, movement, retained) in [
        (-90.0, 0.0, 0.0),
        (90.0, native_float(0xbdae_147b), native_float(0xbd8b_4396)),
    ] {
        let mut state = PlayerState::new(Vec3::new(0.5, 4.4, 0.5));
        let output = Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    mode: MovementMode::Swimming,
                    pitch_degrees,
                    liquid_attach_height: Some(0.6),
                    ..MovementInput::default()
                },
                &Surface {
                    secondary_water: true,
                },
            )
            .unwrap();
        assert_eq!(output.movement.y, movement);
        assert_eq!(output.velocity.y, retained);
    }
}

struct Lava;

impl CollisionWorld for Lava {
    fn collision_boxes(&self, _query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(Vec::new()))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut sample = Submerged.block_physics(block)?;
        sample.layers[0].flags = BlockPhysicsFlags::LAVA;
        Ok(sample)
    }
}

/// Forward travel from rest for one tick, returning the post-drag forward velocity.
fn forward_velocity(input: MovementInput, world: &impl CollisionWorld) -> f32 {
    let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
    state.swim_amount = 1.0;
    state.swim_pose_active = input.mode == MovementMode::Swimming;
    let output = Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                forward: 1.0,
                ..input
            },
            world,
        )
        .unwrap();
    output.velocity.z as f32
}

/// Water and lava travel take their base speed from the liquid movement attributes.
#[test]
fn liquid_travel_reads_the_liquid_movement_attributes() {
    let impulse = 1.0_f32 * 0.98;
    let water = forward_velocity(
        MovementInput {
            underwater_movement_speed: Some(0.05),
            ..MovementInput::default()
        },
        &Submerged,
    );
    assert_eq!(water, impulse * 0.05 * 0.8);
    let lava = forward_velocity(
        MovementInput {
            lava_movement_speed: Some(0.05),
            ..MovementInput::default()
        },
        &Lava,
    );
    assert_eq!(lava, impulse * 0.05 * 0.5);
}

/// A dolphin-boosted swimmer travels at `base * 2 * (level / 3 * 0.3 + 0.7)` and
/// keeps the plain water drag instead of Depth Strider's blend.
#[test]
fn dolphin_boost_scales_swim_speed_and_skips_depth_strider_drag() {
    let impulse = 1.0_f32 * 0.98;
    for (depth_strider, scale) in [(0_u8, 0.7_f32), (3, 1.0)] {
        let boosted = MovementInput {
            mode: MovementMode::Swimming,
            depth_strider,
            effects: sim::MovementEffects {
                dolphin_boost: true,
                ..sim::MovementEffects::default()
            },
            ..MovementInput::default()
        };
        let speed = 0.02_f32 * 2.0 * ((f32::from(depth_strider) / 3.0) * 0.3 + 0.7);
        assert!((speed - 0.04 * scale).abs() < 1.0e-6);
        assert_eq!(
            forward_velocity(boosted, &Submerged),
            impulse * speed * 0.8,
            "depth strider {depth_strider}"
        );
        // Outside the swimming pose the boost does not apply.
        let walking = MovementInput {
            mode: MovementMode::Walking,
            ..boosted
        };
        let plain = MovementInput {
            effects: sim::MovementEffects::default(),
            ..walking
        };
        assert_eq!(
            forward_velocity(walking, &Submerged),
            forward_velocity(plain, &Submerged)
        );
    }
}

/// Water, or dry ground when `water` is false, above a floor whose top is `floor`.
struct Floored {
    floor: Option<f64>,
    water: bool,
}

impl CollisionWorld for Floored {
    fn collision_boxes(&self, query: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
        Ok(CollisionQuery::synthetic(
            self.floor
                .map(|top| {
                    Aabb::new(
                        Vec3::new(-1.0e6, top - 1.0, -1.0e6),
                        Vec3::new(1.0e6, top, 1.0e6),
                    )
                })
                .filter(|floor| floor.intersects(query))
                .into_iter()
                .collect(),
        ))
    }

    fn block_physics(&self, block: [i32; 3]) -> Result<BlockPhysicsSample, WorldQueryError> {
        let mut sample = Submerged.block_physics(block)?;
        if !self.water {
            sample.layers[0].flags = BlockPhysicsFlags::default();
            sample.layers[0].fluid_height_blocks = 0.0;
        }
        Ok(sample)
    }
}

/// A large movement attribute keeps Depth Strider water travel inside every
/// simulator query budget, while dry ground applies it unchanged.
#[test]
fn depth_strider_water_travel_stays_simulable_with_a_large_movement_attribute() {
    for floor in [Some(5.0), None] {
        for depth_strider in 1..=3 {
            for sprinting in [true, false] {
                let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
                state.on_ground = floor.is_some();
                let input = MovementInput {
                    forward: 1.0,
                    strafe: 1.0,
                    yaw_degrees: 45.0,
                    sprinting,
                    depth_strider,
                    movement_speed: Some(20.0),
                    liquid_contact_height: Some(f64::from(sim::PLAYER_HEIGHT as f32)),
                    ..MovementInput::default()
                };
                let world = Floored { floor, water: true };
                for tick in 0..300 {
                    Simulator::default()
                        .tick(&mut state, input, &world)
                        .unwrap_or_else(|error| {
                            panic!(
                                "floor {floor:?} level {depth_strider} sprint {sprinting} tick {tick}: {error}"
                            )
                        });
                }
            }
        }
    }

    let land = |speed: f64| {
        let mut state = PlayerState::new(Vec3::new(0.5, 5.0, 0.5));
        state.on_ground = true;
        Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    forward: 1.0,
                    movement_speed: Some(speed),
                    ..MovementInput::default()
                },
                &Floored {
                    floor: Some(5.0),
                    water: false,
                },
            )
            .unwrap()
            .velocity
            .z
    };
    assert_eq!(land(2.0), 2.0 * land(1.0));
}
