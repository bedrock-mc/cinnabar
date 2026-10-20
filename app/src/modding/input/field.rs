//! Platform IME and modifier observations for focused native mod fields.

use super::{Ime, PhysicalControls};
use bevy::prelude::*;
use ui::mod_panel::FieldModifiers;

/// Captures modifiers at a key edge so later releases cannot change their meaning.
pub(super) fn modifiers(keys: &[String]) -> FieldModifiers {
    let held = |left: &str, right: &str| keys.iter().any(|key| key == left || key == right);
    FieldModifiers {
        shift: held("ShiftLeft", "ShiftRight"),
        control: held("ControlLeft", "ControlRight"),
        alt: held("AltLeft", "AltRight"),
        super_key: held("SuperLeft", "SuperRight"),
    }
}

/// Drains platform IME messages once, admitting only the primary window's messages.
pub(super) fn take_ime(
    physical: &mut PhysicalControls,
    events: Option<&Messages<Ime>>,
    window: Entity,
) -> Vec<Ime> {
    events.map_or_else(Vec::new, |events| {
        physical
            .ime
            .read(events)
            .filter(|event| match event {
                Ime::Preedit { window: owner, .. }
                | Ime::Commit { window: owner, .. }
                | Ime::Enabled { window: owner }
                | Ime::Disabled { window: owner } => *owner == window,
            })
            .cloned()
            .collect()
    })
}

/// Updates preedit/committed text only while a native field owns focused input.
pub(super) fn apply_ime(
    physical: &mut PhysicalControls,
    events: &[Ime],
    ui: &mut mod_ui::ModUi,
    accepting: bool,
) {
    if !accepting || !ui.mod_panel_field_focused() {
        physical.composing = false;
        ui.mod_panel_preedit("", None);
        return;
    }
    for event in events {
        match event {
            Ime::Preedit { value, cursor, .. } => {
                physical.composing = !value.is_empty();
                ui.mod_panel_preedit(value, cursor.map(|(start, _)| start));
            }
            Ime::Commit { value, .. } => {
                physical.composing = false;
                ui.mod_panel_ime_commit(value);
            }
            Ime::Disabled { .. } => {
                physical.composing = false;
                ui.mod_panel_preedit("", None);
            }
            Ime::Enabled { .. } => {}
        }
    }
}

/// Enables platform composition only for focused native text fields and releases our ownership.
pub(super) fn sync_ime(
    physical: &mut PhysicalControls,
    window: &mut Window,
    ui: &mod_ui::ModUi,
    accepting: bool,
) {
    let enabled = accepting && ui.mod_panel_field_focused();
    if enabled || physical.ime_owned {
        window.ime_enabled = enabled;
    }
    if let Some(position) = ui.mod_panel_ime_position() {
        window.ime_position = Vec2::from_array(position);
    }
    physical.ime_owned = enabled;
    if !enabled {
        physical.composing = false;
    }
}
