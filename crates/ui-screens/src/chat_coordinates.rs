//! The Creator coordinate controls in the native chat header.

use json_ui::{DataSource, HitRegion, Scalar};

use crate::chat_screen::{ChatHit, ChatScreen};

/// `chat.popup_toast` keeps its copied feedback visible for this authored interval.
const COPIED_TOAST_MILLIS: u64 = 750;

/// The Creator coordinate header's source selection and copied feedback.
#[derive(Default)]
pub struct ChatCoordinates {
    position: Option<[f32; 3]>,
    block: Option<[i32; 3]>,
    facing: bool,
    dropdown: bool,
    copied_at: Option<u64>,
}

impl ChatCoordinates {
    /// Formats the selected source exactly as the native copy callbacks do.
    fn text(&self) -> Option<String> {
        if self.facing {
            self.block.map(|[x, y, z]| format!("{x} {y} {z}"))
        } else {
            self.position
                .filter(|position| position.iter().all(|axis| axis.is_finite()))
                .map(|[x, y, z]| format!("{x:.2} {y:.2} {z:.2}"))
        }
    }
}

impl ChatCoordinates {
    /// Supplies current local feet and the same verified block target used by picking.
    pub fn set(&mut self, position: Option<[f32; 3]>, block: Option<[i32; 3]>) {
        self.position = position;
        self.block = block;
    }

    /// Edits only the transient header selection; it never changes the chat draft.
    pub fn select(&mut self, facing: Option<bool>) {
        if let Some(facing) = facing {
            self.facing = facing;
            self.dropdown = false;
        } else {
            self.dropdown = !self.dropdown;
        }
    }

    /// Starts native feedback only after the platform clipboard accepted the copy.
    pub fn copied(&mut self, now_millis: u64) {
        self.copied_at = Some(now_millis);
    }
}

impl ChatScreen {
    /// Returns copyable text only while the Creator control is enabled.
    pub fn coordinate_text(&self) -> Option<String> {
        (self.settings.options.value("copy_coordinate_ui") != 0)
            .then(|| self.coordinates.text())
            .flatten()
    }
}

/// Binds the authored dropdown, invalid-target state and copied toast.
pub(crate) fn bind(
    chat: &ChatScreen,
    data: &mut DataSource,
    now_millis: u64,
    translate: &dyn Fn(&str) -> String,
) {
    let coordinates = &chat.coordinates;
    let visible = chat.settings.options.value("copy_coordinate_ui") != 0;
    let text = coordinates.text();
    for (name, value) in [
        ("#chat_coordinate_dropdown_visible", visible),
        ("#chat_coordinate_dropdown", coordinates.dropdown),
        ("#chat_coordinate_dropdown_enabled", true),
        ("#coordinate_type_position", !coordinates.facing),
        ("#coordinate_type_facing", coordinates.facing),
        ("#copy_button_enabled", text.is_some()),
    ] {
        data.set_global(name, Scalar::Bool(value));
    }
    data.set_global(
        "#chat_coordinate_dropdown_label",
        Scalar::Text(translate(if coordinates.facing {
            "chat.coordinateTypeFacing"
        } else {
            "chat.coordinateTypePosition"
        })),
    );
    data.set_global(
        "#coordinates_text",
        Scalar::Text(text.unwrap_or_else(|| translate("chat.coordinatesInvalid"))),
    );
    if visible
        && !chat.settings.open
        && coordinates
            .copied_at
            .is_some_and(|born| now_millis.saturating_sub(born) < COPIED_TOAST_MILLIS)
    {
        data.set_factory_id("toast_message");
        data.set_global(
            "#toast_title",
            Scalar::Text(translate("chat.coordinateCopiedToast")),
        );
    }
}

/// Resolves native coordinate controls without interpreting their geometry.
pub(crate) fn action(region: &HitRegion) -> Option<ChatHit> {
    match region.pressed.as_deref() {
        Some("copy_coordinates_button") => return Some(ChatHit::CopyCoordinates),
        Some("paste_button") => return Some(ChatHit::Paste),
        _ => {}
    }
    match region.control_name.as_deref()?.trim_start_matches('#') {
        "chat_coordinate_dropdown" => Some(ChatHit::CoordinateDropdown),
        "coordinate_type_position" => Some(ChatHit::CoordinateSource(false)),
        "coordinate_type_facing" => Some(ChatHit::CoordinateSource(true)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_copy_formats_and_invalid_targets_are_preserved() {
        let mut coordinates = ChatCoordinates {
            position: Some([-12.125, 64.0, 8.25]),
            block: Some([-13, 65, 8]),
            ..Default::default()
        };
        assert_eq!(coordinates.text().as_deref(), Some("-12.12 64.00 8.25"));
        coordinates.facing = true;
        assert_eq!(coordinates.text().as_deref(), Some("-13 65 8"));
        coordinates.block = None;
        assert_eq!(coordinates.text(), None);
        coordinates.facing = false;
        coordinates.position = Some([f32::NAN, 1.0, 2.0]);
        assert_eq!(coordinates.text(), None);
    }
}
