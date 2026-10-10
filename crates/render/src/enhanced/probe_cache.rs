//! Bounded face scheduling; unchanged captures never create another render view.

use bevy::math::Vec3;

const ALL_FACES: u8 = 0b11_1111;

#[derive(Clone, Copy)]
pub(super) struct ProbeLighting {
    pub sun: Vec3,
    pub phase: u8,
    pub rain: f32,
    pub thunder: f32,
    pub zenith: Vec3,
    pub horizon: Vec3,
}

impl ProbeLighting {
    pub fn reusable(self, next: Self) -> bool {
        self.phase == next.phase
            && self.sun.distance(next.sun) < 0.001
            && (self.rain - next.rain).abs() < 0.01
            && (self.thunder - next.thunder).abs() < 0.01
            && self.zenith.distance(next.zenith) < 0.01
            && self.horizon.distance(next.horizon) < 0.01
    }

    pub fn discontinuity(self, next: Self) -> bool {
        self.phase != next.phase
            || self.sun.distance(next.sun) > 0.25
            || (self.rain - next.rain).abs() > 0.25
            || (self.thunder - next.thunder).abs() > 0.25
    }
}

pub(super) struct ProbeSchedule {
    dirty: u8,
    populated: u8,
    cursor: u32,
    next_refresh: f64,
    next_animation: f64,
    refresh_interval: f64,
    animation_interval: f64,
    pub next_inputs: f64,
}

impl Default for ProbeSchedule {
    fn default() -> Self {
        let budget = super::super::quality::budget(super::super::EnhancedQuality::default());
        Self {
            dirty: ALL_FACES,
            populated: 0,
            cursor: 0,
            next_refresh: 0.0,
            next_animation: 0.0,
            refresh_interval: budget.reflection_refresh,
            animation_interval: budget.reflection_animation,
            next_inputs: 0.0,
        }
    }
}

impl ProbeSchedule {
    pub fn set_intervals(&mut self, refresh: f64, animation: f64) {
        if self.refresh_interval != refresh || self.animation_interval != animation {
            self.refresh_interval = refresh;
            self.animation_interval = animation;
            self.next_refresh = 0.0;
            self.next_animation = 0.0;
            self.next_inputs = 0.0;
        }
    }
    pub fn completed(&mut self, faces: u8) {
        self.dirty &= !(faces & !self.populated);
        self.populated = faces & ALL_FACES;
        self.dirty |= ALL_FACES & !self.populated;
    }
    pub fn invalidate(&mut self) {
        self.dirty = ALL_FACES;
    }

    pub fn relocate(&mut self, priority: u32) {
        self.invalidate();
        self.populated = 0;
        self.cursor = priority % 6;
        self.next_refresh = 0.0;
    }

    pub fn animate(&mut self, now: f64, animated: bool) {
        if animated && now >= self.next_animation {
            self.invalidate();
            self.next_animation = now + self.animation_interval;
        }
    }

    pub fn next_face(&mut self, now: f64) -> Option<u32> {
        if self.dirty == 0 || (self.populated == ALL_FACES && now < self.next_refresh) {
            return None;
        }
        for step in 0..6 {
            let face = (self.cursor + step) % 6;
            let bit = 1 << face;
            if self.dirty & bit != 0 {
                self.dirty &= !bit;
                self.cursor = (face + 1) % 6;
                self.next_refresh = now + self.refresh_interval;
                return Some(face);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn populated() -> ProbeSchedule {
        let mut schedule = ProbeSchedule::default();
        for frame in 0..6 {
            assert_eq!(schedule.next_face(f64::from(frame) / 60.0), Some(frame));
            schedule.completed((1 << (frame + 1)) - 1);
        }
        schedule
    }

    #[test]
    fn unchanged_probe_inputs_do_not_enqueue_cameras() {
        let mut schedule = populated();
        for frame in 6..600 {
            let now = f64::from(frame) / 60.0;
            schedule.animate(now, false);
            assert_eq!(schedule.next_face(now), None);
        }
    }

    #[test]
    fn continuous_changes_do_not_starve_any_face_or_render_every_frame() {
        let mut schedule = populated();
        let mut faces = 0;
        let mut count = 0;
        for frame in 6..66 {
            schedule.invalidate();
            if let Some(face) = schedule.next_face(f64::from(frame) / 60.0) {
                faces |= 1 << face;
                count += 1;
            }
        }
        assert_eq!(faces, ALL_FACES);
        assert!((6..=10).contains(&count));
    }

    #[test]
    fn relocation_warms_priority_face_first_and_then_all_other_faces() {
        let mut schedule = populated();
        schedule.relocate(4);
        let mut faces = 0;
        for frame in 0..6 {
            let next = schedule.next_face(2.0 + f64::from(frame) / 60.0).unwrap();
            if frame == 0 {
                assert_eq!(next, 4);
            }
            faces |= 1 << next;
            schedule.completed(faces);
        }
        assert_eq!(faces, ALL_FACES);
        assert_eq!(schedule.next_face(3.0), None);
    }

    #[test]
    fn unsubmitted_faces_retry_until_shader_and_image_pipelines_are_ready() {
        let mut schedule = ProbeSchedule::default();
        for frame in 0..12 {
            schedule.completed(0);
            assert!(schedule.next_face(f64::from(frame) / 60.0).is_some());
        }
        schedule.completed(ALL_FACES);
        for frame in 12..18 {
            let _ = schedule.next_face(f64::from(frame) / 60.0);
        }
        assert_eq!(schedule.next_face(1.0), None);
    }

    #[test]
    fn small_light_drift_accumulates_before_refresh_and_time_jumps_reset_history() {
        let old = ProbeLighting {
            sun: Vec3::Y,
            phase: 0,
            rain: 0.0,
            thunder: 0.0,
            zenith: Vec3::ONE,
            horizon: Vec3::ONE,
        };
        let mut next = old;
        next.sun.x = 0.0005;
        assert!(old.reusable(next));
        next.sun.x = 0.002;
        assert!(!old.reusable(next));
        assert!(!old.discontinuity(next));
        next.sun = Vec3::NEG_Y;
        assert!(old.discontinuity(next));
    }
}
