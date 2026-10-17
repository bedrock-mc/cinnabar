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

/// Quarter turns the current lily-pad tessellator gives the pad at `position`.
pub const fn lily_pad_quarter_turns(position: [i32; 3]) -> u32 {
    let hash = (position[2] as u32).wrapping_mul(0x06eb_fff5)
        ^ (position[0] as u32).wrapping_mul(0x002f_c20f)
        ^ position[1] as u32;
    (hash
        .wrapping_mul(0x0285_b825)
        .wrapping_add(11)
        .wrapping_mul(hash)
        >> 16)
        & 3
}

#[cfg(test)]
mod tests {
    /// The cases the model shader's pad hash executes on a GPU, including wrapping positions.
    #[test]
    fn lily_pad_turns_match_the_shader_cases() {
        for (position, turns) in [
            ([0, 64, 0], 2),
            ([1, 64, 0], 3),
            ([2, 64, 0], 1),
            ([41, 64, -106], 1),
            ([-1, 64, -1], 0),
            ([-17, 63, -112], 2),
            ([i32::MAX, i32::MIN, i32::MIN], 0),
        ] {
            assert_eq!(
                super::lily_pad_quarter_turns(position),
                turns,
                "{position:?}"
            );
        }
    }

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
