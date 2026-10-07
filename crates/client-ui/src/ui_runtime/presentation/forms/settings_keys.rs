//! The vanilla keyboard grid reads the gameplay router's bindings.

use json_ui::{CollectionItem, DataSource, HitRegion, Scalar};

use crate::menu::{
    MenuAction, MenuView,
    settings_options::{
        EXTRA_GAMEPAD, EXTRA_KEYS, GAMEPAD_BINDINGS, GAMEPAD_OFFSET, KEY_BINDINGS, gamepad_icon,
        key_name,
    },
};

/// Populate each supported action's current key and the pack's reset button.
pub(super) fn bind(view: &MenuView, data: &mut DataSource, translate: &dyn Fn(&str) -> String) {
    let keyboard_labels = KEY_BINDINGS
        .iter()
        .map(|(_, label)| *label)
        .chain(EXTRA_KEYS.iter().map(|(label, _)| *label));
    for (collection, dimension, offset, labels) in [
        (
            "keyboard_standard_collection",
            "#keyboard_standard_grid_dimension",
            0,
            keyboard_labels.collect::<Vec<_>>(),
        ),
        (
            "gamepad_collection",
            "#gamepad_grid_dimension",
            GAMEPAD_OFFSET,
            GAMEPAD_BINDINGS
                .iter()
                .map(|(_, label)| *label)
                .chain(EXTRA_GAMEPAD.iter().map(|(label, _)| *label))
                .collect(),
        ),
    ] {
        let rows = labels
            .iter()
            .enumerate()
            .map(|(index, label)| {
                let index = offset + index;
                let name = if *label == "key.freelook" {
                    "Freelook".to_owned()
                } else {
                    translate(label)
                };
                let control = view.settings_options.key_control(index);
                let capturing = view.key_remap == Some(index as u16);
                let key = if capturing {
                    "...".to_owned()
                } else if offset == 0 {
                    control.map(key_name).unwrap_or_default()
                } else if control.is_none() {
                    translate("controllerLayoutScreen.unassigned")
                } else {
                    String::new()
                };
                let icon = if capturing {
                    ""
                } else {
                    control.map(gamepad_icon).unwrap_or_default()
                };
                CollectionItem::default()
                    .with("#keymapping_name", Scalar::Text(name.clone()))
                    .with("#audible_keymapping_name", Scalar::Text(name))
                    .with("#binding_button_text", Scalar::Text(key))
                    .with("#binding_icon_sprite", Scalar::Text(icon.to_owned()))
            })
            .collect();
        data.set_collection(collection, rows);
        data.set_grid_dimensions(dimension, [1, labels.len() as u32]);
    }
}

/// Route collection row presses to capture or restore that action's key.
pub(super) fn action(region: &HitRegion) -> Option<MenuAction> {
    match region.pressed.as_deref()? {
        "button.reset_keyboard_bindings" => return Some(MenuAction::SettingsResetBindings(false)),
        "button.reset_gamepad_bindings" => return Some(MenuAction::SettingsResetBindings(true)),
        _ => {}
    }
    let offset = match region.collection.as_deref()? {
        "keyboard_standard_collection" => 0,
        "gamepad_collection" => GAMEPAD_OFFSET,
        _ => return None,
    };
    let index = u16::try_from(offset + region.collection_index?).ok()?;
    match region.pressed.as_deref()? {
        "button.binding_button" => Some(MenuAction::SettingsKey(index)),
        "button.reset_binding" => Some(MenuAction::SettingsResetKey(index)),
        _ => None,
    }
}
