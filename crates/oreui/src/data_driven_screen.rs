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

/// A form to draw: its title, its header close button and its controls.
#[derive(Clone, Debug, PartialEq)]
pub struct DataDrivenForm {
    pub title: String,
    /// Whether the header shows a close button, and whether it is enabled.
    pub close: Option<bool>,
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
    /// The open option list's scroll position and extent, and where it drew.
    options_scroll: f32,
    options_max: f32,
    options_viewport: Option<UiRect>,
    /// Each slider's thumb travel by item, in window-logical x.
    travels: Vec<(usize, [f32; 2])>,
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
        canvas
            .offsets
            .insert(draw::OPTIONS_KEY.to_owned(), self.options_scroll);
        let hovered = self.pointer.and_then(|point| self.hit(point));
        let state = draw::State {
            hovered,
            pressed: self.pressed,
            open: self.open,
            editing: self.editing,
        };
        let drawn = draw::form(&mut canvas, form, &state, frame.content, SCROLL_KEY)?;
        for area in &canvas.scrolls {
            if area.key == SCROLL_KEY {
                self.max_scroll = area.max;
                self.scroll = area.offset;
            } else if area.key == draw::OPTIONS_KEY {
                self.options_max = area.max;
                self.options_scroll = area.offset;
            }
        }
        let [left, top] = [frame.safe_area.left(), frame.safe_area.top()];
        let shift = |b: [f32; 4]| rect(b[0] + left, b[1] + top, b[2] + left, b[3] + top).ok();
        self.hits = drawn
            .hits
            .into_iter()
            .filter_map(|(hit, bounds)| {
                let min = bounds.min();
                let max = bounds.max();
                shift([min.x(), min.y(), max.x(), max.y()]).map(|bounds| (hit, bounds))
            })
            .collect();
        self.options_viewport = drawn.options.and_then(shift);
        self.travels = drawn
            .travels
            .into_iter()
            .map(|(item, [start, end])| (item, [start + left, end + left]))
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

    /// Tracks the pointer for next frame's hover state.
    pub fn set_pointer(&mut self, position: Option<UiPoint>) {
        self.pointer = position;
    }

    /// Marks the control held down, drawn pressed until release.
    pub fn set_pressed(&mut self, hit: Option<FormHit>) {
        self.pressed = hit;
    }

    /// Opens the option list of the dropdown at item position `item`, or closes it; a newly
    /// opened list starts at its top.
    pub fn set_open_dropdown(&mut self, item: Option<usize>) {
        if self.open != item {
            self.options_scroll = 0.0;
            self.options_viewport = None;
        }
        self.open = item;
    }

    /// Marks the text field at item position `item` as taking typed text.
    pub fn set_editing(&mut self, item: Option<usize>) {
        self.editing = item;
    }

    /// Scrolls by `delta` logical px (positive moving content up): the open option list under
    /// `position`, otherwise the control column.
    pub fn scroll_at(&mut self, position: Option<UiPoint>, delta: f32) {
        if !delta.is_finite() {
            return;
        }
        let over_options = self.open.is_some()
            && position
                .zip(self.options_viewport)
                .is_some_and(|(point, viewport)| viewport.contains(point));
        if over_options {
            self.options_scroll = (self.options_scroll + delta).clamp(0.0, self.options_max);
        } else {
            self.scroll = (self.scroll + delta).clamp(0.0, self.max_scroll);
        }
    }

    /// Where window-logical `x` lies along slider `item`'s thumb travel, from 0 to 1.
    #[must_use]
    pub fn slider_fraction(&self, item: usize, x: f32) -> Option<f32> {
        let [start, end] = self
            .travels
            .iter()
            .find_map(|(candidate, travel)| (*candidate == item).then_some(*travel))?;
        Some(if end > start {
            ((x - start) / (end - start)).clamp(0.0, 1.0)
        } else {
            0.0
        })
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
