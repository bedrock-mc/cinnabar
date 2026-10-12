//! Frame geometry an engine paint exposes to input.

use std::sync::Arc;

use json_ui::{HitRegion, LayoutReport};
use ui::UiPoint;

/// What the last engine-drawn frame exposes to input: regions and scroll extents
/// in virtual pixels, plus the virtual → window-logical mapping. Which server form it
/// drew, if any, is paired with it by the form presentation.
#[derive(Clone, Debug)]
pub struct EngineFrame {
    pub hits: Arc<[HitRegion]>,
    pub report: LayoutReport,
    /// Where `button.menu_cancel` (Escape) routes on this screen.
    pub cancel_target: Option<String>,
    /// Window-logical position of virtual `(0, 0)`.
    pub origin: [f32; 2],
    /// Window-logical pixels per virtual pixel.
    pub scale: f32,
    /// A container screen's `root_panel` rect `[x, y, w, h]` in virtual pixels.
    pub panel: Option<[f64; 4]>,
    /// Where each edit box drew its text label.
    pub edit_texts: Vec<EditText>,
    /// The ranges of the output nodes that must stay above anything drawn after the screen:
    /// the held stack and hover tooltips.
    pub top: Vec<std::ops::Range<usize>>,
}

/// Where an edit box's text label draws, for placing its caret under a press.
#[derive(Clone, Debug)]
pub struct EditText {
    /// The edit box's region key.
    pub key: String,
    /// The label's left edge, window-logical px.
    pub left: f32,
    /// The label's font scale and named font.
    pub scale: f32,
    pub font: Option<String>,
}

impl EngineFrame {
    pub fn to_virtual(&self, point: UiPoint) -> [f64; 2] {
        [
            f64::from((point.x() - self.origin[0]) / self.scale),
            f64::from((point.y() - self.origin[1]) / self.scale),
        ]
    }
}
