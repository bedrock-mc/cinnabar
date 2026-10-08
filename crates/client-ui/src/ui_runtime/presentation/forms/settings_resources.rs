//! Global Resources section boundary for the resource-pack controller.

use json_ui::{DataSource, Scalar};

/// Supply the vanilla empty-list state until a global pack controller is installed.
pub(super) fn bind(data: &mut DataSource) {
    let icon = format!("{}pack_icon.png", super::server_pack::VANILLA_IN_PACKAGE);
    for binding in ["#cycling_icon_path_global", "#default_item_texture_global"] {
        data.set_global(binding, Scalar::Text(icon.clone()));
    }
    for list in ["selected", "available", "realms", "unowned", "invalid"] {
        data.set_collection(format!("#{list}_pack_items_global"), Vec::new());
        data.set_grid_dimensions(format!("#{list}_grid_dimensions_global"), [1, 0]);
    }
    for binding in [
        "#no_available_packs_visibility_global",
        "#no_realms_packs_visibility_global",
    ] {
        data.set_global(binding, Scalar::Bool(true));
    }
}
