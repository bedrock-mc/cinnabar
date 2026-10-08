//! The `"custom_form"` family: ordered input elements plus an optional submit
//! label. Each element's initial value follows the vanilla client: toggles start
//! false, sliders at `min` (with `step` defaulting to 1 and `max` raised to `min`),
//! step sliders and dropdowns at index 0, inputs empty — and a non-null `default`
//! overrides the start. Anything outside that shape is unsupported rather than
//! guessed, since a wrong element count would misalign the response array.

use std::sync::Arc;

use serde_json::{Map, Value};

use super::{
    MAX_CUSTOM_FORM_ITEMS, ServerFormModel, UnsupportedForm, optional_text, required_text_value,
};

/// A finite form number compared by bit pattern so the model stays `Eq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormNumber(u64);

impl FormNumber {
    /// `None` for NaN or infinity, which no slider can represent.
    pub fn new(value: f64) -> Option<Self> {
        value.is_finite().then(|| Self(value.to_bits()))
    }

    pub fn get(self) -> f64 {
        f64::from_bits(self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CustomForm {
    pub title: Arc<str>,
    pub elements: Arc<[CustomFormElement]>,
    /// The `submit` label; `None` keeps the template's own submit text.
    pub submit: Option<Arc<str>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomFormElement {
    Label {
        text: Arc<str>,
    },
    Header {
        text: Arc<str>,
    },
    Divider,
    Toggle {
        text: Arc<str>,
        default: bool,
        tooltip: Option<Arc<str>>,
    },
    Slider {
        text: Arc<str>,
        min: FormNumber,
        max: FormNumber,
        step: FormNumber,
        default: FormNumber,
        tooltip: Option<Arc<str>>,
    },
    StepSlider {
        text: Arc<str>,
        steps: Arc<[Arc<str>]>,
        default: u32,
        tooltip: Option<Arc<str>>,
    },
    Dropdown {
        text: Arc<str>,
        options: Arc<[Arc<str>]>,
        default: u32,
        tooltip: Option<Arc<str>>,
    },
    Input {
        text: Arc<str>,
        placeholder: Arc<str>,
        default: Arc<str>,
        tooltip: Option<Arc<str>>,
    },
}

pub(super) fn custom_model(object: &Map<String, Value>) -> ServerFormModel {
    match parse(object) {
        Ok(form) => ServerFormModel::Custom(form),
        Err(reason) => ServerFormModel::Unsupported(reason),
    }
}

fn parse(object: &Map<String, Value>) -> Result<CustomForm, UnsupportedForm> {
    let title = optional_text(object, "title")?;
    let content = object
        .get("content")
        .and_then(Value::as_array)
        .ok_or(UnsupportedForm::Controls)?;
    if content.len() > MAX_CUSTOM_FORM_ITEMS {
        return Err(UnsupportedForm::Limit);
    }
    let submit = match object.get("submit") {
        None | Some(Value::Null) => None,
        Some(value) => Some(Arc::from(required_text_value(value)?)),
    };
    let elements = content.iter().map(element).collect::<Result<Vec<_>, _>>()?;
    Ok(CustomForm {
        title: Arc::from(title),
        elements: elements.into(),
        submit,
    })
}

fn element(value: &Value) -> Result<CustomFormElement, UnsupportedForm> {
    let object = value.as_object().ok_or(UnsupportedForm::Controls)?;
    let kind = object
        .get("type")
        .and_then(Value::as_str)
        .ok_or(UnsupportedForm::Controls)?;
    if kind == "divider" {
        return Ok(CustomFormElement::Divider);
    }
    let text: Arc<str> = match object.get("text") {
        Some(value) => Arc::from(required_text_value(value)?),
        None => return Err(UnsupportedForm::Controls),
    };
    let tooltip = match object.get("tooltip") {
        None | Some(Value::Null) => None,
        Some(value) => Some(Arc::from(required_text_value(value)?)),
    };
    let default = object.get("default").filter(|value| !value.is_null());
    Ok(match kind {
        "label" => CustomFormElement::Label { text },
        "header" => CustomFormElement::Header { text },
        "toggle" => CustomFormElement::Toggle {
            text,
            // A well-formed but non-boolean default is skipped, not fatal.
            default: default.and_then(Value::as_bool).unwrap_or(false),
            tooltip,
        },
        "slider" => {
            let number = |key: &str| object.get(key).and_then(Value::as_f64).unwrap_or(0.0);
            let min = number("min");
            let max = number("max").max(min);
            let step = object
                .get("step")
                .and_then(Value::as_f64)
                .filter(|step| *step > 0.0)
                .unwrap_or(1.0);
            let start = default
                .and_then(Value::as_f64)
                .unwrap_or(min)
                .clamp(min, max);
            let finite = |value: f64| FormNumber::new(value).ok_or(UnsupportedForm::Controls);
            CustomFormElement::Slider {
                text,
                min: finite(min)?,
                max: finite(max)?,
                step: finite(step)?,
                default: finite(start)?,
                tooltip,
            }
        }
        "step_slider" => {
            let steps = string_list(object.get("steps"))?;
            CustomFormElement::StepSlider {
                text,
                default: index_default(default, steps.len()),
                steps,
                tooltip,
            }
        }
        "dropdown" => {
            let options = string_list(object.get("options"))?;
            CustomFormElement::Dropdown {
                text,
                default: index_default(default, options.len()),
                options,
                tooltip,
            }
        }
        "input" => CustomFormElement::Input {
            text,
            placeholder: Arc::from(optional_text(object, "placeholder")?),
            default: Arc::from(match default {
                Some(value) => required_text_value(value)?,
                None => "",
            }),
            tooltip,
        },
        _ => return Err(UnsupportedForm::Controls),
    })
}

fn string_list(value: Option<&Value>) -> Result<Arc<[Arc<str>]>, UnsupportedForm> {
    let items = value
        .and_then(Value::as_array)
        .ok_or(UnsupportedForm::Controls)?;
    if items.len() > MAX_CUSTOM_FORM_ITEMS {
        return Err(UnsupportedForm::Limit);
    }
    items
        .iter()
        .map(|item| required_text_value(item).map(Arc::<str>::from))
        .collect()
}

/// An integer default inside `0..len`, else index 0 (the vanilla start).
fn index_default(default: Option<&Value>, len: usize) -> u32 {
    default
        .and_then(Value::as_u64)
        .filter(|index| (*index as usize) < len)
        .map_or(0, |index| index as u32)
}
