//! The slider component (`SliderComponent`) as the 1.26.50 factory reads it,
//! and its value arithmetic (`_updateSliderFromPosition`,
//! `_updateSliderFromStepSize`).

use std::collections::BTreeMap;

use serde_json::Value;

use super::text_of;
use crate::bind::FactoryItem;
use crate::predicate::Scalar;
use crate::tree::ResolvedControl;
use crate::widgets::{bound_bool, bound_number};

/// Bag property holding whether the slider box is selected (its indent state).
pub(crate) const SELECTED_PROPERTY: &str = "#slider_box_selected";
/// Default `slider_timeout` of a step slider whose timeout is not numeric.
const STEP_SLIDER_TIMEOUT: f64 = 0.25;
/// `slider_speed` is in percent of the track per step.
const SPEED_SCALE: f64 = 100.0;

/// What a slider's component reads at creation.
#[derive(Clone, Debug, PartialEq)]
pub struct SliderMeta {
    pub name: Option<String>,
    /// `slider_steps`; more than one makes a step slider.
    pub steps: i64,
    pub vertical: bool,
    pub inverted: bool,
    pub speed: f64,
    /// Seconds between repeated directional steps, when set.
    pub timeout: Option<f64>,
    pub collection: Option<String>,
    pub select_on_hover: bool,
    pub track_button: Option<String>,
    pub small_decrease_button: Option<String>,
    pub small_increase_button: Option<String>,
    pub selected_button: Option<String>,
    pub deselected_button: Option<String>,
    pub tts_value_changed: Option<String>,
    /// The laid-out `#slider_value`: a step index or a `0..=1` percentage.
    pub value: f64,
}

impl SliderMeta {
    pub(crate) fn read(control: &ResolvedControl) -> Self {
        let steps = bound_number(control, "#slider_steps")
            .or_else(|| bound_number(control, "slider_steps"))
            .filter(|steps| steps.fract() == 0.0)
            .map_or(1, |steps| steps as i64);
        let timeout_key = control
            .properties
            .get("#slider_timeout")
            .or_else(|| control.properties.get("slider_timeout"));
        let timeout = timeout_key.filter(|value| !value.is_null()).map(|value| {
            value
                .as_f64()
                .unwrap_or(if steps > 1 { STEP_SLIDER_TIMEOUT } else { 0.0 })
        });
        SliderMeta {
            name: text_of(control, "slider_name"),
            steps,
            vertical: control
                .properties
                .get("slider_direction")
                .and_then(|v| v.as_str())
                == Some("vertical"),
            inverted: bound_bool(control, "slider_inverted").unwrap_or(false),
            speed: bound_number(control, "slider_speed").unwrap_or(1.0),
            timeout,
            collection: text_of(control, "slider_collection_name"),
            select_on_hover: bound_bool(control, "slider_select_on_hover").unwrap_or(false),
            track_button: text_of(control, "slider_track_button"),
            small_decrease_button: text_of(control, "slider_small_decrease_button"),
            small_increase_button: text_of(control, "slider_small_increase_button"),
            selected_button: text_of(control, "slider_selected_button"),
            deselected_button: text_of(control, "slider_deselected_button"),
            tts_value_changed: text_of(control, "tts_value_changed"),
            value: bound_number(control, "#slider_value").unwrap_or(0.0),
        }
    }

    pub fn is_step(&self) -> bool {
        self.steps > 1
    }

    /// The `#slider_value` and step index the pointer at `point` selects on a
    /// slider occupying `rect` (`[x, y, w, h]`).
    pub fn value_at(&self, rect: [f64; 4], point: [f64; 2]) -> (f64, Option<usize>) {
        let (start, length, at) = if self.vertical {
            (rect[1], rect[3], point[1])
        } else {
            (rect[0], rect[2], point[0])
        };
        let mut fraction = if length > 0.0 {
            ((at - start).clamp(0.0, length) / length) as f32
        } else {
            0.0
        };
        if self.inverted {
            fraction = 1.0 - fraction;
        }
        self.settle(f64::from(fraction.clamp(0.0, 1.0)))
    }

    /// The value after `direction` small steps (`_updateSliderFromStepSize`).
    pub fn stepped(&self, current: f64, direction: i32) -> (f64, Option<usize>) {
        let direction = if self.inverted { -direction } else { direction };
        if self.is_step() {
            let last = self.steps - 1;
            let index = (current as i64 + i64::from(direction)).clamp(0, last);
            return (index as f64, Some(index as usize));
        }
        let value = (f64::from(direction) * self.speed / SPEED_SCALE + current).clamp(0.0, 1.0);
        (value, None)
    }

    /// A `0..=1` fraction as the bag value: a step slider rounds to its step.
    fn settle(&self, fraction: f64) -> (f64, Option<usize>) {
        if !self.is_step() {
            return (fraction, None);
        }
        let index = (fraction as f32 * (self.steps - 1) as f32 + 0.5).floor() as i64;
        let index = index.clamp(0, self.steps - 1);
        (index as f64, Some(index as usize))
    }
}

/// `slider_manager_behavior: "gather"` over `slider_manage_groups`: on a
/// button release it republishes every managed slider's value.
#[derive(Clone, Debug, PartialEq)]
pub struct SliderManager {
    pub groups: Vec<String>,
}

impl SliderManager {
    pub(crate) fn read(control: &ResolvedControl) -> Option<Self> {
        // Gather is the only behavior the factory accepts.
        control.properties.get("slider_manager_behavior")?;
        let groups = match control.properties.get("slider_manage_groups") {
            Some(Value::Array(items)) => items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        Some(SliderManager { groups })
    }

    pub(crate) fn manages(&self, meta: &SliderMeta) -> bool {
        meta.name
            .as_ref()
            .is_some_and(|name| self.groups.contains(name))
    }
}

/// The step marks a step slider's component creates through its factory
/// (`SliderComponent::_createSteps`): one per inner step, spaced across the
/// slider, the ones past the current step drawn as progress.
pub(crate) fn step_marks(
    control: &ResolvedControl,
    own: &BTreeMap<String, Scalar>,
) -> Option<Vec<FactoryItem>> {
    if control.control_type.as_deref() != Some("slider") || control.factory.is_none() {
        return None;
    }
    let number = |key: &str| match own.get(key) {
        Some(Scalar::Num(value)) => Some(*value),
        _ => control.properties.get(key).and_then(Value::as_f64),
    };
    let steps = number("#slider_steps")
        .or_else(|| {
            control
                .properties
                .get("slider_steps")
                .and_then(Value::as_f64)
        })
        .unwrap_or(1.0) as i64;
    if steps <= 2 || steps - 2 > crate::bind::feed::MAX_FACTORY_ITEMS as i64 {
        return None;
    }
    let current = number("#slider_value").unwrap_or(0.0) as i64;
    let items = (1..steps - 1)
        .map(|step| {
            let id = if current < step {
                "slider_step_progress"
            } else {
                "slider_step"
            };
            // `-w/2 + step·w/(steps-1)` from the slider's centre, as a share of its width.
            let share = (step as f64 / (steps - 1) as f64 - 0.5) * 100.0;
            FactoryItem::new(id, 0.0)
                .var("step_offset", serde_json::json!([format!("{share}%"), 0]))
        })
        .collect();
    Some(items)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slider(steps: i64, vertical: bool, inverted: bool) -> SliderMeta {
        SliderMeta {
            name: Some("value".into()),
            steps,
            vertical,
            inverted,
            speed: 1.0,
            timeout: None,
            collection: None,
            select_on_hover: false,
            track_button: None,
            small_decrease_button: None,
            small_increase_button: None,
            selected_button: None,
            deselected_button: None,
            tts_value_changed: None,
            value: 0.0,
        }
    }

    // A five-step slider quantizes the pointer to its nearest step.
    #[test]
    fn step_sliders_round_to_a_step() {
        let meta = slider(5, false, false);
        assert_eq!(
            meta.value_at([0.0, 0.0, 100.0, 20.0], [60.0, 5.0]),
            (2.0, Some(2))
        );
        assert_eq!(
            meta.value_at([0.0, 0.0, 100.0, 20.0], [70.0, 5.0]),
            (3.0, Some(3))
        );
        assert_eq!(meta.stepped(4.0, 1), (4.0, Some(4)));
    }

    // Vertical sliders travel along Y and inversion flips the fraction.
    #[test]
    fn vertical_inverted_sliders_read_the_y_axis_flipped() {
        let meta = slider(1, true, true);
        let (value, _) = meta.value_at([0.0, 0.0, 20.0, 100.0], [5.0, 25.0]);
        assert!((value - 0.75).abs() < 1e-6);
        assert!((meta.stepped(0.5, 1).0 - 0.49).abs() < 1e-9);
    }
    #[test]
    fn review_step_mark_factory_checks_the_budget_before_building_items() {
        let control = ResolvedControl {
            name: "slider".into(),
            control_type: Some("slider".into()),
            base: None,
            unresolved_base: None,
            properties: [("slider_steps".into(), serde_json::json!(5000))]
                .into_iter()
                .collect::<BTreeMap<_, _>>()
                .into(),
            children: Vec::new(),
            factory: Some(crate::tree::Factory::default()),
        };
        assert!(step_marks(&control, &BTreeMap::new()).is_none());
    }
}
