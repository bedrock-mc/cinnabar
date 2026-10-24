//! Pending outputs and retained values follow the lifecycle of their owning subsystem.

#[derive(Default)]
pub(super) struct Environment {
    pub time_override: Option<u32>,
    pub pending_time: Option<Option<u32>>,
    pub weather_override: Option<super::cinnabar::extension::environment::Weather>,
    pub pending_weather: Option<Option<super::cinnabar::extension::environment::Weather>>,
    pub click_limits: [Option<u32>; 2],
    pub pending_click_limits: Option<[Option<u32>; 2]>,
    pub fullbright: bool,
    pub pending_fullbright: Option<bool>,
    pub writes: u32,
}

#[derive(Default)]
pub(super) struct Movement {
    pub snapshot: Option<crate::GameplayMovementSnapshot>,
    pub pending_jump: bool,
    pub pending_jump_cancel: bool,
    pub jump_cancel: bool,
    pub jump_pulse: bool,
    pub writes: u32,
}

impl Environment {
    /// Charges all environment imports against their shared callback allowance.
    pub fn write(&mut self) -> anyhow::Result<()> {
        self.writes += 1;
        anyhow::ensure!(
            self.writes <= super::MAX_IMPORT_WRITES,
            "environment import budget exhausted"
        );
        Ok(())
    }
    /// Drops frame-only staging while retaining successfully published overrides.
    pub fn begin_frame(&mut self) {
        self.pending_fullbright = None;
    }
    /// Starts the shared environment import budget for one guest callback.
    pub fn begin_callback(&mut self) {
        self.writes = 0;
    }
    /// Publishes staged retained values only after a successful frame or event.
    pub fn commit(&mut self) {
        if let Some(enabled) = self.pending_fullbright.take() {
            self.fullbright = enabled;
        }
        if let Some(value) = self.pending_weather.take() {
            self.weather_override = value;
        }
        if let Some(value) = self.pending_click_limits.take() {
            self.click_limits = value;
        }
        if let Some(ticks) = self.pending_time.take() {
            self.time_override = ticks;
        }
    }
    /// Discards all staged and committed authority after revocation or a trap.
    pub fn revoke(&mut self) {
        *self = Self::default();
    }
}

impl Movement {
    /// Charges movement requests against their callback allowance.
    pub fn write(&mut self) -> anyhow::Result<()> {
        self.writes += 1;
        anyhow::ensure!(
            self.writes <= super::MAX_IMPORT_WRITES,
            "movement import budget exhausted"
        );
        Ok(())
    }
    /// Every movement output and observation belongs to one frame.
    pub fn begin_frame(&mut self) {
        *self = Self::default();
    }
    /// Resets the movement import allowance for one guest callback.
    pub fn begin_callback(&mut self) {
        self.writes = 0;
    }
    /// Releases callback observations without erasing the successful frame output.
    pub fn finish_frame(&mut self) {
        self.snapshot = None;
    }
    /// A cancellation takes precedence over a jump requested in the same frame.
    pub fn commit_frame(&mut self) {
        self.jump_cancel = std::mem::take(&mut self.pending_jump_cancel);
        self.jump_pulse = std::mem::take(&mut self.pending_jump) && !self.jump_cancel;
    }
    /// Revocation removes staged and published movement output together.
    pub fn revoke(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests;
