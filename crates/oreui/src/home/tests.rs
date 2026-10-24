use super::super::review_tests::paint;
use std::collections::HashMap;
use ui::UiVisual;
use {
    super::*,
    launcher::menu::{MenuAction, MenuScreen, MenuView},
    ui::IconRef,
};

mod fit;

/// A signed-in home view with a stable identity and no downloaded artwork.
fn view() -> MenuView {
    let mut view = MenuView::new(true, "Fallback".into());
    view.auth_state = launcher::menu::auth::AuthState::Authenticated;
    view.feeds.profile.gamertag = "ActualPlayer".into();
    view
}

#[test]
fn desktop_preview_stays_between_the_identity_and_dressing_room_action() {
    let view = view();
    let mut preview = None;
    let (_, hits, nodes) = paint(HashMap::new(), |c| {
        preview = draw(c, &view, [1280.0, 720.0], None, &|_| None).unwrap();
    });
    let preview = preview.expect("desktop home shows the character");
    let wardrobe = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::Navigate(MenuScreen::DressingRoom))
        .unwrap()
        .1;
    assert!(preview.control[0] >= wardrobe.min().x());
    assert!(preview.control[2] <= wardrobe.max().x());
    assert!(preview.control[3] < wardrobe.min().y());
    assert!(preview.control[3] - preview.control[1] > 100.0);
    let identity = nodes.iter().find(|node| matches!(node.visual(), UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|g| g.codepoint).collect::<String>().replace(' ', "") == "ActualPlayer")).unwrap();
    assert!(identity.bounds().max().y() < preview.control[1]);
}

#[test]
fn brand_sits_above_translucent_panels_and_footer_names_game_and_protocol() {
    let view = view();
    let (_, _, nodes) = paint(HashMap::new(), |c| {
        c.title_artwork = Some(IconRef {
            color_mask: None,
            page: 18,
            uv: [0, 0, 400, 80],
            glint: false,
        });
        draw(c, &view, [1280.0, 720.0], None, &|_| None).unwrap();
    });
    let logo = nodes
        .iter()
        .find(|node| {
            matches!(
                node.visual(),
                UiVisual::Sprite {
                    texture_page: 18,
                    ..
                }
            )
        })
        .unwrap();
    let panels = nodes.iter().filter(|node| node.parent().is_none()
        && matches!(node.visual(), UiVisual::Solid { color, .. } if color[..3] == NEUTRAL80.fill[..3] && color[3] < 255))
        .collect::<Vec<_>>();
    assert!(!panels.is_empty());
    for panel in panels {
        assert!(logo.bounds().max().y() < panel.bounds().min().y());
    }
    let text = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|g| g.codepoint)
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>()
        .replace(' ', "");
    assert!(text.contains(&format!(
        "{}{}",
        launcher::PRODUCT_NAME,
        launcher::PRODUCT_VERSION
    )));
    assert!(text.contains(protocol::GAME_VERSION));
    assert!(text.contains(&format!(
        "Minecraft{}/Protocol{}",
        protocol::GAME_VERSION,
        protocol::PROTOCOL_VERSION
    )));
}

#[test]
fn compact_and_short_home_keep_all_actions_reachable() {
    let view = view();
    for size in [[480.0, 640.0], [480.0, 340.0], [960.0, 340.0]] {
        let mut preview = None;
        let mut targets = Vec::new();
        let (scrolls, _, _) = paint(HashMap::new(), |c| {
            c.capture_focus = true;
            preview = draw(c, &view, size, None, &|_| None).unwrap();
            targets = c.focus_hits.clone();
        });
        assert!(preview.is_none(), "short screens prioritize usable actions");
        assert!(scrolls.is_empty(), "home actions fit without scrolling");
        for action in [
            MenuAction::Navigate(MenuScreen::Play),
            MenuAction::Navigate(MenuScreen::Servers),
            MenuAction::Navigate(MenuScreen::Settings),
            MenuAction::Navigate(MenuScreen::Social),
            MenuAction::Navigate(MenuScreen::DressingRoom),
            MenuAction::OpenAccounts,
            MenuAction::Navigate(MenuScreen::Friends),
            MenuAction::Navigate(MenuScreen::Inbox),
            MenuAction::OpenExitDialog,
        ] {
            let target = targets
                .iter()
                .find(|(found, _)| *found == action)
                .unwrap()
                .1;
            let (_, hits, _) = paint(HashMap::new(), |c| {
                draw(c, &view, size, None, &|_| None).unwrap();
            });
            let bounds = hits
                .iter()
                .find(|(found, _)| *found == action)
                .unwrap_or_else(|| panic!("missing {action:?} at {size:?}; hits: {hits:?}"))
                .1;
            assert_eq!(bounds, target, "{action:?} must be fully visible");
            assert!(bounds.min().y() >= 0.0 && bounds.max().y() <= size[1]);
            assert!(bounds.min().x() >= 0.0 && bounds.max().x() <= size[0]);
            for (i, (_, a)) in hits.iter().enumerate() {
                for (_, b) in &hits[i + 1..] {
                    assert!(
                        a.max().x() <= b.min().x()
                            || b.max().x() <= a.min().x()
                            || a.max().y() <= b.min().y()
                            || b.max().y() <= a.min().y()
                    );
                }
            }
        }
    }
}

#[test]
fn signed_out_home_has_a_direct_sign_in_action() {
    let mut view = view();
    view.auth_state = launcher::menu::auth::AuthState::SignedOut;
    let (_, hits, _) = paint(HashMap::new(), |c| {
        draw(c, &view, [1280.0, 720.0], None, &|_| None).unwrap();
    });
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::StartSignIn)
    );
    let signin = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::StartSignIn)
        .unwrap()
        .1;
    let wardrobe = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::Navigate(MenuScreen::DressingRoom))
        .unwrap()
        .1;
    let friends = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::Navigate(MenuScreen::Friends))
        .unwrap()
        .1;
    assert!(signin.min().y() > wardrobe.max().y());
    assert_eq!(signin.min().y(), friends.min().y());
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::Navigate(MenuScreen::Play))
    );
}

#[test]
fn update_ready_notice_leaves_quit_and_play_reachable() {
    let mut view = view();
    view.updates.ready = Some(launcher::menu::updates::Ready {
        version: "2.0.0".into(),
        writable: true,
        protocol_required: false,
    });
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        draw(canvas, &view, [1280.0, 720.0], None, &|_| None).unwrap();
    });
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::OpenExitDialog)
    );
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::Navigate(MenuScreen::Play))
    );
    let text = nodes
        .iter()
        .filter_map(|node| match node.visual() {
            UiVisual::Text { layout, .. } => Some(
                layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect::<String>()
        .replace(' ', "");
    assert!(text.contains("Updateavailable"));
    assert!(
        hits.iter()
            .any(|(action, _)| *action
                == MenuAction::Update(launcher::menu::updates::Action::Restart))
    );
    assert!(
        !hits.iter().any(
            |(action, _)| *action == MenuAction::Update(launcher::menu::updates::Action::Later)
        )
    );
}
