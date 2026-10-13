use super::*;

#[test]
fn shallow_axis_does_not_supply_a_pick_face() {
    let hit = pick_box_point(
        [0.0, 1.0, 0.0],
        [0.00003, 1.0, 3.0],
        [0.000015, 0.0, 1.0],
        [1.0, 2.0, 1.6],
    )
    .unwrap();
    // The shallow X face is skipped, so the Z face supplies the click position.
    assert_eq!(hit[2].to_bits(), 0x3fcc_ccce);
    assert!(hit[0] >= 0.000015);
}

#[test]
fn segment_endpoint_rounding_controls_the_world_space_hit() {
    let actor = super::super::tests::actor(1, "minecraft:pig", [0.0; 3], None);
    let origin = [16_384.0, 1.0, 0.0];
    let hit = pick_actor_by(
        [&actor].into_iter(),
        |_| [([16_385.0, 0.0, 0.0], [16_386.0, 2.0, 3.0])],
        None,
        origin,
        [0.6, 0.0, 0.8],
        3.0,
    )
    .unwrap();
    let end_x = origin[0] + 0.6_f32 * 3.0;
    let min_x = 16_385.0_f32 - ACTOR_PICK_RADIUS;
    let fraction = (min_x - origin[0]) / (end_x - origin[0]);
    assert_eq!(hit.point, [min_x, 1.0, (0.8_f32 * 3.0) * fraction]);
    assert_ne!(
        hit.point[2],
        ((f64::from(min_x) - f64::from(origin[0])) * 4.0 / 3.0) as f32
    );
}

#[test]
fn segment_hits_include_endpoints_and_inside_origins() {
    let min = [1.0, 0.0, 0.0];
    let max = [2.0, 2.0, 2.0];
    assert_eq!(
        pick_box_point([0.0, 1.0, 1.0], [1.0, 1.0, 1.0], min, max),
        Some([1.0; 3])
    );
    assert_eq!(pick_box_point([1.0; 3], [3.0; 3], min, max), Some([1.0; 3]));
    assert_eq!(
        pick_box_point([0.0, 1.0, 1.0], [0.9, 1.0, 1.0], min, max),
        None
    );
}
