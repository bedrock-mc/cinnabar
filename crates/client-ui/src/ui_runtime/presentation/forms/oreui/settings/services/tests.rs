use super::*;
use crate::global_resources::Action as PackAction;
use crate::menu::MenuView;
use crate::ui_runtime::presentation::forms::oreui::review_tests::paint;
use crate::ui_runtime::presentation::forms::oreui::theme;
use std::collections::HashMap;

/// Draws the account panel and returns its visible text and available actions.
fn account_panel(view: &MenuView) -> (Vec<String>, Vec<MenuAction>) {
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
        account(&mut content).unwrap();
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
