//! Vanilla Global Resources bindings; Import and live Apply are Cinnabar extensions.
//! Reference: vanilla ui/settings_sections/general_section.json:1552 and resource_packs_screen.json.

use super::menu_screens::{MenuScreenData, retail_context};
use json_ui::{CollectionItem, DataSource, HitRegion, Scalar};
use launcher::{
    global_resources::{Action, Snapshot},
    menu::{MenuAction, MenuView},
};

/// Binds the vanilla list, selection, priority and pack-settings controls.
pub(super) fn bind(snapshot: &Snapshot, data: &mut DataSource) {
    let shared = CollectionItem::default()
        .with(
            "#selected_grid_visible",
            Scalar::Bool(snapshot.active_expanded),
        )
        .with(
            "#available_grid_visible",
            Scalar::Bool(snapshot.available_expanded),
        )
        .with(
            "#selected_count",
            Scalar::Text(snapshot.active.len().to_string()),
        )
        .with(
            "#available_count",
            Scalar::Text(snapshot.available.len().to_string()),
        );
    for (active, packs, collection, dimensions, expanded) in [
        (
            true,
            &snapshot.active,
            "#selected_pack_items_global",
            "#selected_grid_dimensions_global",
            snapshot.active_expanded,
        ),
        (
            false,
            &snapshot.available,
            "#available_pack_items_global",
            "#available_grid_dimensions_global",
            snapshot.available_expanded,
        ),
    ] {
        let rows = packs
            .iter()
            .enumerate()
            .map(|(index, pack)| {
                let selected = snapshot.selected == Some((active, index));
                shared
                    .clone()
                    .with("#name", Scalar::Text(pack.name.clone()))
                    .with("#description", Scalar::Text(pack.description.clone()))
                    .with(
                        "#size",
                        Scalar::Text(pack.version.map(|v| v.to_string()).join(".")),
                    )
                    .with("#is_selected", Scalar::Bool(selected))
                    .with(
                        "#is_read_more",
                        Scalar::Bool(snapshot.details_expanded != Some((active, index))),
                    )
                    .with(
                        "#is_read_less",
                        Scalar::Bool(snapshot.details_expanded == Some((active, index))),
                    )
                    .with(
                        "#direction_button_visible",
                        Scalar::Bool(selected && !snapshot.busy),
                    )
                    .with("#can_move", Scalar::Bool(true))
                    .with("#can_sort_up", Scalar::Bool(active && index > 0))
                    .with(
                        "#can_sort_down",
                        Scalar::Bool(active && index + 1 < packs.len()),
                    )
                    .with(
                        "#has_pack_settings",
                        Scalar::Bool(!pack.subpacks.is_empty()),
                    )
                    .with(
                        "#icon_path",
                        Scalar::Text(
                            snapshot
                                .icons
                                .get(&(pack.id.to_string(), pack.revision))
                                .cloned()
                                .unwrap_or_else(default_icon),
                        ),
                    )
            })
            .collect();
        data.set_collection(collection, rows);
        data.set_collection_defaults(collection, shared.values.clone());
        data.set_global(dimensions, Scalar::Num(packs.len() as f64));
        data.set_global(
            if active {
                "#selected_grid_visible"
            } else {
                "#available_grid_visible"
            },
            Scalar::Bool(expanded),
        );
    }
    data.set_global(
        "#selected_count",
        Scalar::Text(snapshot.active.len().to_string()),
    );
    data.set_global(
        "#available_count",
        Scalar::Text(snapshot.available.len().to_string()),
    );
    data.set_global(
        "#no_available_packs_visibility_global",
        Scalar::Bool(snapshot.available.is_empty()),
    );
    data.set_global("#default_item_texture_global", Scalar::Text(default_icon()));
    data.set_global(
        "#cinnabar_pack_status",
        Scalar::Text(snapshot.message.clone()),
    );
    data.set_global("#cinnabar_pack_idle", Scalar::Bool(!snapshot.busy));
}

/// The base pack supplies the fallback artwork for packs without a usable icon.
fn default_icon() -> String {
    format!("{}pack_icon.png", super::server_pack::VANILLA_IN_PACKAGE)
}

/// Opens vanilla's content-tier panel over Settings for the selected active pack.
pub(super) fn overlay(snapshot: &Snapshot) -> Option<Box<MenuScreenData>> {
    let index = snapshot.settings?;
    let pack = snapshot.active.get(index)?;
    let mut data = DataSource::new();
    data.set_strict(true);
    data.set_global("#close_button_visible", Scalar::Bool(true));
    let selected = snapshot.selection.get(index).map(|p| p.subpack.as_str());
    let tier = pack
        .subpacks
        .iter()
        .position(|p| Some(p.folder.as_str()) == selected)
        .unwrap_or(0);
    data.set_global("#pack_settings_title", Scalar::Text(pack.name.clone()));
    data.set_global(
        "#has_content_tiering",
        Scalar::Bool(!pack.subpacks.is_empty()),
    );
    data.set_global(
        "#content_tier_supported",
        Scalar::Bool(
            pack.subpacks
                .get(tier)
                .is_none_or(|pack| pack.memory_tier <= snapshot.memory_tier),
        ),
    );
    data.set_global("#content_tier_value", Scalar::Num(tier as f64));
    data.set_global(
        "#content_tier_steps",
        Scalar::Num(pack.subpacks.len() as f64),
    );
    data.set_global(
        "#content_tier_label",
        Scalar::Text(
            pack.subpacks
                .get(tier)
                .map(|pack| pack.name.clone())
                .unwrap_or_default(),
        ),
    );
    Some(Box::new(MenuScreenData {
        reference: "pack_settings.screen",
        context: retail_context(),
        data,
        overlay: None,
    }))
}

/// Maps vanilla pack buttons back to commands for the host's queue.
pub(super) fn action(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
    let target = region.pressed.as_deref()?;
    let index = region.collection_index.unwrap_or(0);
    let action = match target {
        "button.menu_exit" if view.global_resources.settings.is_some() => Action::CloseSettings,
        "button.selected_pack_global" => Action::SelectActive(index),
        "button.available_pack_global" => Action::SelectAvailable(index),
        "button.deselected_pack_global" => Action::SelectActive(index),
        "button.move_left_global" => {
            if region.collection.as_deref() == Some("#available_pack_items_global") {
                Action::Activate(index)
            } else {
                Action::Deactivate(index)
            }
        }
        "button.sort_up_global" => Action::MoveUp(index),
        "button.sort_down_global" => Action::MoveDown(index),
        "button.pack_settings_global" => Action::Settings(index),
        "button.read_toggle_global" => Action::ReadMore(
            region.collection.as_deref() == Some("#selected_pack_items_global"),
            index,
        ),
        "button.expand_selected_global" => Action::ToggleActive,
        "button.expand_available_global" => Action::ToggleAvailable,
        "button.cinnabar_import_pack" => Action::Import,
        "button.cinnabar_apply_packs" => Action::Apply,
        _ => return None,
    };
    Some(MenuAction::GlobalResources(action))
}

/// Adds only Cinnabar's import/apply/status row to the vanilla global pack panel.
pub(super) fn extend_catalog(catalog: &mut json_ui::Catalog) {
    catalog.overlay_text("ui/cinnabar_global_resources.json", EXTENSION);
}

const EXTENSION: &str = r##"{
  "namespace": "general_section",
  "global_texture_pack_section": {
    "controls": [
      { "cinnabar_pack_actions": {
        "type": "stack_panel", "orientation": "horizontal", "size": ["100%", 24],
        "controls": [
          { "import@common_buttons.light_text_button": {
            "size": ["50%", 22], "$button_text": "Import", "$pressed_button_name": "button.cinnabar_import_pack"
          }},
          { "apply@common_buttons.light_text_button": {
            "size": ["50%", 22], "$button_text": "Apply", "$pressed_button_name": "button.cinnabar_apply_packs",
            "bindings": [{"binding_name": "#cinnabar_pack_idle", "binding_name_override": "#enabled"}]
          }}
        ]
      }},
      { "cinnabar_pack_status": {
        "type": "label", "size": ["100%", "default"], "text": "#cinnabar_pack_status",
        "bindings": [{"binding_name": "#cinnabar_pack_status"}]
      }},
      { "cinnabar_pack_list@resource_packs.selected_stack_panel": {} }
    ]
  }
}"##;

/// Splits the tier slider into exactly the selected pack's declared choices.
pub(super) fn slider_actions(view: &MenuView, region: &HitRegion) -> Option<Vec<MenuAction>> {
    (region.control_name.as_deref() == Some("content_tier_slider")).then_some(())?;
    let pack = view
        .global_resources
        .settings
        .and_then(|index| view.global_resources.active.get(index))?;
    Some(
        (0..pack.subpacks.len())
            .map(|index| MenuAction::GlobalResources(Action::Subpack(index)))
            .collect(),
    )
}

#[cfg(test)]
mod tests;
