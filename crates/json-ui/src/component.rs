//! Retained widget components: the engine-owned state machines vanilla attaches
//! to buttons, toggles, sliders, edit boxes and dropdowns, and the dispatcher
//! that turns raw input into their screen events through each control's button
//! mappings. The caller keeps [`Components`] (what the controls write into their
//! property bags, which re-binding reads back) and a [`Dispatcher`] per screen.

use std::collections::BTreeMap;

use serde_json::Value;

use crate::layout::LaidOut;
use crate::tree::ResolvedControl;

mod dispatch;
mod edit;
mod selection_wheel;
mod slider;
mod sound;
mod toggle;

pub(crate) use dispatch::CARET_PROPERTY;
pub use dispatch::{ButtonInput, Dispatch, Dispatcher, PointerInput};
pub use edit::{CARET_BLINK_SECONDS, CARET_GLYPH, EditMeta, TextEdit, TextType};
pub use selection_wheel::SelectionWheelMeta;
pub(crate) use selection_wheel::visibility as wheel_visibility;
pub(crate) use slider::SELECTED_PROPERTY as SLIDER_BOX_SELECTED;
pub(crate) use slider::step_marks as slider_step_marks;
pub use slider::{SliderManager, SliderMeta};
pub use sound::SoundMeta;
pub use toggle::{ToggleManager, ToggleMeta};

/// What a control's components write into property bags, keyed by layout key.
/// Binding reads these over the screen's data, as vanilla's bag writes persist
/// until a controller answers the binding again.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Components {
    bags: BTreeMap<String, BTreeMap<String, Value>>,
    edits: BTreeMap<String, TextEdit>,
    /// The selected edit box or slider box, by control key.
    selected: Option<String>,
}

impl Components {
    /// The bag writes for the control at `key`: `#` names are bag values,
    /// others replace the control's own properties (a text target's `text`).
    pub fn bag(&self, key: &str) -> Option<&BTreeMap<String, Value>> {
        self.bags.get(key)
    }

    pub fn is_empty(&self) -> bool {
        self.bags.is_empty()
    }

    /// Whether binding sees the same property writes, regardless of editing timers.
    pub fn same_bindings(&self, other: &Self) -> bool {
        self.bags == other.bags
    }

    /// Retains a controller's snapped slider position for subsequent directional input.
    pub fn set_slider_value(&mut self, key: &str, value: f64) {
        self.write(key, "#slider_value", Value::from(value));
    }

    pub(crate) fn write(&mut self, key: &str, name: &str, value: Value) {
        self.bags
            .entry(key.to_owned())
            .or_default()
            .insert(name.to_owned(), value);
    }

    pub(crate) fn unwrite(&mut self, key: &str, name: &str) {
        if let Some(bag) = self.bags.get_mut(key) {
            bag.remove(name);
            if bag.is_empty() {
                self.bags.remove(key);
            }
        }
    }

    /// Drop every write for controls under `key` (itself included), as a
    /// rebuilt control starts from its bindings again.
    pub fn forget(&mut self, key: &str) {
        let under =
            |candidate: &String| candidate == key || candidate.starts_with(&format!("{key}/"));
        self.bags.retain(|candidate, _| !under(candidate));
        self.edits.retain(|candidate, _| !under(candidate));
        if self.selected.as_ref().is_some_and(under) {
            self.selected = None;
        }
    }

    /// The selected edit box or slider box, if any.
    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// The editing state of the edit box at `key`.
    pub fn edit(&self, key: &str) -> Option<&TextEdit> {
        self.edits.get(key)
    }

    pub(crate) fn edit_mut(&mut self, key: &str, text: &str) -> &mut TextEdit {
        self.edits
            .entry(key.to_owned())
            .or_insert_with(|| TextEdit::new(text))
    }

    pub(crate) fn set_selected(&mut self, key: Option<String>) {
        self.selected = key;
    }
}

/// The binder's seam for component bag writes: creation-time writes, then
/// what the components wrote since, both under the screen's bindings.
pub(crate) fn write_bag(
    control: &ResolvedControl,
    components: &Components,
    key: &str,
    own: &mut BTreeMap<String, crate::predicate::Scalar>,
) {
    use crate::predicate::Scalar;
    // A toggle's component names its grid collection in its bag at creation.
    if matches!(control.control_type.as_deref(), Some("toggle" | "dropdown")) {
        let name = text_of(control, "toggle_grid_collection_name").unwrap_or_default();
        own.insert(
            crate::bind::COLLECTION_NAME_KEY.to_owned(),
            Scalar::Text(name),
        );
    }
    let Some(bag) = components.bag(key) else {
        return;
    };
    for (name, value) in bag.iter().filter(|(name, _)| name.starts_with('#')) {
        let scalar = match value {
            Value::Bool(flag) => Scalar::Bool(*flag),
            Value::Number(number) => match number.as_f64() {
                Some(number) => Scalar::Num(number),
                None => continue,
            },
            Value::String(text) => Scalar::Text(text.clone()),
            _ => continue,
        };
        own.insert(name.clone(), scalar);
    }
}

/// Plain properties a component set on a control (a text target's `text`),
/// over its baked ones.
pub(crate) fn write_properties(
    components: &Components,
    key: &str,
    properties: &mut BTreeMap<String, Value>,
) {
    if let Some(bag) = components.bag(key) {
        for (name, value) in bag.iter().filter(|(name, _)| !name.starts_with('#')) {
            properties.insert(name.clone(), value.clone());
        }
    }
}

/// A screen event a component raised, for the screen's controller (the host).
#[derive(Clone, Debug, PartialEq)]
pub enum ScreenEvent {
    /// A mapped button event: `id` is the `to_button_id`.
    Button(ButtonEvent),
    /// A toggle changed (`ToggleChangeEventData`).
    Toggle {
        name: String,
        key: String,
        index: Option<usize>,
        checked: bool,
        by_click: bool,
    },
    /// A slider moved: `value` is the normalised position, `step` the step index.
    Slider {
        name: String,
        key: String,
        index: Option<usize>,
        value: f64,
        step: Option<usize>,
        finished: bool,
        /// Whether directional input produced this move.
        directional: bool,
    },
    /// An edit box's text changed or editing ended (`finished`).
    TextEdit {
        name: String,
        key: String,
        index: Option<usize>,
        text: String,
        finished: bool,
    },
    /// An edit box was selected or deselected.
    TextEditSelected { key: String, selected: bool },
    /// A sound component asked for a sound.
    Sound {
        name: String,
        volume: f32,
        pitch: f32,
    },
}

/// A mapped button event and where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct ButtonEvent {
    pub id: String,
    pub from: String,
    /// Layout key of the control whose mapping fired.
    pub key: String,
    pub collection_index: Option<usize>,
    pub collection: Option<String>,
    pub down: bool,
    /// Interaction edge: the press for pointer/gamepad, the release for touch.
    pub interacted: bool,
    pub scope: crate::input::MappingScope,
}

/// What a control's own components need at dispatch, read from its resolved
/// definition and laid-out subtree.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Widget {
    pub toggle: Option<ToggleMeta>,
    pub slider: Option<SliderMeta>,
    pub edit: Option<EditMeta>,
    pub selection_wheel: Option<SelectionWheelMeta>,
    pub toggle_manager: Option<ToggleManager>,
    pub slider_manager: Option<SliderManager>,
    pub sounds: Option<SoundMeta>,
    /// A button's `consume_event` (default true).
    pub consume: bool,
    /// `gesture_tracking_button`: the button whose hold turns pointer motion into gesture deltas.
    pub gesture: Option<String>,
}

impl Widget {
    pub(crate) fn read(node: &LaidOut) -> Self {
        let control = node.control;
        let kind = control.control_type.as_deref().unwrap_or("");
        Widget {
            toggle: matches!(kind, "toggle" | "dropdown").then(|| ToggleMeta::read(control)),
            slider: (kind == "slider").then(|| SliderMeta::read(control)),
            edit: (kind == "edit_box").then(|| EditMeta::read(node)),
            selection_wheel: (kind == "selection_wheel").then(|| SelectionWheelMeta::read(control)),
            toggle_manager: ToggleManager::read(control),
            slider_manager: SliderManager::read(control),
            sounds: SoundMeta::read(control, matches!(kind, "button" | "toggle" | "dropdown")),
            consume: crate::widgets::bound_bool(control, "consume_event").unwrap_or(true),
            gesture: text_of(control, "gesture_tracking_button"),
        }
    }
}

/// The first descendant of `node` named `name`, breadth first.
pub(crate) fn descendant<'n, 'a>(node: &'n LaidOut<'a>, name: &str) -> Option<&'n LaidOut<'a>> {
    let mut queue = std::collections::VecDeque::from([node]);
    while let Some(next) = queue.pop_front() {
        if next.control.name == name && !std::ptr::eq(next, node) {
            return Some(next);
        }
        queue.extend(next.children.iter());
    }
    None
}

pub(crate) fn text_of(control: &ResolvedControl, key: &str) -> Option<String> {
    control
        .properties
        .get(key)
        .and_then(Value::as_str)
        .filter(|text| !text.is_empty())
        .map(str::to_owned)
}
