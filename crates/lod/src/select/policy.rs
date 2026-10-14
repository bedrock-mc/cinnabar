//! When to walk again. Walks follow movement, not rotation: the measure is view
//! independent, so only a camera that moved [`WALK_MOVE_BLOCKS`] since the last walk, a
//! teleport, a changed projection scale, quality, distance or dimension height, changed
//! resident columns or newly stored sections call for one, at most once per
//! [`MIN_WALK_INTERVAL`] of the caller's monotonic time. Resident columns change with every
//! chunk load while chunks stream in, so they alone call for one at most once per
//! [`RESIDENT_WALK_INTERVAL`].

use std::time::Duration;

use crate::select::View;

/// Camera travel in blocks since the last walk that calls for another.
pub const WALK_MOVE_BLOCKS: f64 = 16.0;
/// Camera travel in blocks between two consecutive polls taken as a teleport, after which
/// the walk forgets which sections were refined.
pub const TELEPORT_BLOCKS: f64 = 64.0;
/// Shortest time between walks: at most 30 a second.
pub const MIN_WALK_INTERVAL: Duration = Duration::from_micros(33_334);
/// Shortest time between walks called for only by changed resident columns.
pub const RESIDENT_WALK_INTERVAL: Duration = Duration::from_millis(100);
/// Shortest time between walks called for only by newly stored sections.
pub const DATA_WALK_INTERVAL: Duration = Duration::from_millis(250);
/// Relative change of the projection scale (a field of view change, such as a spyglass)
/// that calls for a walk.
pub const FOCAL_CHANGE: f64 = 0.05;

/// What else changed since the last poll.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Changes {
    /// The resident chunk columns changed.
    pub resident: bool,
    /// Sections were stored or what the store shows changed.
    pub stored: bool,
}

/// A walk is due now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalkDue {
    /// Forget which sections were refined first ([`super::walk::Selector::reset`]).
    pub reset: bool,
}

/// See the module documentation.
#[derive(Debug, Default)]
pub struct WalkPolicy {
    /// The view and time of the last walk.
    last: Option<(View, Duration)>,
    previous_camera: Option<[f64; 3]>,
    reset_due: bool,
    resident_due: bool,
    stored_due: bool,
}

impl WalkPolicy {
    /// Makes the next poll walk afresh, as after a dimension change.
    pub fn reset(&mut self) {
        self.last = None;
        self.reset_due = true;
    }

    /// Called every frame with the current view, the caller's monotonic `uptime` and what
    /// changed since the last call; says whether to walk now. Changes not yet acted on
    /// are remembered until a walk takes them.
    pub fn poll(&mut self, view: &View, uptime: Duration, changes: Changes) -> Option<WalkDue> {
        if self
            .previous_camera
            .replace(view.camera)
            .is_some_and(|previous| distance(previous, view.camera) >= TELEPORT_BLOCKS)
        {
            self.reset_due = true;
        }
        self.resident_due |= changes.resident;
        self.stored_due |= changes.stored;
        let due = match self.last {
            None => true,
            Some((last, at)) => {
                let elapsed = uptime.saturating_sub(at);
                if elapsed < MIN_WALK_INTERVAL {
                    return None;
                }
                self.reset_due
                    || (self.resident_due && elapsed >= RESIDENT_WALK_INTERVAL)
                    || (self.stored_due && elapsed >= DATA_WALK_INTERVAL)
                    || distance(last.camera, view.camera) >= WALK_MOVE_BLOCKS
                    || settings_changed(&last, view)
            }
        };
        if !due {
            return None;
        }
        let reset = self.reset_due || self.last.is_none();
        self.last = Some((*view, uptime));
        self.reset_due = false;
        self.resident_due = false;
        self.stored_due = false;
        Some(WalkDue { reset })
    }
}

/// True when anything but the camera position differs enough to choose again.
fn settings_changed(last: &View, view: &View) -> bool {
    last.threshold_px != view.threshold_px
        || last.distance_blocks != view.distance_blocks
        || last.min_block_y != view.min_block_y
        || last.max_block_y != view.max_block_y
        || (view.focal_px - last.focal_px).abs() > FOCAL_CHANGE * last.focal_px
}

/// The straight-line distance in blocks between `a` and `b`.
fn distance(a: [f64; 3], b: [f64; 3]) -> f64 {
    (0..3)
        .map(|axis| (a[axis] - b[axis]).powi(2))
        .sum::<f64>()
        .sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view(camera: [f64; 3]) -> View {
        View {
            camera,
            focal_px: 800.0,
            threshold_px: 128.0,
            distance_blocks: 4096.0,
            min_block_y: -64,
            max_block_y: 320,
        }
    }

    fn at(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    #[test]
    fn walks_follow_movement_and_changes_at_a_bounded_rate() {
        let mut policy = WalkPolicy::default();
        let none = Changes::default();
        assert_eq!(
            policy.poll(&view([0.0; 3]), at(0), none),
            Some(WalkDue { reset: true })
        );
        // Staying put, or moving less than the walk distance, never walks.
        assert_eq!(policy.poll(&view([0.0; 3]), at(1_000), none), None);
        assert_eq!(policy.poll(&view([10.0, 0.0, 0.0]), at(2_000), none), None);
        // Too soon after the last walk, even with movement.
        let moved = view([20.0, 0.0, 0.0]);
        let first = policy.poll(&moved, at(2_010), none);
        assert_eq!(first, Some(WalkDue { reset: false }));
        assert_eq!(policy.poll(&view([40.0, 0.0, 0.0]), at(2_020), none), None);
        assert!(
            policy
                .poll(&view([40.0, 0.0, 0.0]), at(2_050), none)
                .is_some()
        );
        // Resident changes walk once their interval passed; stored data waits longer.
        let here = view([40.0, 0.0, 0.0]);
        let resident = Changes {
            resident: true,
            stored: false,
        };
        assert_eq!(policy.poll(&here, at(2_060), resident), None);
        assert_eq!(
            policy.poll(&here, at(2_090), resident),
            None,
            "not every frame"
        );
        assert!(policy.poll(&here, at(2_150), none).is_some(), "remembered");
        let stored = Changes {
            resident: false,
            stored: true,
        };
        assert_eq!(policy.poll(&here, at(2_200), stored), None);
        assert!(policy.poll(&here, at(2_400), none).is_some());
        // A zoom walks; a small focal jitter does not.
        let mut zoomed = here;
        zoomed.focal_px *= 1.01;
        assert_eq!(policy.poll(&zoomed, at(3_000), none), None);
        zoomed.focal_px *= 2.0;
        assert!(policy.poll(&zoomed, at(3_100), none).is_some());
    }

    #[test]
    fn teleports_reset_hysteresis_and_resets_walk_at_once() {
        let mut policy = WalkPolicy::default();
        let none = Changes::default();
        policy.poll(&view([0.0; 3]), at(0), none);
        assert_eq!(
            policy.poll(&view([500.0, 0.0, 0.0]), at(100), none),
            Some(WalkDue { reset: true })
        );
        policy.reset();
        assert_eq!(
            policy.poll(&view([500.0, 0.0, 0.0]), at(101), none),
            Some(WalkDue { reset: true })
        );
    }
}
