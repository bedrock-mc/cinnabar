use super::{Vec3, apply_relative_movement};

#[test]
fn collision_flags_use_the_native_float_epsilon_boundary() {
    use crate::{Aabb, CollisionQuery, CollisionWorld, WorldQueryError};
    struct Wall;
    impl CollisionWorld for Wall {
        fn collision_boxes(
            &self,
            query: Aabb,
        ) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            let wall = Aabb::new(
                Vec3::new(f64::from(0.3_f32), 0.0, -1.0),
                Vec3::new(1.0, 4.0, 1.0),
            );
            Ok(CollisionQuery::synthetic(if wall.intersects(query) {
                vec![wall]
            } else {
                Vec::new()
            }))
        }
    }
    for (speed, expected) in [
        (f32::EPSILON, false),
        (f32::EPSILON.next_up(), true),
        (2.0e-6, true),
    ] {
        let motion = super::collision::resolve_motion(
            &Wall,
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(f64::from(speed), 0.0, 0.0),
            false,
            1.8,
        )
        .unwrap();
        assert_eq!(motion.resolved.x, 0.0);
        assert_eq!(motion.collisions.x, expected);
    }
}

#[test]
fn water_acceleration_multiplies_effective_level_before_division() {
    for (speed, level, grounded, expected) in [
        (0.1, 2, true, 0x3d96_2fc9),
        (0.13, 1, true, 0x3d68_1b4e),
        (0.13, 3, false, 0x3d99_9999),
    ] {
        let input = super::MovementInput {
            movement_speed: Some(speed),
            ..Default::default()
        };
        let effective = super::depth_strider_level(level, grounded);
        let actual = super::water_travel_speed(&input, 1.0, effective);
        assert_eq!((actual as f32).to_bits(), expected);
    }
}

#[test]
fn jump_lookup_uses_float_indices_and_float_division_for_table_angles() {
    // Vanilla indexes the table initialized with sinf(i / 10430.378f).
    for (yaw, sine, cosine) in [
        (5.625_f32, 0x3dc8_bd36, 0x3f7e_c46d),
        (-5.625, 0xbdc8_bd04, 0x3f7e_c46d),
        (90.0, 0x3f80_0000, 0xb3bb_bd2e),
        (180.0, 0xb3bb_bd2e, 0xbf80_0000),
    ] {
        let angle = f64::from(yaw.to_radians());
        assert_eq!((crate::math::minecraft_sin(angle) as f32).to_bits(), sine);
        assert_eq!((crate::math::minecraft_cos(angle) as f32).to_bits(), cosine);
    }
}

#[test]
fn distant_position_rounding_does_not_change_motion_or_invent_collisions() {
    use crate::{
        Aabb, CollisionQuery, CollisionWorld, MovementInput, PlayerState, Simulator,
        WorldQueryError,
    };
    struct Empty;
    impl CollisionWorld for Empty {
        fn collision_boxes(&self, _: Aabb) -> Result<CollisionQuery<Vec<Aabb>>, WorldQueryError> {
            Ok(CollisionQuery::synthetic(Vec::new()))
        }
    }
    let mut state = PlayerState::new(Vec3::new(10000.5, 4.0, 10000.5));
    state.velocity = Vec3::new(0.0, 0.0, f64::from(0.02_f32));
    let result = Simulator::default()
        .tick(&mut state, MovementInput::default(), &Empty)
        .unwrap();
    assert_eq!(result.position.z, 10000.51953125);
    assert_eq!(result.movement.z, f64::from(0.02_f32));
    assert_eq!(result.velocity.z, f64::from(0.02_f32 * 0.91_f32));
    assert_eq!(result.velocity.y, f64::from(-0.08_f32 * 0.98_f32));
    assert_eq!(result.collisions, super::AxisCollisions::default());
}

#[test]
fn ordinary_steering_uses_native_float_trigonometry_and_product_order() {
    for (yaw, strafe, speed, initial, expected) in [
        (33.333, 0.0, 0.02, [0.0, 0.0], [0xbc30_75d4, 0x3c86_262c]),
        (5.625, 0.98, 0.1, [0.07, -0.03], [0x3e08_a452, 0x3d41_bebe]),
        (90.0, 0.0, 0.02, [0.0, 0.0], [0xbca0_902e, 0xb06b_7ff2]),
    ] {
        let mut velocity = Vec3::new(initial[0], 0.0, initial[1]);
        apply_relative_movement(&mut velocity, strafe, 0.98, yaw, speed);
        assert_eq!((velocity.x as f32).to_bits(), expected[0], "yaw {yaw}");
        assert_eq!((velocity.z as f32).to_bits(), expected[1], "yaw {yaw}");
        assert_eq!(velocity.x, f64::from(velocity.x as f32));
        assert_eq!(velocity.z, f64::from(velocity.z as f32));
    }
}
