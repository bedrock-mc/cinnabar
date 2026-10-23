//! Script forms shown through data-driven UI: a bordered panel centred over the world with a
//! header title, an optional close button and a scrolling column of controls.
//!
//! The panel geometry follows vanilla captures of a scripted custom form at 1080p: a 36rem
//! panel at least 48rem tall, a 4.8rem header, 2.4rem side insets and 4.8rem input faces.

mod draw;

use ui::{UiPoint, UiRect};
use ui_engine::engine::{
    rect,
    screen::{PaintError, PaintFrame},
};

use super::{paint::Canvas, theme};

/// One control in a form's column; text arrives already resolved for display.
#[derive(Clone, Debug, PartialEq)]
pub enum FormItem {
    Header(String),
    Label(String),
    /// A full-width rule between controls.
    Divider,
    Spacer,
    Button {
        label: String,
        enabled: bool,
    },
    Toggle {
        label: String,
        description: Option<String>,
        on: bool,
        enabled: bool,
    },
    Slider {
        label: String,
        description: Option<String>,
        /// Where the value lies between the minimum (0) and maximum (1).
        fraction: f32,
        /// The value as the label shows it.
        value: String,
        enabled: bool,
    },
    Dropdown {
        label: String,
        description: Option<String>,
        /// Every option's label, in list order.
        options: Vec<String>,
        /// The chosen option's position in `options`.
        selected: Option<usize>,
        enabled: bool,
    },
    TextField {
        label: String,
        description: Option<String>,
        text: String,
        enabled: bool,
    },
}

/// A form to draw: its title, whether it shows a close button, and its controls.
#[derive(Clone, Debug, PartialEq)]
pub struct DataDrivenForm {
    pub title: String,
    pub close: bool,
    pub items: Vec<FormItem>,
}

/// What a press on the form means.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormHit {
    Close,
    /// The control at this position in [`DataDrivenForm::items`].
    Item(usize),
    /// An option of the open dropdown, by its position in the list.
    Option(usize),
}

/// The form's last-frame targets, pointer state and scroll position.
#[derive(Default)]
pub struct DataDrivenScreen {
    hits: Vec<(FormHit, UiRect)>,
    pointer: Option<UiPoint>,
    pressed: Option<FormHit>,
    /// The dropdown item whose option list is open.
    open: Option<usize>,
    /// The text field item taking typed text.
    editing: Option<usize>,
    scroll: f32,
    max_scroll: f32,
}

/// The scroll view key of the form's control column.
const SCROLL_KEY: &str = "ddui_form";

impl DataDrivenScreen {
    /// Draws `form` over the world; its targets land in window-logical px.
    pub fn paint(
        &mut self,
        form: &DataDrivenForm,
        frame: &mut PaintFrame<'_>,
    ) -> Result<(), PaintError> {
        let mut canvas = Canvas::new(
            &mut *frame.nodes,
            &mut *frame.next,
            &mut *frame.layouts,
            frame.font,
            frame.metrics,
            frame.solid_page,
            None,
        );
        canvas.bundle = theme::Bundle::Gameplay;
        canvas.offsets.insert(SCROLL_KEY.to_owned(), self.scroll);
        let hovered = self.pointer.and_then(|point| self.hit(point));
        let state = draw::State {
            hovered,
            pressed: self.pressed,
            open: self.open,
            editing: self.editing,
        };
        let drawn = draw::form(&mut canvas, form, &state, frame.content, SCROLL_KEY)?;
        if let Some(area) = canvas.scrolls.iter().find(|area| area.key == SCROLL_KEY) {
            self.max_scroll = area.max;
            self.scroll = area.offset;
        }
        let [left, top] = [frame.safe_area.left(), frame.safe_area.top()];
        self.hits = drawn
            .into_iter()
            .filter_map(|(hit, bounds)| {
                let min = bounds.min();
                let max = bounds.max();
                rect(min.x() + left, min.y() + top, max.x() + left, max.y() + top)
                    .ok()
                    .map(|bounds| (hit, bounds))
            })
            .collect();
        Ok(())
    }

    /// What a press at the window-logical `position` hits.
    #[must_use]
    pub fn hit(&self, position: UiPoint) -> Option<FormHit> {
        self.hits
            .iter()
            .find_map(|(hit, bounds)| bounds.contains(position).then_some(*hit))
    }

    /// Where `hit` drew last frame, in window-logical px.
    #[must_use]
    pub fn bounds(&self, hit: FormHit) -> Option<UiRect> {
        self.hits
            .iter()
            .find_map(|(candidate, bounds)| (*candidate == hit).then_some(*bounds))
    }

    /// Tracks the pointer for next frame's hover state.
    pub fn set_pointer(&mut self, position: Option<UiPoint>) {
        self.pointer = position;
    }

    /// Marks the control held down, drawn pressed until release.
    pub fn set_pressed(&mut self, hit: Option<FormHit>) {
        self.pressed = hit;
    }

    /// Opens the option list of the dropdown at item position `item`, or closes it.
    pub fn set_open_dropdown(&mut self, item: Option<usize>) {
        self.open = item;
    }

    /// The dropdown item whose option list is open.
    #[must_use]
    pub fn open_dropdown(&self) -> Option<usize> {
        self.open
    }

    /// Marks the text field at item position `item` as taking typed text.
    pub fn set_editing(&mut self, item: Option<usize>) {
        self.editing = item;
    }

    /// Scrolls the control column by `delta` logical px, positive moving content up.
    pub fn scroll_by(&mut self, delta: f32) {
        if delta.is_finite() {
            self.scroll = (self.scroll + delta).clamp(0.0, self.max_scroll);
        }
    }

    /// Forgets the previous form's scroll position and targets.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The last frame's targets.
    #[cfg(any(test, feature = "test-support"))]
    pub fn hits(&self) -> &[(FormHit, UiRect)] {
        &self.hits
    }
}
