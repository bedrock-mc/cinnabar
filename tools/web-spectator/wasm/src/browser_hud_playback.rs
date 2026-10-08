//! Retained HUD state whose lifetime is bounded by one playback epoch.

use json_ui::{Animator, DrawNode, HudModel};

#[derive(Default)]
pub(super) struct BrowserHudPlayback {
    pub(super) cached: Option<(HudModel, [u32; 2], Vec<DrawNode>)>,
    pub(super) animator: Animator,
    pub(super) last_health_drop_millis: Option<u64>,
    title_request: Option<(String, String, u64)>,
    health: Option<(String, f32)>,
}

impl BrowserHudPlayback {
    /// Starts a new playback epoch without retaining animation progress or prior samples.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }

    /// Tracks damage within the current POV, clearing animations when its fighter changes.
    pub(super) fn observe_fighter(&mut self, id: &str, health: f32, now_millis: u64) {
        if self
            .health
            .as_ref()
            .is_none_or(|(previous, _)| previous != id)
        {
            self.cached = None;
            self.animator = Animator::default();
        }
        if self
            .health
            .as_ref()
            .is_some_and(|(previous, value)| previous == id && health < *value)
        {
            self.last_health_drop_millis = Some(now_millis);
        } else if self
            .health
            .as_ref()
            .is_some_and(|(previous, _)| previous != id)
        {
            self.last_health_drop_millis = None;
        }
        self.health = Some((id.to_owned(), health));
    }

    /// Removes the prior POV sample and ends the animation frame when no POV is available.
    pub(super) fn clear_fighter(&mut self) {
        self.health = None;
        self.last_health_drop_millis = None;
        self.title_request = None;
        self.animator.end_frame();
    }

    /// Reuses a title incarnation only while its fighter and event timestamp are unchanged.
    pub(super) fn title_creation_id(
        &mut self,
        fighter: &str,
        updated_at: Option<&str>,
        generation: u64,
    ) -> Option<u64> {
        let Some(updated_at) = updated_at else {
            self.title_request = None;
            return None;
        };
        let creation_id = self
            .title_request
            .as_ref()
            .filter(|(id, updated, _)| id == fighter && updated == updated_at)
            .map_or(generation, |(_, _, sequence)| *sequence);
        self.title_request = Some((fighter.to_owned(), updated_at.to_owned(), creation_id));
        Some(creation_id)
    }
}

#[cfg(test)]
#[path = "browser_hud_playback_tests.rs"]
mod tests;
