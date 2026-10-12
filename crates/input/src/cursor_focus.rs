//! Retained desktop input ownership across focus and occlusion changes.

/// How long a platform may take to grant a requested capture before the request counts as
/// refused, in seconds. Browsers grant pointer lock within a few frames, and refuse one asked for
/// too soon after the player left it or without a click or key press behind it.
pub const CAPTURE_GRANT_SECONDS: f64 = 1.0;

/// Focus loss requires an explicit gameplay return before pointer capture resumes.
#[derive(Debug)]
pub struct CursorFocus {
    focused: bool,
    occluded: bool,
    lost_this_frame: bool,
    waiting_for_return: bool,
    activated: bool,
    /// Whether the platform held the capture last observed, and since when it has not granted one.
    platform_held: bool,
    grant_requested_at: Option<f64>,
}

impl Default for CursorFocus {
    fn default() -> Self {
        Self {
            focused: true,
            occluded: false,
            lost_this_frame: false,
            waiting_for_return: false,
            activated: false,
            platform_held: false,
            grant_requested_at: None,
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

    /// Follows a platform that ends capture on its own, as a browser's Escape ends pointer lock
    /// without a focus change. `requested` is whether the game asks for capture and `held` whether
    /// the platform holds it. Losing a held capture, or no grant within
    /// [`CAPTURE_GRANT_SECONDS`] of `now_seconds`, loses input as a focus loss does, so a click on
    /// the game captures again.
    pub fn platform_capture(&mut self, requested: bool, held: bool, now_seconds: f64) {
        if !requested || held {
            self.platform_held = requested;
            self.grant_requested_at = None;
            return;
        }
        let lost = std::mem::take(&mut self.platform_held);
        let requested_at = *self.grant_requested_at.get_or_insert(now_seconds);
        if lost || now_seconds - requested_at >= CAPTURE_GRANT_SECONDS {
            self.grant_requested_at = None;
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
