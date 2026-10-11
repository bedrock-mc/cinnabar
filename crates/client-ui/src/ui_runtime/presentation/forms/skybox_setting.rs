//! Video control for the optional Bedrock skybox translation.

use json_ui::Catalog;
use launcher::menu::settings_options::NATIVE_SKYBOX_OFFSET_OPTION;
use serde_json::json;

/// Installs the persisted toggle beside the authored sky visibility control.
pub(super) fn install(catalog: &mut Catalog) {
    let option = NATIVE_SKYBOX_OFFSET_OPTION;
    let name = option.name;
    catalog.overlay_text(
        "ui/cinnabar_skybox_offset.json",
        &json!({
            "namespace": "general_section",
            "video_section": {"modifications": [{
                "array_name": "controls", "operation": "insert_after",
                "control_name": "render_clouds_toggle",
                "value": [{format!("{name}@settings_common.option_toggle"): {
                    "$option_label": option.label,
                    "$option_binding_name": format!("#{name}"),
                    "$option_enabled_binding_name": format!("#{name}_enabled"),
                    "$toggle_name": name,
                    "$focus_override_right": "FOCUS_OVERRIDE_STOP"
                }}]
            }]}
        })
        .to_string(),
    );
}
