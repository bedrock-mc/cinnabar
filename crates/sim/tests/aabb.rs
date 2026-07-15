use sim::{Aabb, PLAYER_HEIGHT, PLAYER_HORIZONTAL_EPSILON, PLAYER_WIDTH, Vec3};

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 1.0e-12,
        "{actual} != {expected}"
    );
}

#[test]
fn player_aabb_uses_bedsim_feet_origin_dimensions_and_horizontal_inset() {
    let aabb = Aabb::player_at(Vec3::new(10.0, 64.0, -3.0));

    assert_close(PLAYER_WIDTH, 0.6);
    assert_close(PLAYER_HEIGHT, 1.8);
    assert_close(PLAYER_HORIZONTAL_EPSILON, 1.0e-4);
    assert_close(aabb.min.x, 10.0 - 0.3 + PLAYER_HORIZONTAL_EPSILON);
    assert_close(aabb.min.y, 64.0);
    assert_close(aabb.min.z, -3.0 - 0.3 + PLAYER_HORIZONTAL_EPSILON);
    assert_close(aabb.max.x, 10.0 + 0.3 - PLAYER_HORIZONTAL_EPSILON);
    assert_close(aabb.max.y, 65.8);
    assert_close(aabb.max.z, -3.0 + 0.3 - PLAYER_HORIZONTAL_EPSILON);
}

#[test]
fn swept_union_contains_start_and_end_without_direction_assumptions() {
    let start = Aabb::new(Vec3::new(-1.0, 2.0, 3.0), Vec3::new(0.0, 4.0, 5.0));
    let swept = start.swept(Vec3::new(3.0, -5.0, -7.0));

    assert_eq!(swept.min, Vec3::new(-1.0, -3.0, -4.0));
    assert_eq!(swept.max, Vec3::new(3.0, 4.0, 5.0));
}

#[test]
fn swept_clip_prevents_tunnelling_and_preserves_unrelated_axes() {
    let moving = Aabb::new(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.6, 1.8, 0.6));
    let wall = Aabb::new(Vec3::new(1.0, -1.0, -1.0), Vec3::new(2.0, 2.0, 2.0));

    let clipped = moving.clip_against(wall, Vec3::new(3.0, 0.25, -0.5));
    assert_close(clipped.x, 0.4);
    assert_close(clipped.y, 0.25);
    assert_close(clipped.z, -0.5);
}

#[test]
fn boxes_separated_on_two_axes_do_not_clip() {
    let moving = Aabb::new(Vec3::ZERO, Vec3::ONE);
    let other = Aabb::new(Vec3::new(2.0, 2.0, 0.0), Vec3::new(3.0, 3.0, 1.0));
    let velocity = Vec3::new(5.0, 0.0, 0.0);

    assert_eq!(moving.clip_against(other, velocity), velocity);
}
