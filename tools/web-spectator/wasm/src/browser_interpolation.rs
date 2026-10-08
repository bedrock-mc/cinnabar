//! Sample committed stream poses; never predict movement beyond the newest frame.
use crate::browser_model::{Fighter, Frame};

pub(super) struct FrameMotion<'a> {
    current: &'a Frame,
    previous: Option<&'a Frame>,
    fraction: f32,
}

impl<'a> FrameMotion<'a> {
    pub(super) fn new(current: &'a Frame, previous: Option<&'a Frame>, fraction: f32) -> Self {
        Self {
            current,
            previous: previous.filter(|old| {
                old.id == current.id
                    && old.arena_id == current.arena_id
                    && old.replay_epoch == current.replay_epoch
                    && old.continuity_revision == current.continuity_revision
            }),
            fraction: if fraction.is_finite() {
                fraction.clamp(0.0, 1.0)
            } else {
                1.0
            },
        }
    }

    pub(super) fn previous(&self) -> Option<&'a Frame> {
        self.previous
    }

    fn previous_fighter(&self, fighter: &Fighter) -> Option<&Fighter> {
        self.previous?
            .fighters
            .iter()
            .find(|old| old.id == fighter.id && old.dead == fighter.dead)
    }

    pub(super) fn position(&self, fighter: &Fighter) -> [f32; 3] {
        let Some(old) = self.previous_fighter(fighter) else {
            return fighter.position;
        };
        std::array::from_fn(|axis| {
            old.position[axis] + (fighter.position[axis] - old.position[axis]) * self.fraction
        })
    }

    pub(super) fn angles(&self, fighter: &Fighter) -> [f32; 2] {
        let Some(old) = self.previous_fighter(fighter) else {
            return [fighter.yaw, fighter.pitch];
        };
        // Yaw crosses a circular seam; pitch stays within the camera's vertical
        // range and must not wrap through an upside-down view at its endpoints.
        [
            wrap_degrees(old.yaw + wrap_degrees(fighter.yaw - old.yaw) * self.fraction),
            old.pitch + (fighter.pitch - old.pitch) * self.fraction,
        ]
    }

    pub(super) fn center(&self) -> Option<[f32; 3]> {
        if self.current.fighters.is_empty() {
            return None;
        }
        let total = self
            .current
            .fighters
            .iter()
            .fold([0.0; 3], |mut total, fighter| {
                let position = self.position(fighter);
                for axis in 0..3 {
                    total[axis] += position[axis];
                }
                total
            });
        Some(total.map(|value| value / self.current.fighters.len() as f32))
    }
}

fn wrap_degrees(degrees: f32) -> f32 {
    (degrees + 180.0).rem_euclid(360.0) - 180.0
}

#[cfg(test)]
#[path = "browser_interpolation/tests.rs"]
mod tests;
