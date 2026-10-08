//! Wither component values consumed by the authored spawn and death presentation.

use protocol::{ActorMetadata, ActorMetadataValue};

use super::{ActorKind, ActorSnapshot};

pub(crate) const INVULNERABLE_TICKS_KEY: u32 = 48;
const SHIELD_DISABLED_KEY: u32 = 52;
const DEATH_TICKS: u16 = 200;
pub(crate) const SWELL_DIVISOR: f32 = 28.0;
const SPAWN_OVERLAY_STEP: f32 = 0.0075;
const DEATH_OVERLAY_STEP: f32 = 0.005;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct State {
    invulnerable_ticks: i32,
    death_ticks: u16,
    swell: [u16; 2],
    overlay_alpha: f32,
    shield_disabled: bool,
    shield_interval: u16,
}

impl Default for State {
    fn default() -> Self {
        Self {
            invulnerable_ticks: 0,
            death_ticks: 0,
            swell: [0; 2],
            overlay_alpha: 1.0,
            shield_disabled: false,
            shield_interval: 15,
        }
    }
}

impl State {
    /// Accepts the native metadata types while retaining local fade and swell history.
    pub(super) fn observe(&mut self, metadata: &ActorMetadata) {
        match (&metadata.value, metadata.key) {
            (ActorMetadataValue::Int(ticks), INVULNERABLE_TICKS_KEY) => {
                self.invulnerable_ticks = *ticks;
            }
            (ActorMetadataValue::Short(disabled), SHIELD_DISABLED_KEY) => {
                self.shield_disabled = *disabled != 0;
            }
            _ => {}
        }
    }

    /// Advances the client-owned components once per completed actor tick.
    pub(super) fn tick(&mut self, dead: bool) {
        if !dead {
            if self.invulnerable_ticks > 0 {
                self.invulnerable_ticks -= 1;
                self.overlay_alpha = (self.overlay_alpha - SPAWN_OVERLAY_STEP).clamp(0.0, 1.0);
            }
            return;
        }
        if self.death_ticks == DEATH_TICKS {
            return;
        }
        self.death_ticks += 1;
        if self.death_ticks.is_multiple_of(self.shield_interval) {
            self.shield_disabled = !self.shield_disabled;
            self.shield_interval = self.shield_interval.saturating_sub(1).max(1);
        }
        self.invulnerable_ticks = i32::from(DEATH_TICKS - self.death_ticks);
        self.overlay_alpha += DEATH_OVERLAY_STEP;
        self.swell = [self.swell[1], self.swell[1] + 1];
    }

    pub(super) fn death_ticks(&self) -> u16 {
        self.death_ticks
    }
}

impl ActorSnapshot {
    /// Whether the snapshot owns wither-specific lifecycle presentation.
    pub(crate) fn is_wither(&self) -> bool {
        matches!(&self.kind, ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:wither")
    }

    /// Reads a wither component without advancing it or changing its interpolation history.
    pub(crate) fn wither_query(&self, name: &str, frame_alpha: f32) -> Option<f32> {
        let state = self.status.wither_animation.as_ref()?;
        match name {
            "swell_amount" => {
                let [previous, current] = state.swell.map(f32::from);
                Some((previous + (current - previous) * frame_alpha) / SWELL_DIVISOR)
            }
            "overlay_alpha" => Some(state.overlay_alpha),
            "invulnerable_ticks" => Some(state.invulnerable_ticks as f32),
            "is_shield_powered" => Some(f32::from(!state.shield_disabled)),
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "wither_animation_tests.rs"]
mod tests;
