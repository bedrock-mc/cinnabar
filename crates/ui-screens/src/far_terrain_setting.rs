//! Video controls for far terrain: the on/off toggle and its reach slider.

use json_ui::Catalog;
use launcher::menu::settings_options::{FAR_TERRAIN_DISTANCE_OPTION, FAR_TERRAIN_OPTION};
use serde_json::json;

/// Installs both controls after the authored render-distance slider.
pub fn install(catalog: &mut Catalog) {
    let toggle = FAR_TERRAIN_OPTION.name;
    let slider = FAR_TERRAIN_DISTANCE_OPTION.name;
    catalog.overlay_text(
        "ui/cinnabar_far_terrain.json",
        &json!({
            "namespace": "general_section",
            "video_section": {"modifications": [{
                "array_name": "controls", "operation": "insert_after",
                "control_name": "render_distance_slider",
                "value": [
                    {format!("{toggle}@settings_common.option_toggle"): {
                        "$option_label": FAR_TERRAIN_OPTION.label,
                        "$option_binding_name": format!("#{toggle}"),
                        "$option_enabled_binding_name": format!("#{toggle}_enabled"),
                        "$toggle_name": toggle,
                        "$focus_override_right": "FOCUS_OVERRIDE_STOP"
                    }},
                    {format!("{slider}@settings_common.option_slider"): {
                        "$option_label": format!("#{slider}_slider_label"),
                        "$slider_name": slider,
                        "$slider_value_binding_name": format!("#{slider}"),
                        "$slider_tts_text_value": format!("#{slider}_text_value"),
                        "$option_enabled_binding_name": format!("#{slider}_enabled"),
                        "$slider_steps_binding_name": format!("#{slider}_steps"),
                        "$label_bindings": [{"binding_name": "$option_label"}]
                    }}
                ]
            }]}
        })
        .to_string(),
    );
}
