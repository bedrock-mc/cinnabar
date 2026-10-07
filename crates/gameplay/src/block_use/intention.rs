//! Held placement history and ray-gated continuation along a single axis.

use super::LocalUse;

/// A clicked support and face chosen by the held placement intention.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacementTarget {
    pub position: [i32; 3],
    pub face: u8,
}

#[derive(Debug, Clone, Copy)]
struct PlacementLine {
    next: [i32; 3],
    direction: [i32; 3],
    face: u8,
}

/// Successful-use history survives failed attempts until use is stopped.
#[derive(Debug, Default)]
pub struct BuildIntention {
    last_success: Option<[i32; 3]>,
    line: Option<PlacementLine>,
    placement: bool,
    first_world_hit: Option<[f32; 3]>,
}

impl BuildIntention {
    /// Returns the destination remembered for the start/stop item-use-on actions.
    pub fn last_success_destination(&self) -> Option<[i32; 3]> {
        self.last_success
    }

    /// The initial world intercept keeps orientation-sensitive items consistent while held.
    pub fn first_world_hit(&self) -> Option<[f32; 3]> {
        self.first_world_hit
    }

    /// A placement intention waits for adjacent attempts before it has a line.
    pub fn unlined(&self) -> bool {
        self.placement && self.line.is_none()
    }

    /// Chooses a fresh hit, a velocity-directed face, or the ray-gated next line cell.
    pub fn target(
        &self,
        hit: Option<PlacementTarget>,
        origin: [f32; 3],
        endpoint: [f32; 3],
        velocity: [f32; 3],
        sneaking: bool,
    ) -> Option<PlacementTarget> {
        if self.placement
            && let Some(line) = self.line
        {
            if !segment_intersects_cell(origin, endpoint, line.next) {
                return None;
            }
            return Some(PlacementTarget {
                position: offset(line.next, line.direction.map(|n| -n))?,
                face: line.face,
            });
        }
        let mut target = hit?;
        if self.placement
            && let Some(previous) = self.last_success
            && let Some(face) = velocity_face(velocity)
        {
            target.face = face;
            if !sneaking {
                target.position = previous;
            }
        }
        Some(target)
    }

    /// Establishes a line from an adjacent attempt and advances it only on success.
    #[allow(clippy::too_many_arguments)]
    pub fn record(
        &mut self,
        repeated: bool,
        destination: [i32; 3],
        outcome: LocalUse,
        placement: bool,
        sneaking: bool,
        world_hit: [f32; 3],
    ) {
        let previous = self.last_success;
        if repeated && self.placement && !sneaking {
            if self.line.is_none()
                && let Some(previous) = previous
            {
                let delta = std::array::from_fn(|axis| {
                    i64::from(destination[axis]) - i64::from(previous[axis])
                });
                if delta.iter().map(|n| n.abs()).sum::<i64>() == 1 {
                    let direction = delta.map(|n| n as i32);
                    self.line = Some(PlacementLine {
                        next: destination,
                        direction,
                        face: velocity_face(direction.map(|n| n as f32)).unwrap(),
                    });
                }
            }
            if outcome != LocalUse::Nothing
                && let Some(line) = &mut self.line
                && line.next == destination
            {
                if let Some(next) = offset(line.next, line.direction) {
                    line.next = next;
                } else {
                    self.line = None;
                }
            }
        }
        self.placement = placement;
        if outcome != LocalUse::Nothing {
            if previous.is_none() {
                self.first_world_hit = Some(world_hit);
            }
            self.last_success = Some(destination);
        }
    }
}

/// Chooses the dominant velocity axis; ties fall through to Z.
fn velocity_face([x, y, z]: [f32; 3]) -> Option<u8> {
    if ![x, y, z].into_iter().all(f32::is_finite) || x * x + y * y + z * z <= 0.01 {
        return None;
    }
    Some(if y.abs() > x.abs() && y.abs() > z.abs() {
        u8::from(y > 0.0)
    } else if x.abs() > y.abs() && x.abs() > z.abs() {
        4 + u8::from(x > 0.0)
    } else {
        2 + u8::from(z > 0.0)
    })
}

/// Checked cell arithmetic prevents continuation outside representable block coordinates.
fn offset(cell: [i32; 3], delta: [i32; 3]) -> Option<[i32; 3]> {
    Some([
        cell[0].checked_add(delta[0])?,
        cell[1].checked_add(delta[1])?,
        cell[2].checked_add(delta[2])?,
    ])
}

/// Tests the complete finite segment against the next unit cell, including contact.
fn segment_intersects_cell(origin: [f32; 3], endpoint: [f32; 3], cell: [i32; 3]) -> bool {
    let mut near = 0.0_f32;
    let mut far = 1.0_f32;
    for axis in 0..3 {
        let low = cell[axis] as f32;
        let high = low + 1.0;
        let delta = endpoint[axis] - origin[axis];
        if !origin[axis].is_finite() || !delta.is_finite() {
            return false;
        }
        if delta == 0.0 {
            if origin[axis] < low || origin[axis] > high {
                return false;
            }
        } else {
            let a = (low - origin[axis]) / delta;
            let b = (high - origin[axis]) / delta;
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return false;
            }
        }
    }
    true
}

/// Placement orientation follows the initial world intercept for these state families.
pub fn orientation_sensitive(canonical_state: Option<&str>) -> bool {
    let Some(states) = canonical_state
        .and_then(|s| serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(s).ok())
    else {
        return false;
    };
    states.keys().any(|key| {
        matches!(
            key.strip_prefix("minecraft:").unwrap_or(key),
            "cardinal_direction"
                | "facing_direction"
                | "vertical_half"
                | "direction"
                | "weirdo_direction"
                | "orientation"
                | "upside_down_bit"
                | "top_slot_bit"
                | "ground_sign_direction"
                | "hanging"
                | "rotation"
                | "torch_facing_direction"
                | "vine_direction_bits"
                | "coral_direction"
        )
    })
}
