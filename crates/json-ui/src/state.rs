//! Per-screen interaction state the caller keeps between frames and the layout
//! reads back: which control the pointer hovers or holds, which has focus, and
//! each scroll view's offset and retained dynamics. Controls are addressed by
//! their layout key — the `/`-joined instance-name path with a `[index]` suffix
//! on factory instances — so a key survives a re-bind as long as the tree's
//! shape does.

use std::collections::BTreeMap;

use crate::predicate::Scalar;
use crate::widgets::{Draggable, ScrollMotion};

/// What the caller tells layout about the live pointer/focus/scroll state.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewState {
    pub hovered: Option<String>,
    pub pressed: Option<String>,
    pub focused: Option<String>,
    /// Scroll view key → requested offset in virtual pixels (clamped by layout).
    pub scroll: BTreeMap<String, f64>,
    pub focus_memory: FocusMemory,
    /// What the screen's components wrote into their bags; binding reads it.
    pub components: crate::component::Components,
    /// Scroll view key → what its component keeps between layouts.
    pub scroll_state: BTreeMap<String, ScrollRetained>,
    /// The pointer in virtual pixels, for `follows_cursor` controls; feed it only
    /// while [`LayoutReport::tracks_pointer`], or pointer moves relayout.
    pub pointer: Option<[f64; 2]>,
    /// `draggable` control key → its accumulated drag offset.
    pub drags: BTreeMap<String, [f64; 2]>,
}

/// A scroll view's retained component state beyond its offset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrollRetained {
    /// The extent last laid out, for `jump_to_bottom_on_update`.
    pub extent: Option<f64>,
    /// `#scrollbar_hit_bottom` latches once reached.
    pub hit_bottom: bool,
    /// A touch drag or fling in progress.
    pub motion: Option<ScrollMotion>,
    /// A touch-mode box fading after its last touch; `Some(0)` once hidden.
    pub bar_fade: Option<f32>,
}

/// Focus history navigation keeps between frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FocusMemory {
    /// Focus container key → the control last focused inside it (`use_last_focus`).
    pub last: BTreeMap<String, String>,
    /// The control focus left that keeps its hover look (`reset_on_focus_lost: false`).
    pub held: Option<String>,
}

impl ViewState {
    pub fn is_hovered(&self, key: &str) -> bool {
        self.hovered.as_deref() == Some(key) || self.focus_memory.held.as_deref() == Some(key)
    }

    pub fn is_pressed(&self, key: &str) -> bool {
        self.pressed.as_deref() == Some(key)
    }

    pub fn is_focused(&self, key: &str) -> bool {
        self.focused.as_deref() == Some(key)
    }

    pub fn scroll_offset(&self, key: &str) -> f64 {
        self.scroll.get(key).copied().unwrap_or(0.0)
    }

    /// Whether `other` lays out the same: hover, press, focus and component
    /// writes only gate what draws or binds.
    pub fn same_layout(&self, other: &ViewState) -> bool {
        self.scroll == other.scroll
            && self.scroll_state == other.scroll_state
            && self.pointer == other.pointer
            && self.drags == other.drags
    }

    /// The parts of this state layout reads (see [`ViewState::same_layout`]).
    pub fn layout_part(&self) -> ViewState {
        ViewState {
            scroll: self.scroll.clone(),
            scroll_state: self.scroll_state.clone(),
            pointer: self.pointer,
            drags: self.drags.clone(),
            ..ViewState::default()
        }
    }

    /// Adopt what the last layout decided for scroll views that retain it: a
    /// jump to a new end and the latched `#scrollbar_hit_bottom`. `true` when
    /// anything changed, so the caller lays out again.
    pub fn settle(&mut self, report: &LayoutReport) -> bool {
        let mut changed = false;
        for (key, metrics) in &report.scrolls {
            let wants = metrics.jump_to_end || metrics.hit_bottom;
            if !wants && !self.scroll_state.contains_key(key) {
                continue;
            }
            let retained = self.scroll_state.entry(key.clone()).or_default();
            if metrics.jump_to_end {
                let max = metrics.max_offset();
                if retained.extent != Some(max) {
                    retained.extent = Some(max);
                    self.scroll.insert(key.clone(), metrics.offset);
                    changed = true;
                }
            }
            if metrics.hit_bottom && !retained.hit_bottom {
                retained.hit_bottom = true;
                changed = true;
            }
        }
        changed
    }
}

/// A scroll view's measured extents after layout, along its scrolling axis.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScrollMetrics {
    /// The clamped offset actually applied.
    pub offset: f64,
    pub content: f64,
    pub viewport: f64,
    /// The viewport's leading edge (virtual px), for scrolling a control into view.
    pub viewport_top: f64,
    /// The named viewport rect `[x, y, w, h]`, which takes the wheel.
    pub viewport_rect: Option<[f64; 4]>,
    /// The content rect before scrolling.
    pub content_rect: Option<[f64; 4]>,
    /// The named scrollbar track rect `[x, y, w, h]`.
    pub track: Option<[f64; 4]>,
    /// The drawn scrollbar box rect `[x, y, w, h]`, when it is shown.
    pub thumb: Option<[f64; 4]>,
    /// Pixels scrolled per wheel notch (`scroll_speed`).
    pub speed: f64,
    /// A horizontally draggable box scrolls along x.
    pub horizontal: bool,
    /// The box's `draggable`, which also sets whether it can be grabbed.
    pub box_drag: Draggable,
    pub gesture: bool,
    pub always_handle_scrolling: bool,
    pub touch_mode: bool,
    pub allow_scroll_when_fits: bool,
    pub jump_to_end: bool,
    pub track_button: Option<String>,
    pub touch_button: Option<String>,
    /// `#scroll_bar_visible`, once the view decided it.
    pub bar_visible: Option<bool>,
    /// `#scrollbar_hit_bottom`.
    pub hit_bottom: bool,
    /// `#scrolled_to_end`.
    pub scrolled_to_end: bool,
}

/// The client reads its wheel sensitivity once, from the first view scrolled.
static WHEEL_SENSITIVITY: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

impl ScrollMetrics {
    pub fn max_offset(&self) -> f64 {
        (self.content - self.viewport).max(0.0)
    }

    /// The offset that brings the span `[top, bottom)` (current coordinates)
    /// fully into the viewport, moving as little as possible.
    pub fn offset_revealing(&self, top: f64, bottom: f64) -> f64 {
        let view_bottom = self.viewport_top + self.viewport;
        let shift = if top < self.viewport_top {
            top - self.viewport_top
        } else if bottom > view_bottom {
            (bottom - view_bottom).min(top - self.viewport_top)
        } else {
            0.0
        };
        (self.offset + shift).clamp(0.0, self.max_offset())
    }

    fn along(&self, rect: [f64; 4]) -> (f64, f64) {
        if self.horizontal {
            (rect[0], rect[2])
        } else {
            (rect[1], rect[3])
        }
    }

    /// The offset after the pointer moves `delta` along the axis while holding the
    /// box: content pixels per track pixel.
    pub fn thumb_drag_target(&self, delta: f64) -> f64 {
        let Some(track) = self.track else {
            return self.offset;
        };
        let length = self.along(track).1;
        if length <= 0.0 {
            return self.offset;
        }
        (self.offset + delta * self.content / length).clamp(0.0, self.max_offset())
    }

    /// A press on the track at `point` centres the viewport on that fraction of
    /// the content (the `scrollbar_track_button` event).
    pub fn offset_for_track(&self, point: [f64; 2]) -> f64 {
        let Some(track) = self.track else {
            return self.offset;
        };
        // The fraction reads y only for a vertical box, x otherwise.
        let (start, length, at) = if self.box_drag == Draggable::Vertical {
            (track[1], track[3], point[1])
        } else {
            (track[0], track[2], point[0])
        };
        let fraction = if length > 0.5 {
            (at - start) / length
        } else {
            1.0
        };
        (self.viewport * -0.5 + fraction * self.content).clamp(0.0, self.max_offset())
    }

    /// The offset after `notches` wheel steps (positive scrolls toward the top):
    /// the first view scrolled fixes the sensitivity, and a step up is 120/127 of
    /// one down is 120/128, the client's mouse byte scaling. A horizontal view
    /// ignores the vertical wheel.
    pub fn offset_for_wheel(&self, notches: f64) -> f64 {
        if self.horizontal {
            return self.offset;
        }
        let sensitivity = *WHEEL_SENSITIVITY.get_or_init(|| self.speed);
        let byte = if notches > 0.0 {
            120.0 / 127.0
        } else {
            120.0 / 128.0
        };
        (self.offset - sensitivity * notches * byte).clamp(0.0, self.max_offset())
    }

    /// Whether the wheel at `point` reaches this view: over its viewport or
    /// track, or anywhere under `always_handle_scrolling`.
    pub fn takes_wheel(&self, point: [f64; 2]) -> bool {
        let inside = |rect: Option<[f64; 4]>| {
            rect.is_some_and(|r| {
                point[0] >= r[0]
                    && point[0] <= r[0] + r[2]
                    && point[1] >= r[1]
                    && point[1] <= r[1] + r[3]
            })
        };
        self.always_handle_scrolling || inside(self.viewport_rect) || inside(self.track)
    }

    /// The property-bag values this view publishes to `view` bindings.
    pub fn feedback(&self) -> BTreeMap<String, Scalar> {
        let mut values = BTreeMap::from([
            (
                "#scrollbar_hit_bottom".to_owned(),
                Scalar::Bool(self.hit_bottom),
            ),
            (
                "#scrolled_to_end".to_owned(),
                Scalar::Bool(self.scrolled_to_end),
            ),
        ]);
        if let Some(visible) = self.bar_visible {
            values.insert("#scroll_bar_visible".to_owned(), Scalar::Bool(visible));
        }
        values
    }
}

/// Side results gathered while laying out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayoutReport {
    pub scrolls: BTreeMap<String, ScrollMetrics>,
    /// Each control with a `clip_state_change_event`: the event and whether the
    /// control lies wholly outside its clip. A change between layouts fires it.
    pub clip_states: BTreeMap<String, (String, bool)>,
    /// Whether a laid-out control follows the pointer ([`ViewState::pointer`]).
    pub tracks_pointer: bool,
}

impl LayoutReport {
    /// Each scroll view's published values by control name, for the data
    /// source's `view` binding lookups ([`crate::DataSource::set_control_values`]).
    pub fn scroll_feedback(&self) -> BTreeMap<String, BTreeMap<String, Scalar>> {
        self.scrolls
            .iter()
            .map(|(key, metrics)| (control_name(key).to_owned(), metrics.feedback()))
            .collect()
    }
}

/// The instance name at the end of a layout key, less any `[index]`.
fn control_name(key: &str) -> &str {
    let last = key.rsplit('/').next().unwrap_or(key);
    last.split('[').next().unwrap_or(last)
}
