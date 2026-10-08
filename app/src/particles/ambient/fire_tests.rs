use super::offset_position;

#[test]
fn malformed_extreme_coordinates_skip_unrepresentable_neighbor_positions() {
    assert_eq!(offset_position([i32::MAX, 0, 0], [1, 0, 0]), None);
    assert_eq!(offset_position([0, i32::MIN, 0], [0, -1, 0]), None);
}
