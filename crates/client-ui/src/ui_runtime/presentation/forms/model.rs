//! Maps a decoded server form plus its live values onto the engine's form model.
//! Slider labels use the localized format after resolving their current value.

use std::sync::Arc;

use json_ui::{
    ActionElement, ActionForm, ButtonImage, CustomElement, CustomForm, FormButton, FormModel,
    ModalForm,
};
use protocol::{CustomFormElement, FormButtonImage, MenuElement, ServerFormModel};

use crate::ui_runtime::forms::{FormEngineState, FormValue, values::slider_fraction};
use launcher_host::remote_images::RemoteState;

/// `None` for a form the engine cannot draw (unsupported controls).
pub(super) fn engine_model(
    model: &ServerFormModel,
    state: &FormEngineState,
    translate: &dyn Fn(&str) -> Option<Arc<str>>,
    remote: &dyn Fn(&str) -> RemoteState,
    resolve: &dyn Fn(&protocol::FormText) -> String,
) -> Option<FormModel> {
    Some(match model {
        ServerFormModel::TextMenu(menu) => FormModel::Action(ActionForm {
            title: resolve(&menu.title),
            body: resolve(&menu.content),
            elements: menu
                .buttons
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    ActionElement::Button(FormButton {
                        text: resolve(text),
                        image: menu
                            .button_images
                            .get(index)
                            .and_then(Option::as_ref)
                            .and_then(|image| engine_image(image, remote)),
                    })
                })
                .collect(),
        }),
        ServerFormModel::ElementMenu(menu) => FormModel::Action(ActionForm {
            title: resolve(&menu.title),
            body: resolve(&menu.content),
            elements: menu
                .elements
                .iter()
                .map(|element| match element {
                    MenuElement::Button { text, image } => ActionElement::Button(FormButton {
                        text: resolve(text),
                        image: image.as_ref().and_then(|image| engine_image(image, remote)),
                    }),
                    MenuElement::Label(text) => ActionElement::Label(resolve(text)),
                    MenuElement::Header(text) => ActionElement::Header(resolve(text)),
                    MenuElement::Divider => ActionElement::Divider,
                })
                .collect(),
        }),
        ServerFormModel::Modal(modal) => FormModel::Modal(ModalForm {
            title: resolve(&modal.title),
            body: resolve(&modal.content),
            button1: resolve(&modal.button1),
            button2: resolve(&modal.button2),
        }),
        ServerFormModel::Custom(form) => {
            let slider_format =
                translate("options.sliderLabelFormat").unwrap_or_else(|| Arc::from("%s: %s"));
            FormModel::Custom(CustomForm {
                title: resolve(&form.title),
                icon: form
                    .icon
                    .as_ref()
                    .and_then(|image| engine_image(image, remote)),
                elements: form
                    .elements
                    .iter()
                    .enumerate()
                    .map(|(index, element)| {
                        custom_element(index, element, state, resolve, &slider_format)
                    })
                    .collect(),
                submit_text: match &form.submit {
                    Some(text) => resolve(text),
                    None => translate("gui.submit")
                        .map_or_else(|| "Submit".to_owned(), |text| text.to_string()),
                },
                submit_visible: true,
            })
        }
        // NPC dialogue draws through its own screen, not a form template.
        ServerFormModel::NpcDialogue(_) | ServerFormModel::Unsupported(_) => return None,
    })
}

/// Maps package paths and the current remote image state to template bindings.
fn engine_image(
    image: &FormButtonImage,
    remote: &dyn Fn(&str) -> RemoteState,
) -> Option<ButtonImage> {
    match image {
        FormButtonImage::Path(path) => Some(ButtonImage::Path(path.to_string())),
        FormButtonImage::Url(url) => match remote(url) {
            RemoteState::Ready(_) => Some(ButtonImage::Url(url.to_string())),
            RemoteState::Loading => Some(ButtonImage::Loading),
            RemoteState::Failed => None,
        },
    }
}

/// Resolves one custom control with its retained value and localized slider label.
fn custom_element(
    index: usize,
    element: &CustomFormElement,
    state: &FormEngineState,
    resolve: &dyn Fn(&protocol::FormText) -> String,
    slider_format: &str,
) -> CustomElement {
    let value = state.values.get(index);
    let tooltip =
        |tooltip: &Option<protocol::FormText>| tooltip.as_ref().map_or_else(String::new, resolve);
    match element {
        CustomFormElement::Label { text } => CustomElement::Label {
            text: resolve(text),
        },
        CustomFormElement::Header { text } => CustomElement::Header {
            text: resolve(text),
        },
        CustomFormElement::Divider => CustomElement::Divider,
        CustomFormElement::Toggle {
            text,
            default,
            tooltip: tip,
        } => CustomElement::Toggle {
            text: resolve(text),
            on: match value {
                Some(FormValue::Toggle(on)) => *on,
                _ => *default,
            },
            tooltip: tooltip(tip),
        },
        CustomFormElement::Slider {
            text,
            min,
            max,
            default,
            tooltip: tip,
            timeout,
            step,
        } => {
            let current = match value {
                Some(FormValue::Slider(current)) => *current,
                _ => default.get(),
            };
            CustomElement::Slider {
                text: protocol::format_translation(
                    slider_format,
                    &[resolve(text), number_text(current, step.get())],
                ),
                fraction: slider_fraction(min.get(), max.get(), current),
                tooltip: tooltip(tip),
                timeout: timeout.get(),
            }
        }
        CustomFormElement::StepSlider {
            text,
            steps,
            default,
            tooltip: tip,
        } => {
            let index = match value {
                Some(FormValue::Step(index)) => *index,
                _ => *default,
            };
            CustomElement::StepSlider {
                text: protocol::format_translation(
                    slider_format,
                    &[
                        resolve(text),
                        steps.get(index as usize).map_or_else(String::new, resolve),
                    ],
                ),
                steps: steps.len(),
                index,
                tooltip: tooltip(tip),
            }
        }
        CustomFormElement::Dropdown {
            text,
            options,
            default,
            tooltip: tip,
        } => CustomElement::Dropdown {
            text: resolve(text),
            options: options.iter().map(resolve).collect(),
            index: match value {
                Some(FormValue::Dropdown(index)) => *index,
                _ => *default,
            },
            open: state.open_dropdowns.contains(&index),
            tooltip: tooltip(tip),
        },
        CustomFormElement::MultiSelect {
            text,
            options,
            default,
            tooltip: tip,
        } => CustomElement::MultiSelect {
            text: resolve(text),
            options: options.iter().map(resolve).collect(),
            selected: match value {
                Some(FormValue::MultiSelect(indexes)) => indexes.clone(),
                _ => default.to_vec(),
            },
            open: state.open_multiselects.contains(&index),
            tooltip: tooltip(tip),
        },
        CustomFormElement::Input {
            text,
            placeholder,
            default,
            tooltip: tip,
        } => {
            let current = match value {
                Some(FormValue::Text(current)) => current.clone(),
                _ => default.to_string(),
            };
            CustomElement::Input {
                text: resolve(text),
                value: current,
                placeholder: resolve(placeholder),
                tooltip: tooltip(tip),
            }
        }
    }
}

/// Formats float values as integers for whole steps and with six decimals otherwise.
fn number_text(value: f64, step: f64) -> String {
    let value = value as f32;
    if (step as f32).fract() == 0.0 {
        format!("{}", value.floor() as i32)
    } else {
        format!("{value:.6}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorated_menu_button_images_use_the_shared_engine_route() {
        let model = ServerFormModel::ElementMenu(protocol::ElementMenuForm {
            title: protocol::FormText::from("Menu"),
            content: protocol::FormText::from(""),
            elements: vec![
                MenuElement::Header(protocol::FormText::from("Header")),
                MenuElement::Button {
                    text: protocol::FormText::from("Path"),
                    image: Some(FormButtonImage::Path(Arc::from("textures/items/apple"))),
                },
                MenuElement::Button {
                    text: protocol::FormText::from("Url"),
                    image: Some(FormButtonImage::Url(Arc::from(
                        "https://example.invalid/image",
                    ))),
                },
            ]
            .into(),
        });
        let state = FormEngineState::for_model(&model);
        for (remote, expected) in [
            (RemoteState::Loading, Some(ButtonImage::Loading)),
            (
                RemoteState::Ready(Arc::from([])),
                Some(ButtonImage::Url("https://example.invalid/image".to_owned())),
            ),
            (RemoteState::Failed, None),
        ] {
            let Some(FormModel::Action(form)) =
                engine_model(&model, &state, &|_| None, &|_| remote.clone(), &|text| {
                    text.to_string()
                })
            else {
                panic!("action form");
            };
            assert!(matches!(&form.elements[0], ActionElement::Header(_)));
            assert!(
                matches!(&form.elements[1], ActionElement::Button(FormButton { image: Some(ButtonImage::Path(path)), .. }) if path == "textures/items/apple")
            );
            let ActionElement::Button(button) = &form.elements[2] else {
                panic!("url button");
            };
            assert_eq!(button.image, expected);
        }
    }

    #[test]
    fn slider_labels_use_the_localized_format_and_fractional_step_precision() {
        let finite = |value| protocol::FormNumber::new(value).unwrap();
        let model = ServerFormModel::Custom(protocol::CustomForm {
            title: "Numbers".into(),
            icon: None,
            submit: None,
            elements: vec![CustomFormElement::Slider {
                text: "Volume".into(),
                min: finite(0.0),
                max: finite(10.0),
                step: finite(0.5),
                default: finite(2.5),
                timeout: finite(0.0),
                tooltip: None,
            }]
            .into(),
        });
        let state = FormEngineState::for_model(&model);
        let Some(FormModel::Custom(form)) = engine_model(
            &model,
            &state,
            &|key| (key == "options.sliderLabelFormat").then(|| Arc::from("%s : %s")),
            &|_| RemoteState::Failed,
            &|text| text.to_string(),
        ) else {
            panic!("custom form");
        };
        assert!(
            matches!(&form.elements[0], CustomElement::Slider { text, .. } if text == "Volume : 2.500000")
        );
    }

    #[test]
    fn whole_slider_values_drop_the_fraction() {
        assert_eq!(number_text(5.0, 1.0), "5");
        assert_eq!(number_text(2.5, 0.5), "2.500000");
        assert_eq!(number_text(-1.5, 1.0), "-2");
    }
}
