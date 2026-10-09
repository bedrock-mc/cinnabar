//! The `"custom_form"` family: ordered input elements plus an optional submit
//! label. Each element's initial value follows the vanilla client: toggles start
//! false, sliders at `min` (with `step` defaulting to 1 and `max` raised to `min`),
//! step sliders and dropdowns at index 0, inputs empty — and a non-null `default`
//! overrides the start. Anything outside that shape is unsupported rather than
//! guessed, since a wrong element count would misalign the response array.

use std::sync::Arc;

use serde_json::{Map, Value};

use super::{
    FormButtonImage, FormText, MAX_CUSTOM_FORM_ITEMS, ServerFormModel, UnsupportedForm,
    button_image, literal_value, optional_text, text_value,
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
    pub title: FormText,
    /// Optional server-settings icon, shared with form image loading.
    pub icon: Option<FormButtonImage>,
    pub elements: Arc<[CustomFormElement]>,
    /// The `submit` label; `None` keeps the template's own submit text.
    pub submit: Option<FormText>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CustomFormElement {
    Label {
        text: FormText,
    },
    Header {
        text: FormText,
    },
    Divider,
    Toggle {
        text: FormText,
        default: bool,
        tooltip: Option<FormText>,
    },
    Slider {
        text: FormText,
        min: FormNumber,
        max: FormNumber,
        step: FormNumber,
        default: FormNumber,
        /// Slider dispatch delay in seconds.
        timeout: FormNumber,
        tooltip: Option<FormText>,
    },
    StepSlider {
        text: FormText,
        steps: Arc<[FormText]>,
        default: i32,
        tooltip: Option<FormText>,
    },
    Dropdown {
        text: FormText,
        options: Arc<[FormText]>,
        default: i32,
        tooltip: Option<FormText>,
    },
    MultiSelect {
        text: FormText,
        options: Arc<[FormText]>,
        default: Arc<[i32]>,
        tooltip: Option<FormText>,
    },
    Input {
        text: FormText,
        placeholder: FormText,
        default: Arc<str>,
        tooltip: Option<FormText>,
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
        Some(value) => Some(text_value(value)?),
    };
    let elements = content.iter().map(element).collect::<Result<Vec<_>, _>>()?;
    Ok(CustomForm {
        title,
        icon: button_image(object.get("icon"))?,
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
    let text = optional_text(object, "text")?;
    let tooltip = match object.get("tooltip") {
        None | Some(Value::Null) => None,
        Some(value) => Some(text_value(value)?),
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
            let start = default.and_then(Value::as_f64).unwrap_or(min);
            let finite = |value: f64| FormNumber::new(value).ok_or(UnsupportedForm::Controls);
            CustomFormElement::Slider {
                text,
                min: finite(min)?,
                max: finite(max)?,
                step: finite(step)?,
                default: finite(start)?,
                timeout: finite(f64::from(number("timeout") as f32 / 1000.0_f32))?,
                tooltip,
            }
        }
        "step_slider" => {
            let steps = string_list(object.get("steps"))?;
            CustomFormElement::StepSlider {
                text,
                default: index_default(default),
                steps,
                tooltip,
            }
        }
        "dropdown" => {
            let options = string_list(object.get("options"))?;
            CustomFormElement::Dropdown {
                text,
                default: index_default(default),
                options,
                tooltip,
            }
        }
        "multiselect" => CustomFormElement::MultiSelect {
            text,
            options: string_list(object.get("options"))?,
            default: default
                .and_then(Value::as_array)
                .map(|indexes| {
                    indexes
                        .iter()
                        .filter_map(|value| {
                            value.as_i64().and_then(|index| i32::try_from(index).ok())
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
                .into(),
            tooltip,
        },
        "input" => CustomFormElement::Input {
            text,
            placeholder: optional_text(object, "placeholder")?,
            default: match default {
                Some(value) => literal_value(value)?,
                None => Arc::from(""),
            },
            tooltip,
        },
        _ => return Err(UnsupportedForm::Controls),
    })
}

/// Parses a bounded option list without converting display documents to JSON labels.
fn string_list(value: Option<&Value>) -> Result<Arc<[FormText]>, UnsupportedForm> {
    let items = value
        .and_then(Value::as_array)
        .ok_or(UnsupportedForm::Controls)?;
    if items.len() > MAX_CUSTOM_FORM_ITEMS {
        return Err(UnsupportedForm::Limit);
    }
    items.iter().map(text_value).collect()
}

/// Retains an integer default even when no option currently has that index.
fn index_default(default: Option<&Value>) -> i32 {
    default
        .and_then(Value::as_i64)
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or(0)
}
