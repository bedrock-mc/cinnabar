//! Player input on the showing script screen, written back to the server's data store.

use protocol::ddui::{
    CLOSE_BUTTON_CLICKS, DataDrivenScreenCloseReason, DataPrimitive, DataValue, MessageBoxView,
    SCRIPT_STORE, data_store_update_packet,
};

use super::super::UiRuntime;
use super::projection::{ItemTarget, TargetKind};

/// One player input on the showing data-driven screen.
#[derive(Debug, Clone, PartialEq)]
pub enum DataDrivenAction {
    /// The header close button, or Escape.
    Close,
    /// Presses the form item at this position: a button press or a toggle flip.
    Activate(usize),
    /// Moves a slider item to `fraction` of its range.
    Slide(usize, f32),
    /// Picks a dropdown item's option by its position in the list.
    Choose(usize, usize),
    /// Replaces a text field item's text.
    Text(usize, String),
}

impl UiRuntime {
    /// Applies a player input to the showing screen and queues what the server must receive.
    pub fn data_driven_action(&mut self, action: DataDrivenAction) {
        let Some(active) = self.data_driven.active.clone() else {
            return;
        };
        if action == DataDrivenAction::Close {
            self.write_screen(&active.property, CLOSE_BUTTON_CLICKS, Write::Press);
            self.close_data_driven_screen(DataDrivenScreenCloseReason::ClientCanceled);
            return;
        }
        let Some(projection) = self.project_screen(&active) else {
            return;
        };
        let item = match action {
            DataDrivenAction::Activate(item)
            | DataDrivenAction::Slide(item, _)
            | DataDrivenAction::Choose(item, _)
            | DataDrivenAction::Text(item, _) => item,
            DataDrivenAction::Close => return,
        };
        let enabled = projection.form.items.get(item).is_some_and(item_enabled);
        let Some(Some(target)) = projection.targets.get(item).copied().filter(|_| enabled) else {
            return;
        };
        match (target, action) {
            (ItemTarget::MessageButton(second), DataDrivenAction::Activate(_)) => {
                self.write_screen(
                    &active.property,
                    MessageBoxView::button_path(second),
                    Write::Press,
                );
                self.close_data_driven_screen(DataDrivenScreenCloseReason::ClientCanceled);
            }
            (ItemTarget::Control { index, kind }, action) => {
                let path = |member: &str| format!("layout[{index}].{member}");
                let write = match (kind, action) {
                    (TargetKind::Button, DataDrivenAction::Activate(_)) => {
                        Some((path("onClick"), Write::Press))
                    }
                    (TargetKind::Toggle, DataDrivenAction::Activate(_)) => {
                        Some((path("toggled"), Write::Flip))
                    }
                    (
                        TargetKind::Slider { min, max, step },
                        DataDrivenAction::Slide(_, fraction),
                    ) => Some((
                        path("value"),
                        Write::Value(DataPrimitive::Number(slider_value(
                            min, max, step, fraction,
                        ))),
                    )),
                    (TargetKind::Dropdown, DataDrivenAction::Choose(_, choice)) => self
                        .dropdown_value(&active.property, index, choice)
                        .map(|value| (path("value"), Write::Value(DataPrimitive::Number(value)))),
                    (TargetKind::TextField, DataDrivenAction::Text(_, text)) => Some((
                        path("text"),
                        Write::Value(DataPrimitive::String(text.into())),
                    )),
                    _ => None,
                };
                if let Some((path, write)) = write {
                    self.write_screen(&active.property, &path, write);
                }
            }
            _ => {}
        }
    }

    /// The value of dropdown `index`'s option at list position `choice`.
    fn dropdown_value(&self, property: &str, index: usize, choice: usize) -> Option<f64> {
        self.data_driven
            .stores()
            .property(SCRIPT_STORE, property)?
            .at_path(&format!("layout[{index}].items[{choice}].value"))?
            .as_number()
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
            Ok(update) => {
                if !self.queue_client_packet(data_store_update_packet(&update)) {
                    tracing::warn!(target: "ddui", path, "screen input dropped");
                }
            }
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
