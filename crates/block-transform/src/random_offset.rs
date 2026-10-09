//! State-owned coordinate displacement shared by model and physical shapes.

/// Authored axis ranges span at most eight pixels in either direction.
pub const MAX_AXIS_OFFSET: f32 = 0.5;

/// One axis's block-unit range and equally spaced sample count; zero is continuous.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RandomOffsetAxis {
    pub range: [f32; 2],
    pub steps: u32,
}

/// An admitted block component. Absence is distinct from an explicit zero override.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RandomOffsetComponent {
    pub axes: [RandomOffsetAxis; 3],
}

impl RandomOffsetComponent {
    /// Resolves X, Y and Z from signed X/Z coordinates, independent of column height.
    #[must_use]
    pub fn offset(self, position: [i32; 3]) -> [f32; 3] {
        let random = crate::bamboo::positional_random(position[0], position[2]);
        std::array::from_fn(|axis| self.axes[axis].sample(random[axis]))
    }

    /// Rejects unordered, non-finite or out-of-bounds authored ranges before admission.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.axes.iter().all(|axis| {
            axis.range.iter().all(|value| value.is_finite())
                && axis.range[0] <= axis.range[1]
                && axis.range[0] >= -MAX_AXIS_OFFSET
                && axis.range[1] <= MAX_AXIS_OFFSET
        })
    }
}

impl RandomOffsetAxis {
    /// Converts an authored pixel range into block units without changing its steps.
    #[must_use]
    pub fn from_pixels(range: [f32; 2], steps: u32) -> Self {
        Self {
            range: range.map(|value| value / 16.0),
            steps,
        }
    }

    /// Samples the axis using the high 24 bits, including the single-step midpoint.
    fn sample(self, random: u64) -> f32 {
        let [min, max] = self.range;
        if min >= max {
            return min;
        }
        if self.steps == 1 {
            return (min + max) * 0.5;
        }
        let unit = (random >> 40) as u32 as f32 * (1.0 / 16_777_216.0);
        if self.steps == 0 {
            min + (max - min) * unit
        } else {
            min + (unit * self.steps as f32).floor() * ((max - min) / (self.steps - 1) as f32)
        }
    }
}

/// Default component registered on every vanilla bamboo stalk state.
pub const BAMBOO: RandomOffsetComponent = RandomOffsetComponent {
    axes: [
        RandomOffsetAxis {
            range: [
                crate::bamboo::OFFSET_MIN,
                crate::bamboo::OFFSET_MIN + crate::bamboo::OFFSET_SPAN,
            ],
            steps: crate::bamboo::OFFSET_STEPS,
        },
        RandomOffsetAxis {
            range: [0.0; 2],
            steps: 0,
        },
        RandomOffsetAxis {
            range: [
                crate::bamboo::OFFSET_MIN,
                crate::bamboo::OFFSET_MIN + crate::bamboo::OFFSET_SPAN,
            ],
            steps: crate::bamboo::OFFSET_STEPS,
        },
    ],
};

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_component_matches_packed_stalks_at_signed_wrapping_coordinates() {
        for [x, z] in [[0, 0], [1, 0], [-1, -1], [16, -32], [i32::MIN, i32::MAX]] {
            for y in [i32::MIN, 0, 90, i32::MAX] {
                assert_eq!(
                    BAMBOO.offset([x, y, z]),
                    crate::bamboo::column_offset([x, y, z])
                );
            }
        }
    }
    #[test]
    fn explicit_axis_overrides_keep_draw_order_and_zero_one_continuous_steps() {
        let mut component = BAMBOO;
        component.axes[0] = RandomOffsetAxis {
            range: [-0.5, 0.5],
            steps: 1,
        };
        component.axes[1] = RandomOffsetAxis {
            range: [0.125, 0.125],
            steps: 0,
        };
        assert_eq!(
            component.offset([0, 99, 0]),
            [0.0, 0.125, BAMBOO.offset([0; 3])[2]]
        );
        component.axes[2] = RandomOffsetAxis {
            range: [0.0, 1.0],
            steps: 0,
        };
        let value = component.offset([-1, 4, -1])[2];
        assert!(value > 0.0 && value < 1.0);
        assert_eq!(
            component.offset([-1, 4, -1]),
            component.offset([-1, 100, -1])
        );
    }
}
