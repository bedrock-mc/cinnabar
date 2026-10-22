//! Distance-driven contact cues shared by equipped ground cosmetics.

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Cue {
    Step(f32),
    Land,
}

#[derive(Default)]
pub struct Cadence {
    previous: Option<([f32; 3], bool)>,
    distance: f32,
    side: bool,
    air_seconds: f32,
    descending: bool,
    air_apex: Option<f32>,
}

impl Cadence {
    /// Advances only completed movement ticks; discontinuities prime a new track.
    pub fn advance(&mut self, feet: [f32; 3], grounded: bool, walks: bool, dt: f32) -> Option<Cue> {
        if !feet.iter().all(|v| v.is_finite()) || !dt.is_finite() {
            *self = Self::default();
            return None;
        }
        let Some((previous, was_grounded)) = self.previous.replace((feet, grounded)) else {
            return None;
        };
        let dx = feet[0] - previous[0];
        let dz = feet[2] - previous[2];
        let horizontal = dx.hypot(dz);
        if horizontal > 2.0 || (feet[1] - previous[1]).abs() > 2.0 {
            self.distance = 0.0;
            self.air_seconds = 0.0;
            self.descending = false;
            self.air_apex = None;
            return None;
        }
        if !grounded {
            self.air_seconds += dt.clamp(0.0, 0.25);
            let apex = self
                .air_apex
                .unwrap_or(previous[1])
                .max(previous[1])
                .max(feet[1]);
            self.air_apex = Some(apex);
            self.descending |= feet[1] < apex - 0.02;
            self.distance = 0.0;
            return None;
        }
        let landed = !was_grounded && self.air_seconds >= 0.1 && self.descending;
        self.air_seconds = 0.0;
        self.descending = false;
        self.air_apex = None;
        if landed {
            return Some(Cue::Land);
        }
        if !walks || horizontal < 0.002 {
            return None;
        }
        self.distance += horizontal;
        if self.distance < 1.15 {
            return None;
        }
        self.distance %= 1.15;
        self.side = !self.side;
        Some(Cue::Step(if self.side { 0.15 } else { -0.15 }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_emits_nothing_and_contacts_alternate() {
        let mut c = Cadence::default();
        for _ in 0..100 {
            assert_eq!(c.advance([0.0; 3], true, true, 0.05), None);
        }
        let cues: Vec<_> = (1..=40)
            .filter_map(|i| c.advance([0.0, 0.0, i as f32 * 0.1], true, true, 0.05))
            .collect();
        assert_eq!(cues, [Cue::Step(0.15), Cue::Step(-0.15), Cue::Step(0.15)]);
    }

    #[test]
    fn airborne_motion_only_emits_once_after_descent_and_landing() {
        let mut c = Cadence::default();
        c.advance([0.0; 3], true, true, 0.05);
        for y in [0.3, 0.7, 1.0, 0.8, 0.3] {
            assert_eq!(c.advance([0.0, y, 0.0], false, false, 0.05), None);
        }
        assert_eq!(c.advance([0.0; 3], true, true, 0.05), Some(Cue::Land));
        assert_eq!(c.advance([0.0; 3], true, true, 0.05), None);
    }

    #[test]
    fn teleport_and_startup_do_not_create_a_contact() {
        let mut c = Cadence::default();
        assert_eq!(c.advance([0.0; 3], true, true, 0.05), None);
        assert_eq!(c.advance([0.0, 0.0, 50.0], true, true, 0.05), None);
        assert_eq!(c.advance([0.0, 0.0, 50.1], true, true, 0.05), None);
    }
}

#[cfg(test)]
mod descent_tests {
    use super::*;
    #[test]
    fn the_same_slow_descent_lands_at_twenty_and_one_hundred_twenty_fps() {
        for hz in [20, 120] {
            let mut cadence = Cadence::default();
            let dt = 1.0 / hz as f32;
            cadence.advance([0.0, 0.24, 0.0], false, false, dt);
            for tick in 1..hz / 5 {
                let y = 0.24 - tick as f32 * 1.2 * dt;
                assert_eq!(cadence.advance([0.0, y, 0.0], false, false, dt), None);
            }
            assert_eq!(
                cadence.advance([0.0; 3], true, false, dt),
                Some(Cue::Land),
                "{hz} FPS"
            );
            assert_eq!(cadence.advance([0.0; 3], true, false, dt), None);
        }
    }
}
