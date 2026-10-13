//! Applies hardware defaults synchronously without replacing manual preferences.

use bevy::prelude::*;
use launcher::menu::settings_options::graphics_detection::{self, HardwareFacts};
use std::sync::Arc;

use crate::menu::MenuRuntime;

/// Holds the hardware facts sampled once after renderer initialization.
#[derive(Resource)]
pub struct GraphicsHardware(pub HardwareFacts);

/// Tracks first-launch application and a notice awaiting the UI runtime.
#[derive(Resource, Default)]
pub struct Detection {
    initialized: bool,
    notice: Option<&'static str>,
}

impl MenuRuntime {
    /// Reuses cached hardware tiers unless explicitly requested, preserving user edits.
    fn detect_graphics(&mut self, facts: &HardwareFacts, explicit: bool) -> bool {
        let key = facts.adapter.cache_key();
        let cached = (!explicit)
            .then(|| self.settings_options.graphics_tier(&key))
            .flatten();
        let tier = cached.unwrap_or_else(|| facts.tier());
        let recommendation = graphics_detection::recommend(&self.settings_options, tier);
        let settings = Arc::make_mut(&mut self.settings_options);
        if cached.is_none() {
            settings.cache_graphics_tier(key, tier);
            self.settings_dirty = true;
        }
        let changed = settings.apply_graphics_recommendation(recommendation);
        if changed {
            self.settings_dirty = true;
            self.settings_apply = true;
        }
        changed
    }
}

/// Applies startup defaults or a Video-settings request and emits one completion toast.
pub fn drive(
    mut state: ResMut<Detection>,
    hardware: Option<Res<GraphicsHardware>>,
    mut menu: ResMut<MenuRuntime>,
    mut ui: Option<ResMut<crate::ui_resources::UiRuntimeResource>>,
    time: Option<Res<Time<Real>>>,
) {
    if let Some(hardware) = hardware {
        let explicit = std::mem::take(&mut menu.graphics_detection_requested);
        if !state.initialized || explicit {
            let changed = menu.detect_graphics(&hardware.0, explicit);
            if changed || explicit {
                state.notice = Some(graphics_detection::TUNED_MESSAGE);
            }
            state.initialized = true;
        }
    }
    if let (Some(ui), Some(time)) = (ui.as_mut(), time.as_deref())
        && let Some(notice) = state.notice.take()
    {
        ui.push_client_toast(notice, time.elapsed().as_millis() as u64);
    }
}

#[cfg(test)]
mod tests;
