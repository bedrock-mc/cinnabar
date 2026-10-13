//! Video control for far terrain: the on/off toggle.

use json_ui::Catalog;
use launcher::menu::settings_options::FAR_TERRAIN_OPTION;
use serde_json::json;

/// Installs the toggle after the authored render-distance slider.
pub fn install(catalog: &mut Catalog) {
    let toggle = FAR_TERRAIN_OPTION.name;
    catalog.overlay_text(
        "ui/cinnabar_far_terrain.json",
        &json!({
            "namespace": "general_section",
            "video_section": {"modifications": [{
                "array_name": "controls", "operation": "insert_after",
                "control_name": "render_distance_slider",
                "value": [{format!("{toggle}@settings_common.option_toggle"): {
                    "$option_label": FAR_TERRAIN_OPTION.label,
                    "$option_binding_name": format!("#{toggle}"),
                    "$option_enabled_binding_name": format!("#{toggle}_enabled"),
                    "$toggle_name": toggle,
                    "$focus_override_right": "FOCUS_OVERRIDE_STOP"
                }}]
            }]}
        })
        .to_string(),
    );
}
