//! The status-effect column `mob_effects_renderer` draws: vanilla's layout,
//! computed in whole physical pixels as the reference client does.

use super::{Painter, paint_sprite, visible_bounds};

/// Icon side cap, in GUI px.
const MAX_ICON_SIDE: f32 = 18.0;
/// Icon side per unit of the screen-height-derived d-pad scale, in physical px.
const DPAD_ICON_SIDE: f32 = 10.0;
/// Physical px between icons, and the column's extra top margin beyond one icon.
const ICON_GAP: i32 = 4;
/// Physical px between the control's right edge and the first column.
const RIGHT_INSET: i32 = 16;
/// Physical px the background extends past each icon edge.
const BACKGROUND_INSET: f32 = 2.0;
/// The top band clears this many GUI px plus the status rows, then `TOP_BAND_PAD`.
const TOP_BAND_GUI: f32 = 18.0;
const TOP_BAND_PAD: f32 = 4.0;
/// Status rows the top band always clears, and the GUI px each row takes.
const MIN_STATUS_ROWS: u32 = 2;
const STATUS_ROW_HEIGHT: f32 = 10.0;

/// One active effect, background then icon.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectIcon {
    pub background: &'static str,
    pub icon: &'static str,
    /// Expiry blink; the background stays opaque.
    pub alpha: u8,
}

/// The column's effects in draw order and the status rows its top band clears.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MobEffects {
    pub icons: Vec<EffectIcon>,
    /// Heart rows plus the armor row, while status rows are shown.
    pub status_rows: u32,
}

impl MobEffects {
    pub fn clear(&mut self) {
        self.icons.clear();
    }

    pub(super) fn textures(&self) -> impl Iterator<Item = &str> {
        self.icons
            .iter()
            .flat_map(|icon| -> [&str; 2] { [icon.background, icon.icon] })
    }
}

/// Physical-pixel icon rects for `count` icons in one control; none when the
/// control is too short to hold a single icon.
pub(super) fn icon_rects(
    count: usize,
    control: [f32; 4],
    screen: [f32; 4],
    gui_scale: f32,
    status_rows: u32,
) -> impl Iterator<Item = [f32; 4]> {
    let screen_height = screen[3] - screen[1];
    let dpad = (screen_height * 0.5 / 48.0 - 3.0) * 0.5 + 3.0;
    let side = (DPAD_ICON_SIDE * dpad).min(MAX_ICON_SIDE * gui_scale) as i32;
    let step = side + ICON_GAP;
    let rows = status_rows.max(MIN_STATUS_ROWS) as f32;
    let band =
        (TOP_BAND_GUI * gui_scale + TOP_BAND_PAD + rows * STATUS_ROW_HEIGHT * gui_scale) as i32;
    let top = screen[1] as i32 + band + side + ICON_GAP;
    let right = control[0] as i32 + (control[2] - control[0]) as i32 - RIGHT_INSET - side;
    let height = (control[3] - control[1]) as i32;
    let per_column = if step > 0 {
        (height - (3 * side + 3 * ICON_GAP)) / step
    } else {
        0
    };
    let count = if per_column > 0 { count } else { 0 };
    (0..count).map(move |index| {
        let index = index as i32;
        let x = (right - index / per_column * step) as f32;
        let y = (top + index % per_column * step) as f32;
        [x, y, x + side as f32, y + side as f32]
    })
}

/// Draws each effect's background and icon in the control's column.
pub(super) fn paint(
    painter: &mut Painter<'_>,
    effects: &MobEffects,
    dest: [f32; 4],
    alpha: &dyn Fn([u8; 4]) -> [u8; 4],
) {
    let visible = visible_bounds(painter);
    if effects.icons.is_empty() || visible[0] >= visible[2] || visible[1] >= visible[3] {
        return;
    }
    let gui_scale = painter.metrics.gui_scale;
    let to_physical = gui_scale / painter.px;
    if !to_physical.is_finite() || to_physical <= 0.0 {
        return;
    }
    let rects = icon_rects(
        effects.icons.len(),
        dest.map(|edge| edge * to_physical),
        painter.screen.map(|edge| edge * to_physical),
        gui_scale,
        effects.status_rows,
    );
    for (effect, rect) in effects.icons.iter().zip(rects) {
        let background = [
            rect[0] - BACKGROUND_INSET,
            rect[1] - BACKGROUND_INSET,
            rect[2] + BACKGROUND_INSET,
            rect[3] + BACKGROUND_INSET,
        ];
        let logical = |bounds: [f32; 4]| bounds.map(|edge| edge / to_physical);
        paint_sprite(
            painter,
            None,
            effect.background,
            logical(background),
            255,
            visible,
            alpha,
        );
        paint_sprite(
            painter,
            None,
            effect.icon,
            logical(rect),
            effect.alpha,
            visible,
            alpha,
        );
    }
}

#[cfg(test)]
#[path = "mob_effects_tests.rs"]
mod tests;
