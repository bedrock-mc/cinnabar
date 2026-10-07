//! A short retargetable timeline smooths measured progress without changing the reported value.

use super::super::motion::Surface;

const UPDATE_SECONDS: f64 = 0.220;

#[derive(Default)]
pub(in super::super) struct ProgressTween {
    state: Option<State>,
}

struct State {
    surface: Surface,
    target: f32,
    position: Position,
    touched: bool,
}

struct Position {
    from: f32,
    target: f32,
    started: f64,
}

impl Position {
    fn at(value: f32) -> Self {
        Self {
            from: value,
            target: value,
            started: 0.0,
        }
    }

    fn retarget(&mut self, target: f32, seconds: f64) -> f32 {
        let fraction = ((seconds - self.started) / UPDATE_SECONDS).clamp(0.0, 1.0) as f32;
        let current = self.from + (self.target - self.from) * fraction;
        if target != self.target {
            self.from = current;
            self.target = target;
            self.started = seconds;
        }
        current
    }
}

impl ProgressTween {
    pub(in super::super) fn sample(
        &mut self,
        surface: Surface,
        value: f32,
        seconds: f64,
        animated: bool,
    ) -> f32 {
        let target = if value.is_finite() {
            value.clamp(0.0, 1.0)
        } else {
            0.0
        };
        if !animated {
            self.state = None;
            return target;
        }
        if self
            .state
            .as_ref()
            .is_none_or(|state| state.surface != surface || target < state.target)
        {
            self.state = Some(State {
                surface,
                target,
                position: Position::at(target),
                touched: true,
            });
        }
        let state = self.state.as_mut().unwrap();
        state.touched = true;
        state.target = target;
        state.position.retarget(target, seconds)
    }

    pub(super) fn end_frame(&mut self) {
        if self
            .state
            .as_mut()
            .is_some_and(|state| !std::mem::take(&mut state.touched))
        {
            self.state = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_large_progress_update_does_not_cover_most_of_its_distance_in_one_frame() {
        let mut progress = ProgressTween::default();
        let surface = Surface::LoadingStatus(1);
        progress.sample(surface, 0.0, 0.0, true);
        progress.sample(surface, 0.8, 1.0, true);
        let first_frame = progress.sample(surface, 0.8, 1.0 + 1.0 / 60.0, true);
        assert!(first_frame > 0.0 && first_frame < 0.8 * 0.12);
    }

    #[test]
    fn loading_bar_updates_interpolate_and_retarget_without_jumping_or_restarting() {
        let mut progress = ProgressTween::default();
        let surface = Surface::LoadingStatus(1);
        assert_eq!(progress.sample(surface, 0.1, 0.0, true), 0.1);
        assert_eq!(progress.sample(surface, 0.8, 1.0, true), 0.1);
        let middle = progress.sample(surface, 0.8, 1.04, true);
        assert!(middle > 0.1 && middle < 0.8);
        assert_eq!(progress.sample(surface, 1.0, 1.04, true), middle);
        assert!(progress.sample(surface, 1.0, 1.08, true) > middle);
        assert_eq!(progress.sample(surface, 1.0, 1.3, true), 1.0);
        assert_eq!(progress.sample(surface, 1.0, 5.0, true), 1.0);
    }

    #[test]
    fn loading_bar_resets_for_new_stages_unmounts_and_disabling_animations() {
        let mut progress = ProgressTween::default();
        let surface = Surface::LoadingStatus(1);
        progress.sample(surface, 0.1, 0.0, true);
        progress.sample(surface, 0.9, 1.0, true);
        assert_eq!(
            progress.sample(Surface::LoadingStatus(2), 0.3, 1.01, true),
            0.3
        );
        assert_eq!(progress.sample(surface, 0.9, 1.02, false), 0.9);
        assert_eq!(progress.sample(surface, 0.4, 2.0, true), 0.4);
        assert_eq!(progress.sample(surface, 0.2, 3.0, true), 0.2);
        progress.end_frame();
        progress.end_frame();
        assert_eq!(progress.sample(surface, 0.8, 4.0, true), 0.8);
        assert_eq!(progress.sample(surface, f32::NAN, 5.0, true), 0.0);
    }
}
