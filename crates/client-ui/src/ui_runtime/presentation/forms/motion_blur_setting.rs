//! Optional camera blur selector in the JSON-UI Video settings.

use json_ui::Catalog;
use serde_json::json;

use crate::menu::settings_options::{MOTION_BLUR_CHOICES, MOTION_BLUR_OPTION};

pub(super) fn install(catalog: &mut Catalog) {
    let name = MOTION_BLUR_OPTION.name;
    let choices = MOTION_BLUR_CHOICES
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
        "advanced_graphics_options_section": {
            "modifications": [{
                "array_name": "controls",
                "operation": "insert_after",
                "control_name": "msaa_slider",
                "value": [{
                    format!("{name}@settings_common.option_dropdown"): {
                        "$option_label": MOTION_BLUR_OPTION.label,
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
    catalog.overlay_text("ui/cinnabar_motion_blur.json", &overlay.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_runtime::presentation::forms::pack_harness;

    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    #[test]
    fn video_options_offer_motion_blur_and_all_presets() {
        let Some(carrier) = pack_harness::carrier() else {
            eprintln!(
                "skipping video_options_offer_motion_blur_and_all_presets: missing UI carrier; make assets"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
        install(&mut catalog);
        let context = json_ui::Context::retail(false);
        let section = json_ui::resolve(
            &catalog,
            "general_section.advanced_graphics_options_section",
            &context,
        )
        .control
        .expect("advanced video options");
        assert!(has_text(&section, MOTION_BLUR_OPTION.label));
        let choices = json_ui::resolve(
            &catalog,
            &format!(
                "general_section.{}_dropdown_content",
                MOTION_BLUR_OPTION.name
            ),
            &context,
        )
        .control
        .expect("motion blur presets");
        for choice in MOTION_BLUR_CHOICES {
            assert!(has_text(&choices, choice.label));
        }
    }
}
