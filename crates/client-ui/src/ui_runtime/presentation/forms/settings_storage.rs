//! Vanilla storage categories and cache operations (storage_management.json).

use crate::menu::{
    MenuAction, MenuView,
    settings_storage::{CATEGORIES, StorageAction},
};
use json_ui::{CollectionItem, DataSource, HitRegion, Scalar};

/// Supplies the authored category factory and its measured disk-backed rows.
pub(super) fn bind(view: &MenuView, data: &mut DataSource, translate: &dyn Fn(&str) -> String) {
    data.set_global("#storage_standard_visible", Scalar::Bool(true));
    data.set_global("#screenshots_gallery_enabled", Scalar::Bool(true));
    data.set_global("#category_panel_visible", Scalar::Bool(true));
    data.set_global(
        "#storage_panel_length",
        Scalar::Num(CATEGORIES.len() as f64),
    );
    data.set_global(
        "#clear_cache_button_text",
        Scalar::Text(translate("options.dev_clearAllCache")),
    );
    let mut categories = Vec::new();
    for (index, category) in CATEGORIES.iter().enumerate() {
        let items = view.storage.items(category);
        let bytes = items
            .iter()
            .fold(0_u64, |sum, item| sum.saturating_add(item.bytes));
        let label = if items.len() == 1 {
            "storageManager.mainSizeLabel"
        } else {
            "storageManager.mainSizeLabelPlural"
        };
        let size = translate(label)
            .replacen("%s", &size_text(bytes, translate), 1)
            .replacen("%s", &items.len().to_string(), 1);
        data.set_global(format!("#{category}_size"), Scalar::Text(size));
        data.set_global(
            format!("#{category}_length"),
            Scalar::Num(items.len() as f64),
        );
        categories.push(CollectionItem::new(format!("{category}_panel")).with(
            "#storage_dropdown",
            Scalar::Bool(view.storage.expanded[index]),
        ));
        data.set_collection(
            format!("{category}_panel"),
            items
                .iter()
                .enumerate()
                .map(|(row, item)| {
                    CollectionItem::default()
                        .with(
                            format!("#sub_{category}_name"),
                            Scalar::Text(item.name.clone()),
                        )
                        .with(
                            format!("#sub_{category}_size"),
                            Scalar::Text(size_text(item.bytes, translate)),
                        )
                        .with(
                            format!("#sub_{category}_date"),
                            Scalar::Text(item.date.clone()),
                        )
                        .with(
                            format!("#sub_{category}_game_type"),
                            Scalar::Text(translate(&item.game_type)),
                        )
                        .with(
                            format!("#{category}_isSelected"),
                            Scalar::Bool(selected(view, category, row)),
                        )
                        .with(
                            format!("#{category}_optionsVisible"),
                            Scalar::Bool(selected(view, category, row)),
                        )
                })
                .collect(),
        );
    }
    data.set_collection("storage_panel", categories);
}

/// Maps the pack's native toggles and delete buttons to storage operations.
pub(super) fn action(region: &HitRegion) -> Option<MenuAction> {
    let name = region
        .control_name
        .as_deref()
        .or(region.pressed.as_deref())?;
    let action = match name {
        "button.clear_cache" | "dev_clear_download_cache_button" => StorageAction::RequestClear,
        "button.deleteResources" => StorageAction::RequestDelete,
        "button.delete_local_screenshots" => StorageAction::RequestScreenshots,
        "cache_item_dropdown" => StorageAction::Select(region.collection_index?),
        "world_item_dropdown" => StorageAction::SelectWorld(region.collection_index?),
        name => StorageAction::Toggle(
            u8::try_from(
                CATEGORIES
                    .iter()
                    .position(|category| name == format!("#{category}_tab"))?,
            )
            .ok()?,
        ),
    };
    Some(MenuAction::SettingsStorage(action))
}

/// Uses vanilla's binary megabyte and gigabyte units.
fn size_text(bytes: u64, translate: &dyn Fn(&str) -> String) -> String {
    let (divisor, key) = if bytes >= 1 << 30 {
        (1_u64 << 30, "playscreen.fileSize.GB")
    } else {
        (1_u64 << 20, "playscreen.fileSize.MB")
    };
    format!("{:.2} {}", bytes as f64 / divisor as f64, translate(key))
}

/// Uses the pinned delete prompt, or shows a concrete disk error without hiding failure.
pub(super) fn dialog_model(
    view: &MenuView,
    dialog: crate::menu::MenuDialog,
    translate: super::menu_screens::Translate<'_>,
) -> (json_ui::FormModel, MenuAction) {
    use super::menu_screens::translated;
    let words = |key, fallback| translated(translate, key, fallback);
    let (title, body, button1, confirm) = if dialog == crate::menu::MenuDialog::StorageError {
        (
            words("menu.storageManagement", "Storage"),
            view.storage.error.clone().unwrap_or_default(),
            words("gui.ok", "OK"),
            MenuAction::DismissDialog,
        )
    } else if view.storage.deleting_screenshots {
        (
            words(
                "options.dev_deleteLocalScreenshots",
                "Delete Local Screenshots",
            ),
            words(
                "storageManager.delete.content.screenshots",
                "Delete all local screenshots?",
            ),
            words("storageManager.delete.confirm", "Delete"),
            MenuAction::SettingsStorage(StorageAction::ConfirmDelete),
        )
    } else {
        (
            words("storageManager.delete.title", "Delete %s permanently?").replace(
                "%s",
                &words("storageManager.contentType.cachedData", "Cached Data"),
            ),
            words(
                "storageManager.delete.content",
                "Are you sure you want to delete the selected items? %s%s%s",
            )
            .replace("%s", ""),
            words("storageManager.delete.confirm", "Delete"),
            MenuAction::SettingsStorage(StorageAction::ConfirmDelete),
        )
    };
    (
        json_ui::FormModel::Modal(json_ui::ModalForm {
            title,
            body,
            button1,
            button2: words("storageManager.delete.cancel", "Cancel"),
        }),
        confirm,
    )
}

/// Only existing mutable categories expose the authored delete tray.
fn selected(view: &MenuView, category: &str, index: usize) -> bool {
    match category {
        "cache" => view.storage.selected == Some(index),
        "world" => view.storage.selected_world == Some(index),
        _ => false,
    }
}
