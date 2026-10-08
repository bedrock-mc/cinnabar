//! Full-height graphics expander built from the installed settings button and arrow templates.

use json_ui::{Catalog, Context};

/// Keeps the controller and option grid, adapting only the requested section-button treatment.
pub(super) fn install(catalog: &mut Catalog) {
    let Some(button) = json_ui::resolve(
        catalog,
        "settings_common.action_button",
        &Context::desktop(),
    )
    .control
    else {
        return;
    };
    let Some(size) = button.properties.get("size") else {
        return;
    };
    let overlay = serde_json::json!({
        "namespace": "general_section",
        "video_section/advanced_graphics_options_panel/advanced_graphics_options_button": {
            "size": size
        },
        "advanced_graphics_options_button_content": {
            "size": ["100% - 6px", "100%"],
            "use_child_anchors": true
        },
        "advanced_graphics_options_button_content/advanced_graphics_options_label": {
            "anchor_from": "left_middle", "anchor_to": "left_middle"
        },
        "advanced_graphics_options_button_content/plus_panel/plus": {
            "texture": "textures/ui/arrowRight"
        },
        "advanced_graphics_options_button_content/minus_panel/minus": {
            "texture": "textures/ui/arrowDown"
        }
    });
    catalog.overlay_text("ui/cinnabar_graphics_expander.json", &overlay.to_string());
}
