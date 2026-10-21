//! View Bobbing selector for full, hand-only and disabled movement bobbing.

use json_ui::Catalog;
use serde_json::json;

use launcher::menu::settings_options::{SettingId, VIEW_BOBBING_CHOICES};

/// Replaces the pack's bobbing toggle with its existing dropdown templates.
pub fn install(catalog: &mut Catalog) {
    let option = SettingId::ViewBobbing.definition();
    let name = option.name;
    let choices = VIEW_BOBBING_CHOICES
        .iter()
        .map(|choice| {
            json!({
                "@settings_common.radio_with_label": {
                    "$toggle_state_binding_name": format!("#{}", choice.name),
                    "$radio_label_text": choice.label
                }
            })
        })
        .collect::<Vec<_>>();
    let overlay = json!({
        "namespace": "general_section",
        format!("{name}_dropdown_content@settings_common.option_radio_dropdown_group"): {
            "$radio_buttons": choices
        },
        "video_section": {
            "modifications": [{
                "array_name": "controls",
                "operation": "replace",
                "control_name": "view_bobbing_toggle",
                "value": [{
                    "view_bobbing_toggle@settings_common.option_dropdown": {
                        "$option_label": option.label,
                        "$dropdown_content": format!("general_section.{name}_dropdown_content"),
                        "$dropdown_area": "content_area",
                        "$dropdown_name": format!("{name}_dropdown"),
                        "$option_enabled_binding_name": format!("#{name}_dropdown_enabled"),
                        "$options_dropdown_toggle_label_binding": format!("#{name}_dropdown_toggle_label")
                    }
                }]
            }]
        }
    });
    catalog.overlay_text("ui/cinnabar_view_bobbing.json", &overlay.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Finds a translated caption or choice label in an inherited control.
    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    /// The native menu exposes every mode using the saved option's bindings.
    #[test]
    fn video_options_offer_all_view_bobbing_modes() {
        let Some(carrier) = ui_engine::test_support::carrier() else {
            eprintln!(
                "skipping video_options_offer_all_view_bobbing_modes: missing installed UI carrier; make assets"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
        install(&mut catalog);
        let context = json_ui::Context::retail(false);
        let option = SettingId::ViewBobbing.definition();
        let section = json_ui::resolve(&catalog, "general_section.video_section", &context)
            .control
            .expect("video options");
        assert!(has_text(&section, option.label));
        let choices = json_ui::resolve(
            &catalog,
            &format!("general_section.{}_dropdown_content", option.name),
            &context,
        )
        .control
        .expect("view bobbing choices");
        for choice in VIEW_BOBBING_CHOICES {
            assert!(has_text(&choices, choice.label));
        }
    }
}
