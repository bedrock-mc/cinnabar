//! Video control for far terrain: the Auto / On / Off dropdown.

use json_ui::Catalog;
use launcher::menu::settings_options::{FAR_TERRAIN_CHOICES, FAR_TERRAIN_OPTION};
use serde_json::json;

/// Installs the dropdown after the authored render-distance slider, reusing the pack's
/// dropdown templates.
pub fn install(catalog: &mut Catalog) {
    let name = FAR_TERRAIN_OPTION.name;
    let choices = FAR_TERRAIN_CHOICES
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
    catalog.overlay_text(
        "ui/cinnabar_far_terrain.json",
        &json!({
            "namespace": "general_section",
            format!("{name}_dropdown_content@settings_common.option_radio_dropdown_group"): {
                "$radio_buttons": choices
            },
            "video_section": {"modifications": [{
                "array_name": "controls", "operation": "insert_after",
                "control_name": "render_distance_slider",
                "value": [{format!("{name}@settings_common.option_dropdown"): {
                    "$option_label": FAR_TERRAIN_OPTION.label,
                    "$dropdown_content": format!("general_section.{name}_dropdown_content"),
                    "$dropdown_area": "content_area",
                    "$dropdown_name": format!("{name}_dropdown"),
                    "$option_enabled_binding_name": format!("#{name}_dropdown_enabled"),
                    "$options_dropdown_toggle_label_binding": format!("#{name}_dropdown_toggle_label")
                }}]
            }]}
        })
        .to_string(),
    );
}
