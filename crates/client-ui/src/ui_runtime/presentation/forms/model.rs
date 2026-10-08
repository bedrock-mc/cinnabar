//! Maps a decoded server form plus its live values onto the engine's form model.
//! A slider's label shows `text: value` and a step slider's `text: step`; the
//! exact vanilla label composition needs native confirmation.

use std::sync::Arc;

use json_ui::{
    ActionElement, ActionForm, ButtonImage, CustomElement, CustomForm, FormButton, FormModel,
    ModalForm,
};
use protocol::{CustomFormElement, FormButtonImage, MenuElement, ServerFormModel};

use super::remote_images::RemoteState;
use crate::ui_runtime::forms::{FormEngineState, FormValue};

/// `None` for a form the engine cannot draw (unsupported controls).
pub(super) fn engine_model(
    model: &ServerFormModel,
    state: &FormEngineState,
    translate: &dyn Fn(&str) -> Option<Arc<str>>,
    remote: &dyn Fn(&str) -> RemoteState,
) -> Option<FormModel> {
    Some(match model {
        ServerFormModel::TextMenu(menu) => FormModel::Action(ActionForm {
            title: menu.title.to_string(),
            body: menu.content.to_string(),
            elements: menu
                .buttons
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    ActionElement::Button(FormButton {
                        text: text.to_string(),
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
            title: menu.title.to_string(),
            body: menu.content.to_string(),
            elements: menu
                .elements
                .iter()
                .map(|element| match element {
                    MenuElement::Button { text, image } => ActionElement::Button(FormButton {
                        text: text.to_string(),
                        image: image.as_ref().and_then(|image| engine_image(image, remote)),
                    }),
                    MenuElement::Label(text) => ActionElement::Label(text.to_string()),
                    MenuElement::Header(text) => ActionElement::Header(text.to_string()),
                    MenuElement::Divider => ActionElement::Divider,
                })
                .collect(),
        }),
        ServerFormModel::Modal(modal) => FormModel::Modal(ModalForm {
            title: modal.title.to_string(),
            body: modal.content.to_string(),
            button1: modal.button1.to_string(),
            button2: modal.button2.to_string(),
        }),
        ServerFormModel::Custom(form) => FormModel::Custom(CustomForm {
            title: form.title.to_string(),
            elements: form
                .elements
                .iter()
                .enumerate()
                .map(|(index, element)| custom_element(index, element, state))
                .collect(),
            submit_text: match &form.submit {
                Some(text) => text.to_string(),
                None => translate("gui.submit")
                    .map_or_else(|| "Submit".to_owned(), |text| text.to_string()),
            },
            submit_visible: true,
        }),
        // NPC dialogue draws through its own screen, not a form template.
        ServerFormModel::NpcDialogue(_) | ServerFormModel::Unsupported(_) => return None,
    })
}

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

fn custom_element(
    index: usize,
    element: &CustomFormElement,
    state: &FormEngineState,
) -> CustomElement {
    let value = state.values.get(index);
    let tooltip = |tooltip: &Option<Arc<str>>| tooltip.as_deref().unwrap_or("").to_owned();
    match element {
        CustomFormElement::Label { text } => CustomElement::Label {
            text: text.to_string(),
        },
        CustomFormElement::Header { text } => CustomElement::Header {
            text: text.to_string(),
        },
        CustomFormElement::Divider => CustomElement::Divider,
        CustomFormElement::Toggle {
            text,
            default,
            tooltip: tip,
        } => CustomElement::Toggle {
            text: text.to_string(),
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
            ..
        } => {
            let current = match value {
                Some(FormValue::Slider(current)) => *current,
                _ => default.get(),
            };
            let span = max.get() - min.get();
            CustomElement::Slider {
                text: format!("{text}: {}", number_text(current)),
                fraction: if span > 0.0 {
                    (current - min.get()) / span
                } else {
                    0.0
                },
                tooltip: tooltip(tip),
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
                _ => *default as usize,
            };
            CustomElement::StepSlider {
                text: format!(
                    "{text}: {}",
                    steps.get(index).map_or("", |step| step.as_ref())
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
            text: text.to_string(),
            options: options.iter().map(|option| option.to_string()).collect(),
            index: match value {
                Some(FormValue::Dropdown(index)) => *index,
                _ => *default as usize,
            },
            open: state.open_dropdown == Some(index),
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
                text: text.to_string(),
                value: current,
                placeholder: placeholder.to_string(),
                tooltip: tooltip(tip),
            }
        }
    }
}

/// A slider value without a trailing `.0` for whole numbers.
fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorated_menu_button_images_use_the_shared_engine_route() {
        let model = ServerFormModel::ElementMenu(protocol::ElementMenuForm {
            title: Arc::from("Menu"),
            content: Arc::from(""),
            elements: vec![
                MenuElement::Header(Arc::from("Header")),
                MenuElement::Button {
                    text: Arc::from("Path"),
                    image: Some(FormButtonImage::Path(Arc::from("textures/items/apple"))),
                },
                MenuElement::Button {
                    text: Arc::from("Url"),
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
                engine_model(&model, &state, &|_| None, &|_| remote.clone())
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
    fn whole_slider_values_drop_the_fraction() {
        assert_eq!(number_text(5.0), "5");
        assert_eq!(number_text(2.5), "2.5");
    }
}
