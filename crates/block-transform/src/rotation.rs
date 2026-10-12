//! Quarter-turn rotation shared by model geometry and physical queries.

/// One right-handed quarter turn about `axis`; +90 degrees about Y turns north to west.
pub fn rotate_quarter<T: Copy + std::ops::Neg<Output = T>>(value: [T; 3], axis: usize) -> [T; 3] {
    let [x, y, z] = value;
    match axis {
        0 => [x, -z, y],
        1 => [z, y, -x],
        _ => [-y, x, z],
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn quarter_turns_about_y_cycle_north_to_west() {
        assert_eq!(super::rotate_quarter([0, 0, 1], 1), [1, 0, 0]);
        assert_eq!(super::rotate_quarter([1, 0, 0], 1), [0, 0, -1]);
    }

    #[test]
    fn four_turns_about_any_axis_are_identity() {
        for axis in 0..3 {
            let value = [1.0_f32, 2.0, 3.0];
            let mut turned = value;
            for _ in 0..4 {
                turned = super::rotate_quarter(turned, axis);
            }
            assert_eq!(turned, value, "axis {axis}");
        }
    }
}
