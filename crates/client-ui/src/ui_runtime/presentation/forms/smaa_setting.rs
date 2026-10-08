//! Optional spatial anti-aliasing beside vanilla's MSAA slider.

use json_ui::Catalog;
use serde_json::json;

use crate::menu::settings_options::{SMAA_CHOICES, SMAA_OPTION};

/// Reuses the pack's dropdown templates immediately after its MSAA control.
pub(super) fn install(catalog: &mut Catalog) {
    let name = SMAA_OPTION.name;
    let choices = SMAA_CHOICES
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
                        "$option_label": SMAA_OPTION.label,
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
    catalog.overlay_text("ui/cinnabar_smaa.json", &overlay.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_runtime::presentation::forms::pack_harness;

    /// Finds a visible label throughout the inherited dropdown controls.
    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    #[test]
    fn video_options_show_spatial_antialiasing_beside_msaa() {
        let Some(carrier) = pack_harness::carrier() else {
            eprintln!(
                "skipping video_options_show_spatial_antialiasing_beside_msaa: missing UI carrier; make assets"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
        install(&mut catalog);
        let section = json_ui::resolve(
            &catalog,
            "general_section.advanced_graphics_options_section",
            &json_ui::Context::retail(false),
        )
        .control
        .expect("advanced video options");
        let msaa = section
            .children
            .iter()
            .position(|child| child.name == "msaa_slider")
            .expect("MSAA slider");
        let selector = &section.children[msaa + 1];
        assert_eq!(selector.name, SMAA_OPTION.name);
        assert!(has_text(selector, SMAA_OPTION.label));
        let choices = json_ui::resolve(
            &catalog,
            &format!("general_section.{}_dropdown_content", SMAA_OPTION.name),
            &json_ui::Context::retail(false),
        )
        .control
        .expect("spatial anti-aliasing choices");
        for choice in SMAA_CHOICES {
            assert!(has_text(&choices, choice.label));
        }
    }
}
