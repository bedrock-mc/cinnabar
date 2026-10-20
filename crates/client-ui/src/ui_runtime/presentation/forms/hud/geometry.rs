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

/// Unions native bottom-HUD cells in the editor's safe-content GUI coordinate space.
pub(super) fn keep_out(
    frame: &ui_engine::frame::EngineFrame,
    paint: &ui_screens::hud_renderers::HudPaint,
) -> Option<[f64; 4]> {
    let unit = f64::from(frame.scale / FONT_DESIGN_PIXEL_TEXELS as f32);
    if !unit.is_finite() || unit <= 0. {
        return None;
    }
    let mut bounds: Option<Rect> = None;
    for hit in &frame.hits {
        let clip = Rect::new(hit.clip.x, hit.clip.y, hit.clip.w, hit.clip.h);
        let local_clip = hit
            .transform
            .and_then(|t| t.inverse())
            .map_or(clip, |t| t.bounds(clip));
        let mut add = |rect: Rect| {
            let rect = hit.transform.map_or(rect, |t| t.bounds(rect));
            let rect = rect.intersect(clip);
            if rect.w > 0. && rect.h > 0. {
                bounds = Some(bounds.map_or(rect, |old| {
                    let x = old.x.min(rect.x);
                    let y = old.y.min(rect.y);
                    Rect::new(
                        x,
                        y,
                        (old.x + old.w).max(rect.x + rect.w) - x,
                        (old.y + old.h).max(rect.y + rect.h) - y,
                    )
                }));
            }
        };
        let cell_rect = |cell: &ui_screens::hud_renderers::Cell| {
            Rect::new(
                hit.rect.x + f64::from(cell.at[0]),
                hit.rect.y + f64::from(cell.at[1]),
                f64::from(cell.size[0]),
                f64::from(cell.size[1]),
            )
        };
        match hit.renderer.as_deref() {
            Some("heart_renderer") => {
                for cell in paint.hearts.visible_cells(
                    [hit.rect.x as f32, hit.rect.y as f32],
                    1.,
                    [
                        local_clip.x as f32,
                        local_clip.y as f32,
                        (local_clip.x + local_clip.w) as f32,
                        (local_clip.y + local_clip.h) as f32,
                    ],
                ) {
                    add(cell_rect(&cell));
                }
            }
            Some("hunger_renderer") => {
                for cell in paint.hunger.cells(1) {
                    add(cell_rect(&cell));
                }
            }
            Some(renderer @ ("armor_renderer" | "bubbles_renderer" | "horse_heart_renderer")) => {
                let cells = match renderer {
                    "armor_renderer" => &paint.armor,
                    "bubbles_renderer" => &paint.bubbles,
                    _ => &paint.mount_hearts,
                };
                for cell in cells {
                    add(cell_rect(cell));
                }
            }
            Some("horse_jump_renderer") if paint.mount_jump.is_some() => {
                add(Rect::new(hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h))
            }
            Some("hotbar_renderer") => {
                add(Rect::new(hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h))
            }
            _ if hit.name == "hotbar_panel" => {
                add(Rect::new(hit.rect.x, hit.rect.y, hit.rect.w, hit.rect.h))
            }
            _ => {}
        }
    }
    bounds.map(|r| [r.x, r.y, r.w, r.h].map(|v| v * unit))
}

#[cfg(test)]
mod tests;
