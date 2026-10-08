//! The language radio collection in vanilla `general_section.json:5527`.

use json_ui::{CollectionItem, DataSource, HitKind, HitRegion, Scalar};

use crate::menu::{MenuAction, MenuView};

/// Supplies native labels, selection and the one-column grid the vanilla section requests.
pub(super) fn bind(view: &MenuView, data: &mut DataSource) {
    let selected = view.settings_options.language();
    let items = view
        .language_choices
        .iter()
        .enumerate()
        .map(|(index, (code, name))| {
            CollectionItem::default()
                .with("#language_description", Scalar::Text(name.clone()))
                .with(
                    "#language_initial_selected",
                    Scalar::Bool(selected.map_or(index == 0, |selected| selected == code)),
                )
        })
        .collect();
    data.set_collection("languages", items);
    data.set_grid_dimensions(
        "#language_grid_dimension",
        [1, view.language_choices.len() as u32],
    );
}

/// Routes only language radio hits; other settings retain their own handlers.
pub(super) fn action(region: &HitRegion) -> Option<MenuAction> {
    if region.kind != HitKind::Toggle || region.collection.as_deref() != Some("languages") {
        return None;
    }
    Some(MenuAction::SettingsLanguage(
        u16::try_from(region.collection_index?).ok()?,
    ))
}
