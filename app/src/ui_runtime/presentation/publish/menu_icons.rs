use client_ui::ui_runtime::presentation::UiPresentationRuntime;
use launcher::menu::MenuView;

/// Resolves navigation artwork through canonical item identities.
pub(super) fn install(presentation: &UiPresentationRuntime, view: &mut MenuView) {
    view.featured_icon = presentation.item_icon(
        assets::facts::presentation::UiItemIcon::Featured.identifier(),
        0,
    );
    view.realm_icon = presentation.item_icon(
        assets::facts::presentation::UiItemIcon::Realm.identifier(),
        0,
    );
    view.friend_icon = presentation.item_icon(
        assets::facts::presentation::UiItemIcon::Friend.identifier(),
        0,
    );
    view.saved_icon = presentation.item_icon(
        assets::facts::presentation::UiItemIcon::Book.identifier(),
        0,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn navigation_icons_resolve_in_the_pinned_item_inventory() {
        let mut entries = protocol::vanilla_item_registry()
            .iter()
            .map(|item| assets::IconEntry {
                identifier: item.identifier.to_string().into(),
                metadata: 0,
                sprite: 0,
            })
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| a.identifier.cmp(&b.identifier));
        let bytes = assets::encode_icon_catalog(
            [1; 32],
            &[assets::IconSprite {
                width: 1,
                height: 1,
                rgba8: Arc::from([90, 65, 40, 255]),
            }],
            &entries,
        )
        .unwrap();
        let presentation = UiPresentationRuntime::with_hud_and_icons(
            ui_engine::test_support::fixture_font(),
            client_ui::test_support::fixture_hud(),
            Arc::new(assets::RuntimeIconCatalog::decode(&bytes).unwrap()),
        )
        .unwrap();
        let mut view = MenuView::new(true, "Fixture".into());
        install(&presentation, &mut view);
        assert!(view.featured_icon.is_some());
        assert!(view.realm_icon.is_some());
        assert!(view.friend_icon.is_some());
        assert!(view.saved_icon.is_some());
    }
}
