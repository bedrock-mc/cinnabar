//! Frozen float regressions for vanilla elytra travel. These are derived cases,
//! not captured live trajectories.

use sim::{
    Aabb, BlockPhysicsFacts, BlockPhysicsFlags, BlockPhysicsSample, CollisionQuery, CollisionWorld,
    MovementEffects, MovementInput, MovementMode, PlayerState, Simulator, SurfaceResponse, Vec3,
    WorldQueryError,
};

struct OpenWorld {
    water: bool,
}

impl CollisionWorld for OpenWorld {
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

const AIR: OpenWorld = OpenWorld { water: false };

fn bits(velocity: Vec3) -> [u32; 3] {
    [velocity.x, velocity.y, velocity.z].map(|axis| (axis as f32).to_bits())
}

fn glide(
    velocity: [f32; 3],
    pitch: f32,
    yaw: f32,
    previous_rotation: Option<[f32; 2]>,
    effects: MovementEffects,
) -> Vec3 {
    let mut state = PlayerState::new(Vec3::new(0.5, 100.0, 0.5));
    state.velocity = Vec3::new(
        f64::from(velocity[0]),
        f64::from(velocity[1]),
        f64::from(velocity[2]),
    );
    state.previous_rotation = previous_rotation;
    Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                mode: MovementMode::Gliding,
                pitch_degrees: f64::from(pitch),
                yaw_degrees: f64::from(yaw),
                effects,
                ..MovementInput::default()
            },
            &AIR,
        )
        .unwrap()
        .velocity
}

#[test]
fn glide_steering_uses_vanilla_float_order() {
    let dive = glide(
        [0.3, -0.4, 0.2],
        45.0,
        -60.0,
        None,
        MovementEffects::default(),
    );
    assert_eq!(bits(dive), [0x3ea1_9315, 0xbed6_809d, 0x3e53_03ee]);
    let climb = glide(
        [0.3, -0.1, 1.2],
        -25.0,
        135.0,
        None,
        MovementEffects::default(),
    );
    assert_eq!(bits(climb), [0x3e3f_9c50, 0xbd55_0a7b, 0x3f7d_3066]);
}

/// Slow falling replaces glide gravity even while the glider is rising.
#[test]
fn slow_falling_glide_gravity_ignores_vertical_direction() {
    let slow = MovementEffects {
        slow_falling: true,
        ..MovementEffects::default()
    };
    let rising = glide([0.0, 0.3, 0.6], 0.0, 0.0, None, slow);
    assert_eq!(bits(rising), [0x31b2_6d59, 0x3e95_460b, 0x3f18_1063]);
    let ordinary = glide([0.0, 0.3, 0.6], 0.0, 0.0, None, MovementEffects::default());
    assert_eq!(bits(ordinary), [0x31b2_6d59, 0x3e8c_7e28, 0x3f18_1063]);
}

/// A firework boost pulls velocity toward 1.5 times the look before drag.
#[test]
fn firework_boost_steers_glide_toward_the_look() {
    let boosted = glide(
        [0.0, -0.1, 0.5],
        20.0,
        30.0,
        None,
        MovementEffects {
            glide_boost: true,
            ..MovementEffects::default()
        },
    );
    assert_eq!(bits(boosted), [0xbed2_08fa, 0xbeae_e337, 0x3f6e_eee6]);
    let unboosted = glide(
        [0.0, -0.1, 0.5],
        20.0,
        30.0,
        None,
        MovementEffects::default(),
    );
    assert_eq!(bits(unboosted), [0xbcf3_ac5a, 0xbde8_69c2, 0x3efe_79f2]);
}

/// Steering reads the previous rotation advanced by the wrapped difference,
/// whose float rounding can move the look's trig-table index.
#[test]
fn glide_look_interpolates_from_the_previous_rotation() {
    let pitch = f32::from_bits(0x424c_a786);
    let yaw = f32::from_bits(0x428e_ab1f);
    let previous = [f32::from_bits(0xc22b_487a), f32::from_bits(0x4277_45bc)];
    let steered = glide(
        [0.2, -0.3, 0.4],
        pitch,
        yaw,
        Some(previous),
        MovementEffects::default(),
    );
    assert_eq!(bits(steered), [0x3dfe_d291, 0xbeab_cc05, 0x3ebf_c7df]);
    let unrotated = glide(
        [0.2, -0.3, 0.4],
        pitch,
        yaw,
        None,
        MovementEffects::default(),
    );
    assert_eq!(bits(unrotated), [0x3dfe_d1a7, 0xbeab_cc05, 0x3ebf_c733]);
}

#[test]
fn each_tick_retains_its_rotation_for_the_next() {
    let mut state = PlayerState::new(Vec3::new(0.5, 100.0, 0.5));
    Simulator::default()
        .tick(
            &mut state,
            MovementInput {
                pitch_degrees: 12.5,
                yaw_degrees: -170.0,
                ..MovementInput::default()
            },
            &AIR,
        )
        .unwrap();
    assert_eq!(state.previous_rotation, Some([12.5, -170.0]));
}

/// Water travel takes precedence over gliding.
#[test]
fn gliding_in_water_follows_water_travel() {
    let water = OpenWorld { water: true };
    let travel = |mode| {
        let mut state = PlayerState::new(Vec3::new(0.5, 100.0, 0.5));
        state.velocity = Vec3::new(0.2, -0.3, 0.4);
        Simulator::default()
            .tick(
                &mut state,
                MovementInput {
                    mode,
                    pitch_degrees: 30.0,
                    ..MovementInput::default()
                },
                &water,
            )
            .unwrap()
            .velocity
    };
    assert_eq!(travel(MovementMode::Gliding), travel(MovementMode::Walking));
}
