//! Focused regressions derived from current 1.26.50.26 canonical movement
//! bodies and their matching PE data. These use a synthetic fully wet world;
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
    // RVA 0x0320fc20: sprint is ActorData bit 3; y uses the independent
    // water retention. RVA 0x0322d5d0 then applies water gravity outside swim.
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
    // Dispatch 0x09fdd550 excludes MobIsJumping; jump 0x0a5dc2e0 uses
    // the default liquid impulse. Swimming skips gravity (0x0322d5d0).
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
    // Current 0x09fd2140 selects the faster steering rate below its dive
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
    // WaterSinkInputSystem 0x0dc3db30 adds the descent input in water,
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
    // Current 0x09eeeb70 floors attach7 and copies its material's liquid byte.
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
