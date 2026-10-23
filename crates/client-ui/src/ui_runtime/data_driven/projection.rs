//! Projects the showing script screen from its store property into the OreUI form.

use oreui::data_driven_screen::{DataDrivenForm, FormEntry, FormItem};
use protocol::ddui::{
    ControlKind, CustomFormView, DataValue, FormControl, MessageBoxView, ScreenButton, ScreenText,
    ScriptedScreen,
};

use super::super::UiRuntime;
use super::ActiveScreen;

/// What a projected form item writes when the player uses it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ItemTarget {
    /// A custom form control, by its layout index.
    Control { index: usize, kind: TargetKind },
    /// A message box button; `true` for button 2.
    MessageButton(bool),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum TargetKind {
    Button,
    Toggle,
    Slider { min: f64, max: f64, step: f64 },
    Dropdown,
    TextField,
}

/// One drawn form and the target behind each of its items, aligned with `form.items`.
pub(super) struct Projection {
    pub(super) form: DataDrivenForm,
    pub(super) targets: Vec<Option<ItemTarget>>,
}

impl Projection {
    /// The item with `key` and the target it writes, while it shows.
    pub(super) fn item(&self, key: usize) -> Option<(&FormItem, Option<ItemTarget>)> {
        let position = self.form.items.iter().position(|entry| entry.key == key)?;
        Some((
            &self.form.items[position].item,
            self.targets.get(position).copied().flatten(),
        ))
    }
}

impl UiRuntime {
    /// Resolves script text: raw messages (or strings holding one) through the shared raw
    /// text resolver, anything else as written.
    pub(super) fn screen_text(&self, text: Option<ScreenText<'_>>) -> String {
        let Some(text) = text else {
            return String::new();
        };
        let source = text.source();
        if source.trim_start().starts_with('{')
            && let Ok(document) = protocol::parse_raw_text(&source)
        {
            return self.resolve_raw_text(&document).text;
        }
        source.into_owned()
    }

    pub(super) fn project_screen(&self, active: &ActiveScreen) -> Option<Projection> {
        let property = self
            .data_driven
            .stores()
            .property(protocol::ddui::SCRIPT_STORE, &active.property)?;
        match active.screen {
            ScriptedScreen::CustomForm => self.project_custom_form(property),
            ScriptedScreen::MessageBox => self.project_message_box(property),
        }
    }

    fn project_custom_form(&self, property: &DataValue) -> Option<Projection> {
        let view = CustomFormView::read(property)?;
        let mut items = Vec::with_capacity(view.controls.len());
        let mut targets = Vec::with_capacity(view.controls.len());
        for control in &view.controls {
            let (item, target) = self.project_control(control);
            items.push(FormEntry {
                key: control.index,
                item,
            });
            targets.push(target.map(|kind| ItemTarget::Control {
                index: control.index,
                kind,
            }));
        }
        Some(Projection {
            form: DataDrivenForm {
                title: self.screen_text(view.title),
                close: view.close_button.map(|button| !button.disabled),
                items,
            },
            targets,
        })
    }

    fn project_control(&self, control: &FormControl<'_>) -> (FormItem, Option<TargetKind>) {
        let text = |text| self.screen_text(text);
        let description =
            |text: Option<ScreenText<'_>>| text.map(|text| self.screen_text(Some(text)));
        match &control.kind {
            ControlKind::Header(value) => (FormItem::Header(text(*value)), None),
            ControlKind::Label(value) => (FormItem::Label(text(*value)), None),
            ControlKind::Divider => (FormItem::Divider, None),
            // Pack images are not drawn yet; they keep the spacer's place.
            ControlKind::Spacer | ControlKind::Image { .. } => (FormItem::Spacer, None),
            ControlKind::Button(button) => (
                FormItem::Button {
                    label: text(button.label),
                    enabled: !button.disabled,
                },
                Some(TargetKind::Button),
            ),
            ControlKind::Toggle {
                label,
                description: about,
                toggled,
                disabled,
            } => (
                FormItem::Toggle {
                    label: text(*label),
                    description: description(*about),
                    on: *toggled,
                    enabled: !disabled,
                },
                Some(TargetKind::Toggle),
            ),
            ControlKind::Slider {
                label,
                description: about,
                value,
                min,
                max,
                step,
                disabled,
            } => {
                let span = max - min;
                // Non-finite numbers cannot place a thumb; the slider then shows disabled.
                let usable = value.is_finite() && min.is_finite() && span.is_finite() && span > 0.0;
                let fraction = if usable {
                    ((value - min) / span).clamp(0.0, 1.0) as f32
                } else {
                    0.0
                };
                (
                    FormItem::Slider {
                        label: text(*label),
                        description: description(*about),
                        fraction,
                        value: number_text(*value),
                        enabled: !disabled && usable,
                    },
                    Some(TargetKind::Slider {
                        min: *min,
                        max: *max,
                        step: *step,
                    }),
                )
            }
            ControlKind::Dropdown {
                label,
                description: about,
                value,
                items,
                disabled,
            } => {
                let selected = items.iter().position(|item| item.value == *value);
                (
                    FormItem::Dropdown {
                        label: text(*label),
                        description: description(*about),
                        options: items.iter().map(|item| text(item.label)).collect(),
                        selected,
                        enabled: !disabled && !items.is_empty(),
                    },
                    Some(TargetKind::Dropdown),
                )
            }
            ControlKind::TextField {
                label,
                description: about,
                text: value,
                disabled,
            } => (
                FormItem::TextField {
                    label: text(*label),
                    description: description(*about),
                    text: (*value).to_owned(),
                    enabled: !disabled,
                },
                Some(TargetKind::TextField),
            ),
        }
    }

    fn project_message_box(&self, property: &DataValue) -> Option<Projection> {
        let view = MessageBoxView::read(property)?;
        // The body is key 0 and the buttons keep their numbers as keys.
        let mut items = vec![FormEntry {
            key: 0,
            item: FormItem::Label(self.screen_text(view.body)),
        }];
        let mut targets = vec![None];
        for (key, second, button) in [(1, false, view.button1), (2, true, view.button2)] {
            let Some(ScreenButton {
                label, disabled, ..
            }) = button
            else {
                continue;
            };
            items.push(FormEntry {
                key,
                item: FormItem::Button {
                    label: self.screen_text(label),
                    enabled: !disabled,
                },
            });
            targets.push(Some(ItemTarget::MessageButton(second)));
        }
        Some(Projection {
            form: DataDrivenForm {
                title: self.screen_text(view.title),
                close: None,
                items,
            },
            targets,
        })
    }
}

/// A slider value as its label shows it: whole numbers without a fraction.
fn number_text(value: f64) -> String {
    if value.fract() == 0.0 && value.abs() < 1e15 {
        format!("{value:.0}")
    } else {
        format!("{value}")
    }
}
