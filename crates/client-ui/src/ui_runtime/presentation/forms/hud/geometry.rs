//! Named, inert HUD roots measured from the real completed frame.

use std::collections::BTreeMap;

use json_ui::{HitKind, Rect};
use ui::{
    FONT_DESIGN_PIXEL_TEXELS,
    mod_hud::{HudGeometry, HudGeometryRequests},
};

use super::UiPresentationRuntime;

impl UiPresentationRuntime {
    /// Returns native hotbar and status artwork bounds for the host's HUD editor inputs.
    /// The completed frame supplies pack placement, clipping, transforms and GUI scale.
    pub fn mod_hud_editor_native_keep_out(&self, expected_session: u64) -> Option<[f64; 4]> {
        (self.screens.touch.hud_session == Some(expected_session))
            .then_some(self.screens.touch.hud_keep_out)
            .flatten()
    }

    /// Reads unique inert HUD roots from the last completed frame of `expected_session`.
    /// Results use the same safe-content GUI basis as native personal panels.
    pub fn mod_hud_geometry(
        &self,
        names: &HudGeometryRequests,
        expected_session: u64,
    ) -> BTreeMap<String, HudGeometry> {
        if names.validate().is_err() || self.screens.touch.hud_session != Some(expected_session) {
            return BTreeMap::new();
        }
        let Some(frame) = self.screens.touch.hud.as_ref() else {
            return BTreeMap::new();
        };
        let unit = frame.scale / FONT_DESIGN_PIXEL_TEXELS as f32;
        if !unit.is_finite() || unit <= 0.0 {
            return BTreeMap::new();
        }
        names
            .0
            .iter()
            .filter_map(|name| {
                let mut matches = frame.hits.iter().filter(|hit| {
                    hit.name == *name && hit.kind == HitKind::Panel && hit.pressed.is_none()
                });
                let hit = matches.next()?;
                if matches.next().is_some() {
                    return None;
                }
                let layout = Rect::new(hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h);
                let painted = hit
                    .transform
                    .map_or(layout, |transform| transform.bounds(layout));
                // Frame origin is the safe-area origin; omitting it keeps this in safe content.
                let geometry = HudGeometry {
                    rect: [painted.x, painted.y, painted.w, painted.h]
                        .map(|value| value as f32 * unit),
                    size: [layout.w, layout.h].map(|value| value as f32 * unit),
                };
                geometry.validate().ok()?;
                Some((name.clone(), geometry))
            })
            .collect()
    }
}

#[cfg(test)]
mod tests;
