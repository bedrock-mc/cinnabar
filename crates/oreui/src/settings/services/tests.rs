use crate::review_tests::paint;
use crate::theme;
use launcher::global_resources::Action as PackAction;
use launcher::menu::MenuView;
use std::collections::HashMap;
use {super::*, launcher::menu::MenuAction, launcher::menu::auth::AuthState};

/// Draws the account panel and returns its visible text and available actions.
fn account_panel(view: &MenuView) -> (Vec<String>, Vec<MenuAction>) {
    panel(view, account)
}

#[test]
fn french_account_keeps_gamertags_verbatim() {
    let mut view = MenuView::new(true, "Player".into());
    view.auth_state = AuthState::Authenticated;
    assert!(std::sync::Arc::make_mut(&mut view.settings_options).set_language("fr_FR"));
    for name in ["High", "Automatic", "options.fullscreen"] {
        view.feeds.profile.gamertag = name.into();
        let (labels, _) = account_panel(&view);
        assert!(labels.iter().any(|label| label == name), "{labels:?}");
    }
}

#[test]
fn french_storage_keeps_world_and_cache_names_verbatim() {
    use launcher::menu::settings_storage::StorageItem;
    let mut view = MenuView::new(true, "Player".into());
    assert!(std::sync::Arc::make_mut(&mut view.settings_options).set_language("fr_FR"));
    let storage_view = std::sync::Arc::make_mut(&mut view.storage);
    storage_view.expanded.fill(true);
    storage_view.worlds = vec![StorageItem {
        name: "High".into(),
        ..Default::default()
    }];
    storage_view.cached = vec![StorageItem {
        name: "options.fullscreen".into(),
        ..Default::default()
    }];
    let (labels, actions) = panel(&view, storage);
    for name in ["High", "options.fullscreen"] {
        assert!(labels.iter().any(|label| label == name), "{labels:?}");
    }
    assert!(actions.contains(&MenuAction::SettingsStorage(StorageAction::SelectWorld(0))));
    assert!(actions.contains(&MenuAction::SettingsStorage(StorageAction::Select(0))));
}

/// Draws one settings panel and returns its visible text and available actions.
fn panel(
    view: &MenuView,
    draw: fn(&mut Content<'_, '_>) -> Result<(), PaintError>,
) -> (Vec<String>, Vec<MenuAction>) {
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        draw(&mut content).unwrap();
    });
    let labels = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    (labels, hits.into_iter().map(|(action, _)| action).collect())
}

#[test]
fn account_settings_follow_profile_saved_account_and_local_fallback() {
    let mut view = MenuView::new(true, launcher::PRODUCT_NAME.into());
    view.auth_state = AuthState::Authenticated;
    view.feeds.accounts = vec![
        launcher::accounts::AccountProfile {
            id: "first".into(),
            gamertag: "Saved player".into(),
            ..Default::default()
        },
        launcher::accounts::AccountProfile {
            id: "second".into(),
            gamertag: "Other player".into(),
            ..Default::default()
        },
    ];
    view.feeds.account_active_id = Some("first".into());
    for (active, profile, expected) in [
        (Some("first"), "", "Saved player"),
        (Some("first"), "Updated player", "Updated player"),
        (Some("second"), "", "Other player"),
        (None, "", launcher::PRODUCT_NAME),
    ] {
        view.feeds.account_active_id = active.map(str::to_owned);
        view.feeds.profile.gamertag = profile.into();
        for over_world in [false, true] {
            view.over_world = over_world;
            let (labels, actions) = account_panel(&view);
            assert!(labels.iter().any(|label| label == expected), "{labels:?}");
            assert!(actions.contains(&MenuAction::SignOut));
            if expected != launcher::PRODUCT_NAME {
                assert!(!labels.iter().any(|label| label == launcher::PRODUCT_NAME));
            }
        }
    }
    view.auth_state = AuthState::SignedOut;
    view.feeds.profile = Default::default();
    view.feeds.account_active_id = None;
    let (labels, actions) = account_panel(&view);
    assert!(!labels.iter().any(|label| label == launcher::PRODUCT_NAME));
    assert!(actions.contains(&MenuAction::StartSignIn));
    assert!(!actions.contains(&MenuAction::SignOut));
}

/// A signed-in account lists only rows that do something; the inert Privacy and Realms
/// invitation buttons are gone.
#[test]
fn signed_in_account_rows_all_have_actions() {
    let mut view = MenuView::new(true, launcher::PRODUCT_NAME.into());
    view.auth_state = AuthState::Authenticated;
    let (labels, actions) = account_panel(&view);
    let buttons = ["Open", "Manage", "Sign Out"];
    let drawn = labels
        .iter()
        .filter(|label| buttons.contains(&label.as_str()))
        .count();
    assert_eq!(drawn, actions.len(), "{labels:?} {actions:?}");
    assert_eq!(
        actions,
        [
            MenuAction::SettingsSupport(SupportAction::Open(SupportLink::Gamertag)),
            MenuAction::SettingsSupport(SupportAction::Open(SupportLink::Account)),
            MenuAction::SignOut,
        ]
    );
}

/// Storage lists only the categories the client fills, so no row is permanently empty.
#[test]
fn storage_lists_only_categories_the_client_keeps() {
    use launcher::menu::settings_storage::StorageAction;
    let view = MenuView::new(true, launcher::PRODUCT_NAME.into());
    let (_, actions) = panel(&view, storage);
    let toggles: Vec<_> = CATEGORIES
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            actions.contains(&MenuAction::SettingsStorage(StorageAction::Toggle(
                *index as u8,
            )))
        })
        .map(|(_, category)| *category)
        .collect();
    assert_eq!(toggles, ["world", "cache"]);
}

#[test]
fn storage_offers_folder_buttons_only_when_the_host_can_open_folders() {
    use launcher::menu::settings_storage::{StorageAction, StorageFolder};
    let mut view = MenuView::new(true, launcher::PRODUCT_NAME.into());
    for opens_folders in [true, false] {
        std::sync::Arc::make_mut(&mut view.storage).opens_folders = opens_folders;
        let (_, actions) = panel(&view, storage);
        for folder in [StorageFolder::Worlds, StorageFolder::ResourcePacks] {
            let action = MenuAction::SettingsStorage(StorageAction::OpenFolder(folder));
            assert_eq!(actions.contains(&action), opens_folders, "{folder:?}");
        }
    }
}

#[test]
fn empty_pack_library_still_renders_the_always_active_vanilla_base() {
    let view = MenuView::new(true, "Player".into());
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        resources(&mut content).unwrap();
    });
    let labels: Vec<String> = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect(),
            ),
            _ => None,
        })
        .collect();
    assert!(labels.iter().any(|label| label == "Vanilla Textures"));
    assert!(labels.iter().any(|label| label.contains("Always active")));
    assert!(!hits.iter().any(|(action, _)| matches!(
        action,
        MenuAction::GlobalResources(
            PackAction::Deactivate(_) | PackAction::MoveUp(_) | PackAction::MoveDown(_)
        )
    )));
}

#[test]
fn checked_language_radio_has_the_primary_diamond_face() {
    let mut view = MenuView::new(true, "Player".into());
    view.language_choices = vec![("en_US".into(), "English".into())].into();
    let (_, _, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view: &view,
            span: [100.0, 900.0],
            column_width: 800.0,
            y: 100.0,
            translate: &|_| None,
            nested: false,
        };
        language(&mut content).unwrap();
    });
    assert!(nodes.iter().any(|node| match node.visual() {
        ui::UiVisual::Solid { color, .. } | ui::UiVisual::RotatedSprite { color, .. } =>
            *color == theme::PRIMARY_ROLE.fill,
        _ => false,
    }));
}

/// Touch availability selects editable controls while desktop-only hosts retain their hint.
#[test]
fn touch_settings_offer_the_live_sensitivity_on_touch_devices() {
    let mut view = MenuView::new(true, "Player".into());
    for available in [false, true] {
        view.touch_available = available;
        let (labels, actions) = panel(&view, touch);
        let sensitivity =
            launcher::menu::settings_options::SettingId::TouchSensitivity.index() as u16;
        assert_eq!(
            actions.iter().any(|action| matches!(action,
            MenuAction::SettingsOption(index, _) if *index == sensitivity)),
            available
        );
        assert_eq!(
            labels
                .iter()
                .any(|label| label == "Use a touch device to customize controls"),
            !available
        );
    }
}
