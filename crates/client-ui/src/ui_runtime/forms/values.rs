//! Live state for a form drawn by the JSON-UI engine: the caller-held view state
//! (hover, press, focus, scroll) plus each custom-form element's current value,
//! seeded from the decoded defaults and edited by input until submission.

use std::sync::Arc;

use json_ui::{HitRegion, LayoutReport, ViewState};
use protocol::{CustomFormElement, CustomFormValue, ServerFormModel};
use ui::UiPoint;

use super::ServerFormIdentity;

/// What the last engine-drawn frame exposes to input: regions and scroll extents
/// in virtual pixels, plus the virtual → window-logical mapping.
#[derive(Clone, Debug)]
pub struct EngineFrame {
    /// The form drawn, or `None` for a container screen.
    pub identity: Option<ServerFormIdentity>,
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

/// One custom-form element's value; decorations hold `None` so indexes align.
#[derive(Debug, Clone, PartialEq)]
pub enum FormValue {
    None,
    Toggle(bool),
    /// The slider's actual value (already snapped to its step).
    Slider(f64),
    Step(i32),
    Dropdown(i32),
    Text(String),
    MultiSelect(Vec<i32>),
}

/// A pointer drag the engine path is tracking.
#[derive(Debug, Clone, PartialEq)]
pub enum FormDrag {
    /// A scrollbar box: the scroll view key and the pointer's last position along its axis.
    ScrollBox { view: String, last: f64 },
    /// A `draggable` control: its key and the pointer's last position.
    Control { key: String, last: [f64; 2] },
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct FormEngineState {
    pub view: ViewState,
    pub values: Vec<FormValue>,
    /// Expanded dropdowns retain their own option lists.
    pub open_dropdowns: std::collections::BTreeSet<usize>,
    /// Expanded multiselects retain independent option lists.
    pub open_multiselects: std::collections::BTreeSet<usize>,
    /// Sliders whose last change set a track position.
    pub position_sliders: std::collections::BTreeSet<usize>,
    pub drag: Option<FormDrag>,
    /// A touch pan on this scroll view key, with the pointer's last position.
    pub scroll_touch: Option<(String, [f64; 2])>,
    /// When scroll dynamics last stepped.
    pub scroll_clock: Option<std::time::Instant>,
    /// The template's input components (mappings, hover, slider tracks).
    pub dispatcher: json_ui::Dispatcher,
    /// App-clock seconds at the last input frame, for caret blinks.
    pub clock: f64,
}

impl FormEngineState {
    /// Drops transient input without changing the retained form or its draft.
    pub fn cancel_pointer_input(&mut self) {
        self.view.hovered = None;
        self.view.pressed = None;
        self.view.pointer = None;
        self.drag = None;
        self.scroll_touch = None;
        self.scroll_clock = None;
        for scroll in self.view.scroll_state.values_mut() {
            scroll.motion = None;
        }
        // The dispatcher also retains slider tracks, gestures and button edges.
        // Reset them without dispatching a release that could submit the form.
        self.dispatcher = json_ui::Dispatcher::default();
    }

    pub fn for_model(model: &ServerFormModel) -> Self {
        let values = match model {
            ServerFormModel::Custom(form) => form.elements.iter().map(initial_value).collect(),
            _ => Vec::new(),
        };
        Self {
            values,
            ..Self::default()
        }
    }

    /// The response array, one entry per element in wire order.
    pub fn submission(&self) -> Arc<[CustomFormValue]> {
        self.values
            .iter()
            .map(|value| match value {
                FormValue::None => CustomFormValue::Null,
                FormValue::Toggle(on) => CustomFormValue::Toggle(*on),
                FormValue::Slider(value) => CustomFormValue::Slider(*value),
                FormValue::Step(index) => CustomFormValue::Step(*index),
                FormValue::Dropdown(index) => CustomFormValue::Dropdown(*index),
                FormValue::MultiSelect(indexes) => {
                    CustomFormValue::MultiSelect(indexes.clone().into())
                }
                FormValue::Text(text) => CustomFormValue::Input(text.clone()),
            })
            .collect()
    }
}

fn initial_value(element: &CustomFormElement) -> FormValue {
    match element {
        CustomFormElement::Label { .. }
        | CustomFormElement::Header { .. }
        | CustomFormElement::Divider => FormValue::None,
        CustomFormElement::Toggle { default, .. } => FormValue::Toggle(*default),
        CustomFormElement::Slider { default, .. } => FormValue::Slider(default.get()),
        CustomFormElement::StepSlider { default, .. } => FormValue::Step(*default),
        CustomFormElement::Dropdown { default, .. } => FormValue::Dropdown(*default),
        CustomFormElement::MultiSelect { default, .. } => FormValue::MultiSelect(default.to_vec()),
        CustomFormElement::Input { default, .. } => FormValue::Text(default.to_string()),
    }
}

/// Returns the float track position, or zero for a collapsed range.
pub(crate) fn slider_fraction(min: f64, max: f64, current: f64) -> f64 {
    let (min, max, current) = (min as f32, max as f32, current as f32);
    if max > min {
        f64::from((current - min) / (max - min))
    } else {
        0.0
    }
}

/// Snaps a drag position with float arithmetic, retaining the result as a JSON number.
pub fn slider_value_at(min: f64, max: f64, step: f64, fraction: f64) -> f64 {
    f64::from(slider_grid_value(
        min as f32,
        max as f32,
        step as f32,
        fraction as f32,
        0.0,
    ))
}

/// Advances an unchanged snapped value by one step when directional input moves.
pub(super) fn slider_value_after_step(
    min: f64,
    max: f64,
    step: f64,
    fraction: f64,
    current: f64,
) -> f64 {
    const CHANGE_THRESHOLD: f32 = 1e-5;
    let (min, max, step, fraction, current) = (
        min as f32,
        max as f32,
        step as f32,
        fraction as f32,
        current as f32,
    );
    let snapped = slider_grid_value(min, max, step, fraction, 0.0);
    let delta = fraction - (current - min) / (max - min);
    let nudge = if delta.abs() > CHANGE_THRESHOLD && (snapped - current).abs() <= CHANGE_THRESHOLD {
        if delta > 0.0 { 1.0 } else { -1.0 }
    } else {
        0.0
    };
    f64::from(slider_grid_value(min, max, step, fraction, nudge))
}

/// Applies a step offset to the rounded float grid and clamps to the authored range.
fn slider_grid_value(min: f32, max: f32, step: f32, fraction: f32, nudge: f32) -> f32 {
    let span = (max - min).max(0.0);
    let fraction = fraction.clamp(0.0, 1.0);
    if step <= 0.0 || span == 0.0 {
        return (min + span * fraction).clamp(min, max);
    }
    (step * ((span * fraction / step).round() + nudge) + min).clamp(min, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_drags_snap_to_the_step_grid() {
        assert_eq!(slider_value_at(0.0, 10.0, 2.0, 0.34), 4.0);
        assert_eq!(slider_value_at(0.0, 10.0, 2.0, 1.0), 10.0);
        assert_eq!(slider_value_at(5.0, 5.0, 1.0, 0.7), 5.0);
    }

    #[test]
    fn submission_keeps_decorations_as_nulls_in_order() {
        let state = FormEngineState {
            values: vec![
                FormValue::None,
                FormValue::Toggle(true),
                FormValue::Text("hi".into()),
            ],
            ..FormEngineState::default()
        };
        assert_eq!(
            state.submission().as_ref(),
            [
                CustomFormValue::Null,
                CustomFormValue::Toggle(true),
                CustomFormValue::Input("hi".into())
            ]
        );
    }

    #[test]
    fn pointer_cancellation_preserves_form_values_focus_and_scroll_position() {
        let mut state = FormEngineState {
            values: vec![FormValue::Text("unsent draft".into())],
            open_dropdowns: [0].into_iter().collect(),
            ..Default::default()
        };
        state.view.focused = Some("editor".into());
        state.view.hovered = Some("slider".into());
        state.view.pressed = Some("slider".into());
        state.view.pointer = Some([12.0, 13.0]);
        state.view.scroll.insert("list".into(), 25.0);
        state.drag = Some(FormDrag::Control {
            key: "slider".into(),
            last: [12.0, 13.0],
        });
        state.scroll_touch = Some(("list".into(), [12.0, 13.0]));
        state.cancel_pointer_input();
        assert_eq!(state.values, vec![FormValue::Text("unsent draft".into())]);
        assert!(state.open_dropdowns.contains(&0));
        assert_eq!(state.view.focused.as_deref(), Some("editor"));
        assert_eq!(state.view.scroll.get("list"), Some(&25.0));
        assert!(state.drag.is_none() && state.scroll_touch.is_none());
        assert!(state.view.pressed.is_none() && state.view.hovered.is_none());
        assert!(state.view.pointer.is_none());
        assert_eq!(state.dispatcher, json_ui::Dispatcher::default());
    }
}

#[cfg(test)]
mod float_precision_tests {
    use super::slider_value_at;

    #[test]
    fn slider_responses_use_native_float_precision() {
        assert_eq!(slider_value_at(0.0, 1.0, 0.1, 0.3), f64::from(0.3_f32));
    }
}
