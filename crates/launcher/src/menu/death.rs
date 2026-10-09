//! Real-time stages shared by death input and OreUI presentation.

/// Duration of each death prompt stage and the wait before respawn progress.
pub const STAGE_SECONDS: f64 = 0.5;
/// Time for the death backdrop to reach its final extent.
pub const BACKDROP_SECONDS: f64 = 5.0;
/// Fade duration for the death message, controls, and progress.
pub const CONTENT_FADE_SECONDS: f64 = 0.4;
/// Fade duration for the death backdrop.
pub const OVERLAY_FADE_SECONDS: f64 = 0.8;
/// Respawn commands repeat at this interval while recovery is pending.
pub const RESPAWN_RETRY_SECONDS: f64 = 1.0;

/// Presentation age survives opening the game menu and resets for each death.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DeathPresentation {
    pub active: bool,
    pub hardcore: bool,
    pub exiting_world: bool,
    pub return_seconds: Option<f64>,
    pub elapsed_seconds: f64,
    pub respawn_seconds: Option<f64>,
    pub immediate_respawn: bool,
    pub animations: bool,
}

impl Default for DeathPresentation {
    fn default() -> Self {
        Self {
            active: false,
            hardcore: false,
            exiting_world: false,
            return_seconds: None,
            elapsed_seconds: BACKDROP_SECONDS,
            respawn_seconds: None,
            immediate_respawn: false,
            animations: true,
        }
    }
}

impl DeathPresentation {
    /// Starts a new death; immediate respawn enters its initial wait directly.
    pub fn new(animations: bool, immediate_respawn: bool) -> Self {
        Self {
            active: true,
            elapsed_seconds: 0.0,
            animations,
            immediate_respawn,
            ..Self::default()
        }
    }

    /// Returns when the prompt advances to accepting ordinary button actions.
    pub fn controls_at(self) -> f64 {
        STAGE_SECONDS * if self.animations { 2.0 } else { 1.0 }
    }

    /// Advances real time while bounding ages once every visual has settled.
    pub fn advance(&mut self, delta: f64) {
        if !delta.is_finite() || delta <= 0.0 {
            return;
        }
        self.elapsed_seconds = (self.elapsed_seconds + delta).min(BACKDROP_SECONDS);
        if let Some(age) = &mut self.respawn_seconds {
            *age = (*age + delta).min(STAGE_SECONDS + CONTENT_FADE_SECONDS);
        }
        if let Some(age) = &mut self.return_seconds {
            *age = (*age + delta).min(CONTENT_FADE_SECONDS);
        }
    }

    /// Reports the ordinary action stage; a pending respawn retires its actions.
    pub fn controls_ready(self) -> bool {
        !self.immediate_respawn
            && self.respawn_seconds.is_none()
            && self.elapsed_seconds >= self.controls_at()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn actions_wait_for_the_prompt_even_without_animations() {
        for animations in [false, true] {
            let mut state = DeathPresentation::new(animations, false);
            state.advance(state.controls_at() - 0.001);
            assert!(!state.controls_ready());
            state.advance(0.001);
            assert!(state.controls_ready());
            state.respawn_seconds = Some(0.0);
            assert!(!state.controls_ready());
        }
        let mut immediate = DeathPresentation::new(true, true);
        immediate.advance(100.0);
        assert!(!immediate.controls_ready());
    }

    #[test]
    fn invalid_time_does_not_reveal_actions_or_advance_progress() {
        let mut state = DeathPresentation::new(true, false);
        state.respawn_seconds = Some(0.0);
        let initial = state;
        for delta in [f64::NAN, f64::INFINITY, -1.0, 0.0] {
            state.advance(delta);
            assert_eq!(state, initial);
        }
        state.advance(100.0);
        assert_eq!(state.elapsed_seconds, BACKDROP_SECONDS);
        assert_eq!(
            state.respawn_seconds,
            Some(STAGE_SECONDS + CONTENT_FADE_SECONDS)
        );
    }
}
