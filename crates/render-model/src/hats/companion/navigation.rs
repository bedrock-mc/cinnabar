//! Short, committed ground hops with a verified landing and a swept flight path.
use glam::Vec3;

pub(super) const GRAVITY: f32 = 20.0;
pub(super) const ARRIVAL: f32 = 0.25;

pub(super) struct Hop {
    pub start: Vec3,
    pub end: Vec3,
    pub yaw: f32,
    pub elapsed: f32,
    pub duration: f32,
    pub crest: f32,
}

impl Hop {
    /// Lifts before crossing the riser, then settles onto the selected support.
    pub fn point(&self, elapsed: f32) -> Vec3 {
        let t = (elapsed / self.duration).clamp(0.0, 1.0);
        let horizontal = ((t - 0.25) / 0.65).clamp(0.0, 1.0);
        let horizontal = horizontal * horizontal * (3.0 - 2.0 * horizontal);
        let mut point = self.start.lerp(self.end, horizontal);
        let peak = self.start.y.max(self.end.y) + self.crest;
        let sine = (std::f32::consts::PI * t).sin().max(0.0);
        point.y = if t < 0.5 {
            self.start.y + (peak - self.start.y) * sine
        } else {
            self.end.y + (peak - self.end.y) * sine
        };
        if t == 1.0 { self.end } else { point }
    }
}

/// Commits a supported step up or down that makes progress and fits throughout the flight.
pub(super) fn hop(
    feet: Vec3,
    goal: Vec3,
    direction: Vec3,
    yaw: f32,
    terrain: &mut impl FnMut(Vec3, f32, f32) -> Option<f32>,
    clip: &mut impl FnMut(Vec3, f32, Vec3) -> Vec3,
    clear: &mut impl FnMut(Vec3, f32) -> bool,
) -> Option<Hop> {
    let distance = ((goal - feet) * Vec3::new(1.0, 0.0, 1.0)).length();
    for length in [0.35_f32, 0.55, 0.75, 1.0, 1.25, 1.5] {
        if length > distance + ARRIVAL {
            continue;
        }
        let probe = feet + direction * length;
        let Some(ground) = terrain(probe, 1.0, 3.0) else {
            continue;
        };
        if (ground - feet.y).abs() <= 0.05 || ground > feet.y + 1.001 {
            continue;
        }
        let end = Vec3::new(probe.x, ground, probe.z);
        if ((goal - end) * Vec3::new(1.0, 0.0, 1.0)).length() >= distance - 0.1
            || !clear(end, yaw)
            || !clear(feet, yaw)
        {
            continue;
        }
        for crest in [0.2, 0.1, 0.03] {
            let hop = Hop {
                start: feet,
                end,
                yaw,
                elapsed: 0.0,
                duration: (length / 2.5).clamp(0.4, 0.65),
                crest,
            };
            let mut previous = feet;
            let valid = (1..=24).all(|step| {
                let point = hop.point(hop.duration * step as f32 / 24.0);
                let movement = point - previous;
                let valid =
                    clear(point, yaw) && clip(previous, yaw, movement).distance(movement) <= 0.001;
                previous = point;
                valid
            });
            if valid {
                return Some(hop);
            }
        }
    }
    None
}

/// Finds a nearby supported destination, rather than chasing an obstructed raw owner slot.
pub(super) fn placement(
    target: Vec3,
    yaw: f32,
    terrain: &mut impl FnMut(Vec3, f32, f32) -> Option<f32>,
    clear: &mut impl FnMut(Vec3, f32) -> bool,
) -> Option<Vec3> {
    for distance in [0.0, 0.5, 1.0, 1.5, 2.0] {
        for direction in 0..8 {
            let angle = direction as f32 * std::f32::consts::FRAC_PI_4;
            let candidate = target + Vec3::new(angle.sin(), 0.0, angle.cos()) * distance;
            if let Some(ground) = terrain(candidate, 1.0, 3.0) {
                let feet = Vec3::new(candidate.x, ground, candidate.z);
                if clear(feet, yaw) {
                    return Some(feet);
                }
            }
            if distance == 0.0 {
                break;
            }
        }
    }
    None
}
