//! The Video-section chat placement preference uses the existing dropdown templates.

use json_ui::{Catalog, Context};

use crate::menu::settings_options::{CHAT_POSITION_OPTION, SettingKind, SettingsOptions};

/// Adds a client preference to the vanilla Video section, using one shared option definition.
pub(super) fn install(catalog: &mut Catalog) {
    let option = CHAT_POSITION_OPTION;
    let SettingKind::Dropdown(choices) = option.kind else {
        unreachable!()
    };
    let name = option.name;
    let content = format!("{name}_content");
    let rows: Vec<_> = choices
        .iter()
        .map(|choice| {
            serde_json::json!({
                "@settings_common.radio_with_label": {
                    "$toggle_state_binding_name": format!("#{}", choice.name),
                    "$radio_label_text": choice.label
                }
            })
        })
        .collect();
    let overlay = serde_json::json!({
        "namespace": "general_section",
        "video_section": {
            "modifications": [{
                "array_name": "controls", "operation": "insert_front",
                "value": [{
                    format!("{name}@settings_common.option_dropdown"): {
                        "$option_label": option.label,
                        "$dropdown_content": format!("general_section.{content}"),
                        "$dropdown_area": "content_area",
                        "$dropdown_name": format!("{name}_dropdown"),
                        "$option_enabled_binding_name": format!("#{name}_dropdown_enabled"),
                        "$options_dropdown_toggle_label_binding": format!("#{name}_dropdown_toggle_label"),
                        "$dropdown_scroll_content_size": ["100%", "145%"]
                    }
                }]
            }]
        },
        format!("{content}@settings_common.option_radio_dropdown_group"): { "$radio_buttons": rows }
    });
    catalog.overlay_text("ui/cinnabar_chat_position.json", &overlay.to_string());
}

/// Resolution changes invalidate the retained screen when the saved placement changes.
pub(super) fn context(context: Context, options: &SettingsOptions) -> Context {
    context.with_flag("cinnabar_chat_top", options.chat_at_top())
}

#[cfg(test)]
mod tests;
