//! Retained desktop input ownership across focus and occlusion changes.
use bevy::prelude::Resource;

/// Focus loss requires an explicit gameplay return before pointer capture resumes.
#[derive(Resource, Debug)]
pub struct CursorFocus {
    focused: bool,
    occluded: bool,
    lost_this_frame: bool,
    waiting_for_return: bool,
    activated: bool,
}

impl Default for CursorFocus {
    fn default() -> Self {
        Self {
            focused: true,
            occluded: false,
            lost_this_frame: false,
            waiting_for_return: false,
            activated: false,
        }
    }
}

impl CursorFocus {
    /// Starts a frame without forgetting a pending explicit gameplay return.
    pub fn begin_frame(&mut self, focused: bool) {
        self.lost_this_frame = false;
        self.activated = false;
        self.focus_changed(focused);
    }

    /// Retains even a loss followed by a gain in the same event batch.
    pub fn focus_changed(&mut self, focused: bool) {
        self.focused = focused;
        if !focused {
            self.lose_input();
        }
    }

    /// Occlusion suspends input independently of the window's keyboard focus.
    pub fn occlusion_changed(&mut self, occluded: bool) {
        self.occluded = occluded;
        if occluded {
            self.lose_input();
        }
    }

    /// Reports whether desktop input may reach the game this frame.
    pub const fn available(&self) -> bool {
        self.focused && !self.occluded && !self.lost_this_frame
    }

    /// Keeps the activation edge after the owning screen consumes physical input.
    pub fn record_activation(&mut self, activated: bool) {
        self.activated = self.available() && activated;
    }

    /// Retains an explicit screen dismissal while its response still owns input.
    pub fn authorize_screen_return(&mut self) {
        if self.available() && self.activated {
            self.waiting_for_return = false;
        }
    }

    /// Permits capture after a focused click or an explicit return from a screen.
    pub fn allow_capture(&mut self, ui_owned_input: bool, clicked: bool) -> bool {
        if self.available() && !ui_owned_input && clicked {
            self.waiting_for_return = false;
        }
        self.capture_allowed()
    }

    /// Gates late cursor writers as well as the camera's normal capture request.
    pub const fn capture_allowed(&self) -> bool {
        self.available() && !self.waiting_for_return
    }

    /// Retires implicit capture authorization when desktop input leaves the game.
    fn lose_input(&mut self) {
        self.lost_this_frame = true;
        self.waiting_for_return = true;
    }
}

#[cfg(test)]
mod tests;
