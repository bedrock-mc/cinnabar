//! Player input on the showing script screen, written back to the server's data store.

use protocol::ddui::{
    CLOSE_BUTTON_CLICKS, ControlKind, CustomFormView, DataDrivenScreenCloseReason, DataPrimitive,
    DataValue, MessageBoxView, SCRIPT_STORE, data_store_update_packet,
};

use super::super::UiRuntime;
use super::projection::{ItemTarget, TargetKind};

/// One player input on the showing data-driven screen. Items are named by their
/// [`FormEntry::key`](oreui::data_driven_screen::FormEntry::key), the layout index of a custom
/// form control, so inputs and focus stay on their control as others appear or hide.
#[derive(Debug, Clone, PartialEq)]
pub enum DataDrivenAction {
    /// The header close button; ignored while the button is disabled.
    Close,
    /// Escape or Back: closes the form, pressing an enabled close button first.
    Dismiss,
    /// Uses the item: presses a button, flips a toggle, focuses a text field or opens and
    /// closes a dropdown's option list.
    Activate(usize),
    /// Moves a slider item to `fraction` of its range.
    Slide(usize, f32),
    /// Picks the open dropdown's option at this list position.
    Choose(usize),
    /// Appends typed text to the focused text field.
    Type(String),
    /// Removes the focused text field's last character.
    Backspace,
    /// Ends text entry and closes an open option list.
    Blur,
}

/// The control taking keyboard text or showing its option list, by layout index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Focus {
    Text(usize),
    Dropdown(usize),
}

/// The keys of the focused control: the edited text field and the dropdown whose option list
/// is open.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FocusItems {
    pub editing: Option<usize>,
    pub open_dropdown: Option<usize>,
}

impl UiRuntime {
    /// Applies a player input to the showing screen and queues what the server must receive.
    pub fn data_driven_action(&mut self, action: DataDrivenAction) {
        let Some(active) = self.data_driven.active.clone() else {
            return;
        };
        let Some(projection) = self.project_screen(&active) else {
            if matches!(action, DataDrivenAction::Dismiss) {
                self.close_data_driven_screen(DataDrivenScreenCloseReason::ClientCanceled);
            }
            return;
        };
        // Whether the enabled layout control with key `index` still shows as `kind`.
        let shows = |index: usize, wanted: fn(TargetKind) -> bool| {
            projection.item(index).is_some_and(|(item, target)| {
                item_enabled(item)
                    && matches!(target, Some(ItemTarget::Control { kind, .. }) if wanted(kind))
            })
        };
        let focus = self.data_driven.focus;
        match action {
            DataDrivenAction::Close | DataDrivenAction::Dismiss => {
                let enabled = projection.form.close == Some(true);
                if enabled {
                    self.write_screen(&active.property, CLOSE_BUTTON_CLICKS, Write::Press);
                }
                if enabled || action == DataDrivenAction::Dismiss {
                    self.close_data_driven_screen(DataDrivenScreenCloseReason::ClientCanceled);
                }
            }
            DataDrivenAction::Blur => self.data_driven.focus = None,
            DataDrivenAction::Type(_) | DataDrivenAction::Backspace => {
                let Some(Focus::Text(index)) = focus else {
                    return;
                };
                if !shows(index, |kind| kind == TargetKind::TextField) {
                    self.data_driven.focus = None;
                    return;
                }
                let path = format!("layout[{index}].text");
                let mut text = self
                    .screen_value(&active.property, &path)
                    .unwrap_or_default();
                match action {
                    DataDrivenAction::Type(typed) => text.push_str(&typed),
                    _ => {
                        if text.pop().is_none() {
                            return;
                        }
                    }
                }
                self.write_screen(
                    &active.property,
                    &path,
                    Write::Value(DataPrimitive::String(text.into())),
                );
            }
            DataDrivenAction::Choose(choice) => {
                self.data_driven.focus = None;
                let Some(Focus::Dropdown(index)) = focus else {
                    return;
                };
                if !shows(index, |kind| kind == TargetKind::Dropdown) {
                    return;
                }
                if let Some(value) = self.dropdown_value(&active.property, index, choice) {
                    self.write_screen(
                        &active.property,
                        &format!("layout[{index}].value"),
                        Write::Value(DataPrimitive::Number(value)),
                    );
                }
            }
            DataDrivenAction::Activate(item) | DataDrivenAction::Slide(item, _) => {
                let Some((_, Some(target))) = projection
                    .item(item)
                    .filter(|(form_item, _)| item_enabled(form_item))
                else {
                    return;
                };
                self.data_driven.focus = None;
                let ItemTarget::Control { index, kind } = target else {
                    let ItemTarget::MessageButton(second) = target else {
                        return;
                    };
                    if matches!(action, DataDrivenAction::Activate(_)) {
                        self.write_screen(
                            &active.property,
                            MessageBoxView::button_path(second),
                            Write::Press,
                        );
                        self.close_data_driven_screen(DataDrivenScreenCloseReason::ClientCanceled);
                    }
                    return;
                };
                let path = |member: &str| format!("layout[{index}].{member}");
                match (kind, action) {
                    (TargetKind::Button, DataDrivenAction::Activate(_)) => {
                        self.write_screen(&active.property, &path("onClick"), Write::Press);
                    }
                    (TargetKind::Toggle, DataDrivenAction::Activate(_)) => {
                        self.write_screen(&active.property, &path("toggled"), Write::Flip);
                    }
                    (TargetKind::TextField, DataDrivenAction::Activate(_)) => {
                        self.data_driven.focus = Some(Focus::Text(index));
                    }
                    (TargetKind::Dropdown, DataDrivenAction::Activate(_)) => {
                        self.data_driven.focus = (focus != Some(Focus::Dropdown(index)))
                            .then_some(Focus::Dropdown(index));
                    }
                    (
                        TargetKind::Slider { min, max, step },
                        DataDrivenAction::Slide(_, fraction),
                    ) => self.write_screen(
                        &active.property,
                        &path("value"),
                        Write::Value(DataPrimitive::Number(slider_value(
                            min, max, step, fraction,
                        ))),
                    ),
                    _ => {}
                }
            }
        }
    }

    /// The keys of the focused control while it still shows: the edited text field and the
    /// dropdown whose option list is open.
    #[must_use]
    pub fn data_driven_focus_items(&self) -> FocusItems {
        let (Some(active), Some(focus)) =
            (self.data_driven.active.as_ref(), self.data_driven.focus)
        else {
            return FocusItems::default();
        };
        let Some(projection) = self.project_screen(active) else {
            return FocusItems::default();
        };
        let (key, wanted) = match focus {
            Focus::Text(key) => (key, TargetKind::TextField),
            Focus::Dropdown(key) => (key, TargetKind::Dropdown),
        };
        let shows = projection.item(key).is_some_and(|(_, target)| {
            matches!(target, Some(ItemTarget::Control { kind, .. }) if kind == wanted)
        });
        let key = shows.then_some(key);
        match focus {
            Focus::Text(_) => FocusItems {
                editing: key,
                open_dropdown: None,
            },
            Focus::Dropdown(_) => FocusItems {
                editing: None,
                open_dropdown: key,
            },
        }
    }

    /// The string at `path` in the showing screen's property.
    fn screen_value(&self, property: &str, path: &str) -> Option<String> {
        self.data_driven
            .stores()
            .property(SCRIPT_STORE, property)?
            .at_path(path)?
            .as_str()
            .map(str::to_owned)
    }

    /// The value of dropdown `index`'s option at list position `choice`.
    fn dropdown_value(&self, property: &str, index: usize, choice: usize) -> Option<f64> {
        let property = self.data_driven.stores().property(SCRIPT_STORE, property)?;
        CustomFormView::read(property)?
            .controls
            .into_iter()
            .find(|control| control.index == index)
            .and_then(|control| match control.kind {
                ControlKind::Dropdown { items, .. } => items.get(choice).map(|item| item.value),
                _ => None,
            })
    }

    /// Writes one input locally and queues it for the server; unchanged values send nothing.
    fn write_screen(&mut self, property: &str, path: &str, write: Write) {
        let current = self
            .data_driven
            .stores()
            .property(SCRIPT_STORE, property)
            .and_then(|value| value.at_path(path));
        let value = match write {
            // A press counts up from what the server last saw.
            Write::Press => {
                DataPrimitive::Number(current.and_then(DataValue::as_number).unwrap_or(0.0) + 1.0)
            }
            Write::Flip => {
                DataPrimitive::Bool(!current.and_then(DataValue::as_bool).unwrap_or(false))
            }
            Write::Value(value) => {
                if current.is_some_and(|current| *current == DataValue::from(value.clone())) {
                    return;
                }
                value
            }
        };
        match self
            .data_driven
            .stores
            .write(SCRIPT_STORE, property, path, value)
        {
            Ok(update) => self
                .data_driven
                .outgoing
                .push_back(data_store_update_packet(&update)),
            Err(outcome) => tracing::debug!(target: "ddui", path, ?outcome, "screen input skipped"),
        }
    }
}

/// How an input changes the value at its path.
enum Write {
    Press,
    Flip,
    Value(DataPrimitive),
}

fn item_enabled(item: &oreui::data_driven_screen::FormItem) -> bool {
    use oreui::data_driven_screen::FormItem;
    match item {
        FormItem::Button { enabled, .. }
        | FormItem::Toggle { enabled, .. }
        | FormItem::Slider { enabled, .. }
        | FormItem::Dropdown { enabled, .. }
        | FormItem::TextField { enabled, .. } => *enabled,
        FormItem::Header(_) | FormItem::Label(_) | FormItem::Divider | FormItem::Spacer => false,
    }
}

/// The slider value at `fraction` of its range, snapped to whole steps from the minimum.
fn slider_value(min: f64, max: f64, step: f64, fraction: f32) -> f64 {
    let raw = min + (max - min) * f64::from(fraction.clamp(0.0, 1.0));
    if step > 0.0 && step.is_finite() {
        (min + ((raw - min) / step).round() * step).clamp(min, max)
    } else {
        raw
    }
}
