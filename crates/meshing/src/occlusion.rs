//! Bounded proofs that an opaque voxel slab covers a box's entire silhouette.

/// Maximum plane and cell probes shared by one kind of entity in a frame.
pub const ENTITY_OCCLUSION_PROBES: usize = 4096;
/// Minimum gap from the eye to a tested face; camera near clipping must fit inside it.
pub const OCCLUSION_EYE_GAP_BLOCKS: i32 = 1;
const _: () = assert!(OCCLUSION_EYE_GAP_BLOCKS > 0);
const MAX_PLANES: usize = 64;
const ROUNDING_MARGIN: f64 = 0.0001;
const COORDINATE_LIMIT: f64 = 8_000_000.0;

/// Work remaining in this frame. Exhaustion always draws; no old verdict is reused.
#[derive(Debug)]
pub struct OcclusionBudget {
    remaining: usize,
}

impl Default for OcclusionBudget {
    fn default() -> Self {
        Self {
            remaining: ENTITY_OCCLUSION_PROBES,
        }
    }
}

impl OcclusionBudget {
    /// Creates a smaller budget for deterministic workload tests.
    pub fn new(probes: usize) -> Self {
        Self { remaining: probes }
    }

    /// Plane and cell probes still available in the supplied budget.
    pub fn remaining(&self) -> usize {
        self.remaining
    }

    /// Charges work before it happens; a depleted budget cannot prove occlusion.
    fn charge(&mut self) -> bool {
        if self.remaining == 0 {
            return false;
        }
        self.remaining -= 1;
        true
    }

    /// Hides only if one complete opaque slab separates `eye` from every point of the box.
    /// `opaque` must certify a current opaque face toward the supplied front neighbour.
    /// The caller must keep the camera's near-clip region inside the eye gap.
    /// Gaps, missing terrain, invalid bounds and exhausted work all draw. Unlike corner
    /// ray samples, covering the enclosing projected rectangle also covers its interior.
    pub fn hides_box(
        &mut self,
        eye: [f32; 3],
        low: [f32; 3],
        high: [f32; 3],
        mut opaque: impl FnMut([i32; 3], [i32; 3]) -> bool,
    ) -> bool {
        let eye = eye.map(f64::from);
        let low = low.map(f64::from);
        let high = high.map(f64::from);
        if eye
            .iter()
            .chain(&low)
            .chain(&high)
            .any(|v| !v.is_finite() || v.abs() > COORDINATE_LIMIT)
            || (0..3).any(|axis| low[axis] > high[axis])
        {
            return false;
        }
        let distance: [f64; 3] =
            std::array::from_fn(|axis| (low[axis] - eye[axis]).max(eye[axis] - high[axis]));
        let axis = (0..3)
            .max_by(|&a, &b| distance[a].total_cmp(&distance[b]))
            .unwrap();
        if distance[axis] <= f64::from(OCCLUSION_EYE_GAP_BLOCKS) + 1.0 {
            return false;
        }
        let positive = eye[axis] < low[axis];
        let first = if positive {
            eye[axis].ceil() as i32 + OCCLUSION_EYE_GAP_BLOCKS
        } else {
            eye[axis].floor() as i32 - 1 - OCCLUSION_EYE_GAP_BLOCKS
        };
        let u = (axis + 1) % 3;
        let v = (axis + 2) % 3;
        // Every tested plane has the same angular silhouette; only its distance changes.
        let slopes = [u, v].map(|a| {
            let mut range = ProjectionSlopes {
                axis: a,
                low: f64::INFINITY,
                high: f64::NEG_INFINITY,
                margin: ROUNDING_MARGIN,
            };
            for across in [low[a], high[a]] {
                for depth in [low[axis], high[axis]] {
                    let slope = (across - eye[a]) / (depth - eye[axis]);
                    range.low = range.low.min(slope);
                    range.high = range.high.max(slope);
                    range.margin = range
                        .margin
                        .max(2.0 * ROUNDING_MARGIN * (1.0 + slope.abs()));
                }
            }
            if !positive {
                std::mem::swap(&mut range.low, &mut range.high);
            }
            range
        });
        for step in 0..MAX_PLANES {
            if !self.charge() {
                return false;
            }
            let cell = first
                + if positive {
                    step as i32
                } else {
                    -(step as i32)
                };
            if (positive && f64::from(cell + 1) >= low[axis])
                || (!positive && f64::from(cell) <= high[axis])
            {
                break;
            }
            let plane = f64::from(cell + i32::from(!positive));
            let delta = plane - eye[axis];
            let min = slopes.map(|range| eye[range.axis] + delta * range.low);
            let max = slopes.map(|range| eye[range.axis] + delta * range.high);
            let margin = slopes.map(|range| range.margin);
            let min: [i32; 2] = std::array::from_fn(|i| (min[i] - margin[i]).floor() as i32);
            let max: [i32; 2] = std::array::from_fn(|i| (max[i] + margin[i]).floor() as i32);
            let mut covered = true;
            'cells: for x in min[0]..=max[0] {
                for y in min[1]..=max[1] {
                    // Charge the face cell and its front neighbour before querying them.
                    if !self.charge() || !self.charge() {
                        return false;
                    }
                    let mut position = [0; 3];
                    position[axis] = cell;
                    position[u] = x;
                    position[v] = y;
                    let mut neighbour = position;
                    neighbour[axis] += if positive { -1 } else { 1 };
                    if !opaque(position, neighbour) {
                        covered = false;
                        break 'cells;
                    }
                }
            }
            if covered {
                return true;
            }
        }
        false
    }
}

/// Angular bounds of a box along one axis perpendicular to the proof direction.
#[derive(Clone, Copy)]
struct ProjectionSlopes {
    axis: usize,
    low: f64,
    high: f64,
    margin: f64,
}

#[cfg(test)]
mod tests;
