use super::*;
use bevy::math::Affine3A;

#[test]
fn cube_order_draws_near_before_far_and_preserves_entity_ties() {
    let far = Entity::from_bits(1);
    let near = Entity::from_bits(2);
    let side = Entity::from_bits(3);
    let visible = [
        (far, SubChunkKey::new(0, 0, 0, 0)),
        (side, SubChunkKey::new(0, 2, 0, 4)),
        (near, SubChunkKey::new(0, 0, 0, 4)),
    ];
    let rangefinder = ViewRangefinder3d::from_world_from_view(&Affine3A::from_translation(
        Vec3::new(0.0, 0.0, 80.0),
    ));
    let mut ties = [near, side];
    ties.sort();
    let expected = [ties[0], ties[1], far];
    assert_eq!(front_to_back_cube_entities(visible, &rangefinder), expected);
    assert_eq!(
        front_to_back_cube_entities(visible.into_iter().rev(), &rangefinder),
        expected,
        "draw order must not depend on extraction order"
    );
}

#[test]
fn cube_order_follows_the_view_when_the_camera_turns() {
    let left = Entity::from_bits(1);
    let right = Entity::from_bits(2);
    let visible = [
        (left, SubChunkKey::new(0, -2, 0, 0)),
        (right, SubChunkKey::new(0, 2, 0, 0)),
    ];
    let view = Affine3A::from_rotation_translation(
        Quat::from_rotation_y(-std::f32::consts::FRAC_PI_2),
        Vec3::new(-80.0, 0.0, 0.0),
    );
    let rangefinder = ViewRangefinder3d::from_world_from_view(&view);
    assert_eq!(
        front_to_back_cube_entities(visible, &rangefinder),
        [left, right]
    );
    let view = Affine3A::from_rotation_translation(
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        Vec3::new(80.0, 0.0, 0.0),
    );
    let rangefinder = ViewRangefinder3d::from_world_from_view(&view);
    assert_eq!(
        front_to_back_cube_entities(visible, &rangefinder),
        [right, left]
    );
}
