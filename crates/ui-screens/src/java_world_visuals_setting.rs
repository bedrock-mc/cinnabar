//! Optional Java world presentation in the in-game Video menu.

use json_ui::Catalog;
use launcher::menu::settings_options::{JAVA_WORLD_VISUALS_DESCRIPTION, JAVA_WORLD_VISUALS_OPTION};
use serde_json::json;

/// Adds one opt-in control using the installed setting and description templates.
pub fn install(catalog: &mut Catalog) {
    let option = JAVA_WORLD_VISUALS_OPTION;
    let name = option.name;
    let overlay = json!({
        "namespace": "general_section",
        "advanced_graphics_options_section": {
            "modifications": [{
                "array_name": "controls",
                "operation": "insert_after",
                "control_name": "smooth_lighting_toggle",
                "value": [{
                    format!("{name}@settings_common.option_toggle"): {
                        "$option_label": option.label,
                        "$has_option_description": true,
                        "$option_description": JAVA_WORLD_VISUALS_DESCRIPTION,
                        "$option_binding_name": format!("#{name}"),
                        "$option_enabled_binding_name": format!("#{name}_enabled"),
                        "$toggle_name": name
                    }
                }]
            }]
        }
    });
    catalog.overlay_text("ui/cinnabar_java_world_visuals.json", &overlay.to_string());
}
