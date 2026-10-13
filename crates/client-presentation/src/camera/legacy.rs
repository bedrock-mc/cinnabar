//! Adapts completed movement observations to the shared Bedrock 1.12 camera sampler.

use bevy::prelude::Resource;
use render_model::legacy_combat::LegacyCameraState;

/// Camera motion is retained per actor session and never written back to gameplay.
#[derive(Resource, Debug)]
pub struct LegacyCameraMotion {
    pub state: LegacyCameraState,
    session: Option<(u64, u64)>,
    reanchor_epoch: u64,
}

impl Default for LegacyCameraMotion {
    /// Prepares shared trigonometry while the camera resources are installed.
    fn default() -> Self {
        render_model::legacy_combat::prepare_trigonometry();
        Self {
            state: LegacyCameraState::default(),
            session: None,
            reanchor_epoch: 0,
        }
    }
}

impl LegacyCameraMotion {
    /// Clears old motion and reports when an existing camera must skip pre-correction history.
    pub fn observe_subject(
        &mut self,
        session: Option<(u64, u64)>,
        tick: Option<u64>,
        reanchor_epoch: u64,
    ) -> bool {
        let reanchored = self.reanchor_epoch != reanchor_epoch && self.state.last_tick().is_some();
        if self.session != session
            || reanchored
            || tick.is_none()
            || tick
                .zip(self.state.last_tick())
                .is_some_and(|(tick, last)| tick < last)
        {
            self.state = LegacyCameraState::default();
        }
        self.session = session;
        self.reanchor_epoch = reanchor_epoch;
        reanchored
    }
}

#[cfg(test)]
mod tests;
