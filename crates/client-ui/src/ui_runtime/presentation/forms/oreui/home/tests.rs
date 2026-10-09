use super::super::review_tests::paint;
use super::*;
use std::collections::HashMap;
use ui::UiVisual;

fn view() -> MenuView {
    let mut view = MenuView::new(true, "Fallback".into());
    view.auth_state = crate::menu::auth::AuthState::Authenticated;
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
fn brand_sits_above_both_panels_and_versions_name_the_app_and_protocol() {
    let view = view();
    let (_, _, nodes) = paint(HashMap::new(), |c| {
        c.title_artwork = Some(IconRef {
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
        && matches!(node.visual(), UiVisual::Solid { color, .. } if *color == NEUTRAL80.fill))
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
    assert!(text.contains(&format!("Protocol{}", protocol::PROTOCOL_VERSION)));
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
        let scroll = scrolls.first().unwrap();
        if size[1] < 500.0 {
            assert!(scroll.max > 0.0);
        }
        for action in [
            MenuAction::Navigate(MenuScreen::Play),
            MenuAction::Navigate(MenuScreen::Servers),
            MenuAction::Navigate(MenuScreen::Settings),
            MenuAction::Navigate(MenuScreen::Social),
            MenuAction::Navigate(MenuScreen::DressingRoom),
            MenuAction::Store(crate::store::OPEN),
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
            let offset = (target.max().y() - scroll.viewport.max().y()).clamp(0.0, scroll.max);
            let (_, hits, _) = paint(HashMap::from([(scroll.key.clone(), offset)]), |c| {
                draw(c, &view, size, None, &|_| None).unwrap();
            });
            let bounds = hits
                .iter()
                .find(|(found, _)| *found == action)
                .unwrap_or_else(|| panic!("missing {action:?} at {size:?}; hits: {hits:?}"))
                .1;
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
fn commerce_routes_do_not_render_service_promotional_art() {
    let mut view = view();
    view.feeds.home.store_art = Some(crate::menu::ButtonArt {
        default_background: "promotion".into(),
        banner: "BUY NOW".into(),
        ..Default::default()
    });
    let art = HashMap::from([(
        "promotion".to_owned(),
        IconRef {
            page: 17,
            uv: [0, 0, 64, 64],
            glint: false,
        },
    )]);
    let font = crate::ui_runtime::presentation::tests::fixture_font();
    let metrics = super::super::super::super::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
    let hits = {
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        canvas.artwork = Some(&art);
        draw(&mut canvas, &view, [1280.0, 720.0], None, &|_| None).unwrap();
        canvas.hits
    };
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::Navigate(MenuScreen::Social))
    );
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::Store(crate::store::OPEN))
    );
    assert!(!nodes.iter().any(|node| matches!(
        node.visual(),
        UiVisual::Sprite {
            texture_page: 17,
            ..
        }
    )));
    assert!(
        !nodes
            .iter()
            .any(|node| matches!(node.visual(), UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|g| g.codepoint).collect::<String>().contains("BUY")))
    );
}

#[test]
fn signed_out_home_has_a_direct_sign_in_action() {
    let mut view = view();
    view.auth_state = crate::menu::auth::AuthState::SignedOut;
    let (_, hits, _) = paint(HashMap::new(), |c| {
        draw(c, &view, [1280.0, 720.0], None, &|_| None).unwrap();
    });
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::StartSignIn)
    );
    assert!(
        hits.iter()
            .any(|(action, _)| *action == MenuAction::Navigate(MenuScreen::Play))
    );
}

#[test]
fn home_headings_and_hero_action_use_the_bold_bundled_block_face() {
    use crate::ui_runtime::oreui_fonts::OreUiFont;

    let base = crate::ui_runtime::presentation::tests::fixture_font();
    let font = base
        .with_named_font(OreUiFont::Seven.name(), &base)
        .unwrap()
        .with_named_font(OreUiFont::FiveBold.name(), &base)
        .unwrap();
    let regular_page = font
        .font_named(OreUiFont::Seven.name())
        .glyph('\u{fffd}')
        .unwrap()
        .page;
    let block_page = base.glyph('\u{fffd}').unwrap().page;
    let metrics = super::super::super::super::TextMetrics::for_viewport(
        [1280, 720],
        ui::DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let (mut nodes, mut next, mut layouts) =
        (Vec::new(), 1, ui::TextLayoutCache::new(128, 1024 * 1024));
    {
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        draw(&mut canvas, &view(), [1280.0, 720.0], None, &|_| None).unwrap();
    }
    let glyph_for = |text: &str| {
        nodes
            .iter()
            .find_map(|node| match node.visual() {
                UiVisual::Text { layout, .. }
                    if layout
                        .glyphs()
                        .iter()
                        .map(|glyph| glyph.codepoint)
                        .collect::<String>()
                        == text =>
                {
                    layout
                        .glyphs()
                        .iter()
                        .find(|glyph| !glyph.codepoint.is_whitespace())
                        .map(|glyph| (glyph.page, glyph.style.bold))
                }
                _ => None,
            })
            .unwrap_or_else(|| panic!("missing home label {text:?}"))
    };
    assert_eq!(glyph_for("LET'S PLAY"), (block_page, true));
    assert_eq!(glyph_for("PLAY"), (block_page, true));
    assert_eq!(glyph_for("Dressing Room"), (regular_page, false));
    assert_ne!(block_page, regular_page);
}
