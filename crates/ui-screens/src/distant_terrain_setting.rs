//! Opt-in distant terrain controls in the JSON-UI Video settings: the toggle, distance,
//! quality and per-server storage cap, after the motion blur selector.

use json_ui::Catalog;
use launcher::menu::settings_options::{
    DISTANT_TERRAIN_DISTANCE_OPTION, DISTANT_TERRAIN_OPTION, DISTANT_TERRAIN_QUALITY_CHOICES,
    DISTANT_TERRAIN_QUALITY_OPTION, DISTANT_TERRAIN_STORAGE_OPTION, MOTION_BLUR_OPTION,
    SettingDefinition,
};
use serde_json::{Value, json};

/// The pressed name of the "Clear distant terrain cache" button.
pub const CLEAR_BUTTON: &str = "button.clear_distant_terrain_cache";

/// A slider bound to `option` through the settings host's generic slider bindings.
fn slider(option: SettingDefinition) -> Value {
    let name = option.name;
    json!({
        format!("{name}_slider@settings_common.option_slider"): {
            "$option_label": format!("#{name}_slider_label"),
            "$slider_name": name,
            "$slider_value_binding_name": format!("#{name}"),
            "$slider_tts_text_value": format!("#{name}_text_value"),
            "$option_enabled_binding_name": format!("#{name}_enabled"),
            "$slider_steps_binding_name": format!("#{name}_steps"),
            "$label_bindings": [{"binding_name": "$option_label"}]
        }
    })
}

/// Installs the controls into the advanced graphics options after motion blur.
pub fn install(catalog: &mut Catalog) {
    let toggle = DISTANT_TERRAIN_OPTION.name;
    let quality = DISTANT_TERRAIN_QUALITY_OPTION.name;
    let choices = DISTANT_TERRAIN_QUALITY_CHOICES
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
    let controls = vec![
        json!({
            format!("{toggle}_toggle@settings_common.option_toggle"): {
                "$option_label": DISTANT_TERRAIN_OPTION.label,
                "$option_binding_name": format!("#{toggle}"),
                "$option_enabled_binding_name": format!("#{toggle}_enabled"),
                "$toggle_name": toggle,
                "$focus_override_right": "FOCUS_OVERRIDE_STOP"
            }
        }),
        slider(DISTANT_TERRAIN_DISTANCE_OPTION),
        json!({
            format!("{quality}@settings_common.option_dropdown"): {
                "$option_label": DISTANT_TERRAIN_QUALITY_OPTION.label,
                "$dropdown_content": format!("general_section.{quality}_dropdown_content"),
                "$dropdown_area": "content_area",
                "$dropdown_name": format!("{quality}_dropdown"),
                "$option_enabled_binding_name": format!("#{quality}_dropdown_enabled"),
                "$options_dropdown_toggle_label_binding": format!("#{quality}_dropdown_toggle_label")
            }
        }),
        slider(DISTANT_TERRAIN_STORAGE_OPTION),
        json!({
            "distant_terrain_clear@settings_common.action_button": {
                "$pressed_button_name": CLEAR_BUTTON,
                "$button_text": "Clear distant terrain cache"
            }
        }),
    ];
    let overlay = json!({
        "namespace": "general_section",
        format!("{quality}_dropdown_content@settings_common.option_radio_dropdown_group"): {
            "$radio_buttons": choices
        },
        "advanced_graphics_options_section": {
            "modifications": [{
                "array_name": "controls",
                "operation": "insert_after",
                "control_name": MOTION_BLUR_OPTION.name,
                "value": controls
            }]
        }
    });
    catalog.overlay_text("ui/cinnabar_distant_terrain.json", &overlay.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has_text(control: &json_ui::ResolvedControl, text: &str) -> bool {
        control
            .properties
            .get("text")
            .and_then(|value| value.as_str())
            == Some(text)
            || control.children.iter().any(|child| has_text(child, text))
    }

    #[test]
    fn video_options_offer_distant_terrain_after_motion_blur() {
        let Some(carrier) = ui_engine::test_support::carrier() else {
            eprintln!(
                "skipping video_options_offer_distant_terrain_after_motion_blur: missing UI carrier; make assets"
            );
            return;
        };
        let files = carrier.ui_files();
        let mut catalog =
            Catalog::from_files(files.iter().map(|file| (&*file.path, &*file.bytes))).unwrap();
        super::super::motion_blur_setting::install(&mut catalog);
        install(&mut catalog);
        let context = json_ui::Context::retail(false);
        let section = json_ui::resolve(
            &catalog,
            "general_section.advanced_graphics_options_section",
            &context,
        )
        .control
        .expect("advanced video options");
        assert!(has_text(&section, DISTANT_TERRAIN_OPTION.label));
        let names: Vec<&str> = section
            .children
            .iter()
            .map(|child| child.name.as_str())
            .collect();
        let blur = names
            .iter()
            .position(|name| *name == MOTION_BLUR_OPTION.name)
            .expect("motion blur row");
        assert_eq!(
            names.get(blur + 1),
            Some(&format!("{}_toggle", DISTANT_TERRAIN_OPTION.name).as_str())
        );
        let choices = json_ui::resolve(
            &catalog,
            &format!(
                "general_section.{}_dropdown_content",
                DISTANT_TERRAIN_QUALITY_OPTION.name
            ),
            &context,
        )
        .control
        .expect("quality choices");
        for choice in DISTANT_TERRAIN_QUALITY_CHOICES {
            assert!(has_text(&choices, choice.label));
        }
    }
}
