//! The scroll view's input-side vocabulary: a layout component's `draggable`
//! axis and the touch overscroll bound (the layout lives in `layout::scroll`).

use crate::tree::ResolvedControl;
use crate::widgets::prop_str;

/// Touch overscroll allowed past either end, as a fraction of the viewport.
pub(crate) const OVERSCROLL: f64 = 0.25;

/// `draggable` on a layout component (`ui::Draggable`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Draggable {
    #[default]
    NotDraggable,
    Horizontal,
    Vertical,
    Both,
}

impl Draggable {
    pub(crate) fn of(control: &ResolvedControl) -> Self {
        match prop_str(control, "draggable") {
            Some("horizontal") => Draggable::Horizontal,
            Some("vertical") => Draggable::Vertical,
            Some("both") => Draggable::Both,
            _ => Draggable::NotDraggable,
        }
    }
}
