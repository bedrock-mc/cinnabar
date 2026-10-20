//! Retained desktop input ownership across focus and occlusion changes.

/// Retains gameplay capture across focus loss and explicit returns from screens.
#[derive(Debug)]
pub struct CursorFocus {
    focused: bool,
    occluded: bool,
    lost_this_frame: bool,
    waiting_for_return: bool,
    activated: bool,
    resume_capture: bool,
    /// Gameplay holds the pointer on platforms without an OS cursor grab, as a lock would.
    grab_free_capture: bool,
}

impl Default for CursorFocus {
    fn default() -> Self {
        Self {
            focused: true,
            occluded: false,
            lost_this_frame: false,
            waiting_for_return: false,
            activated: false,
            resume_capture: false,
            grab_free_capture: false,
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

    /// Records whether gameplay holds the pointer where the platform cannot lock the cursor.
    pub fn set_grab_free_capture(&mut self, captured: bool) {
        self.grab_free_capture = captured;
        self.record_capture(captured);
    }

    /// Whether gameplay holds the pointer without an OS grab, while capture stays authorized.
    pub const fn grab_free_capture(&self) -> bool {
        self.grab_free_capture && self.capture_allowed()
    }

    /// Remembers the last available frame's capture request through background frames.
    pub fn record_capture(&mut self, captured: bool) {
        if self.available() {
            self.resume_capture = captured;
        }
    }

    /// Suspends capture while desktop input leaves the game.
    fn lose_input(&mut self) {
        self.lost_this_frame = true;
        self.waiting_for_return = !self.resume_capture;
        self.grab_free_capture = false;
    }
}

#[cfg(test)]
mod tests;
