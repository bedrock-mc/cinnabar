//! The terrain occlusion preference in the in-game Video screen.

use json_ui::Catalog;
use launcher::menu::settings_options::SettingId;
use serde_json::json;

/// Installs the shared toggle beside render distance without limiting visible terrain.
pub fn install(catalog: &mut Catalog) {
    let option = SettingId::ChunkOcclusionCulling.definition();
    let name = option.name;
    catalog.overlay_text(
        "ui/cinnabar_chunk_occlusion.json",
        &json!({
            "namespace": "general_section",
            "video_section": {"modifications": [{
                "array_name": "controls", "operation": "insert_after",
                "control_name": "render_distance_slider",
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
