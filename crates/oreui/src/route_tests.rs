//! Pause and text-field drawing, without the menu adapter.

use super::*;
use launcher::menu::{MenuAction, MenuField, MenuScreen, MenuView};
use ui_engine::test_support::fixture_font;

#[test]
fn pause_shades_its_action_column_without_covering_the_character() {
    let view = MenuView::new(true, "Fixture".into());
    let mut preview = None;
    let (_, hits, nodes) = super::review_tests::paint(Default::default(), |canvas| {
        preview = super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let preview = preview.unwrap();
    let shade = super::test_support::solids(&nodes)
        .into_iter()
        .find(|(_, color)| *color == [0, 0, 0, 128])
        .unwrap()
        .0;
    for action in [
        MenuAction::PauseResume,
        MenuAction::PauseSettings,
        MenuAction::PauseDisconnect,
    ] {
        let bounds = hits.iter().find(|(found, _)| *found == action).unwrap().1;
        assert!(bounds.min().x() >= shade[0] && bounds.max().x() <= shade[2]);
    }
    assert!(preview.control[0] > shade[2]);
    let wardrobe = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::Navigate(MenuScreen::DressingRoom))
        .unwrap()
        .1;
    assert!(preview.control[3] < wardrobe.min().y());
}

#[test]
fn focused_fields_respect_the_blink_phase_and_hide_carets_during_selection() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.field = Some(MenuField::Name);
    for (shown, selection, expected) in [
        (true, None, true),
        (false, None, false),
        (true, Some([0, 2]), false),
    ] {
        view.caret.shown = shown;
        view.caret.selection = selection;
        let (_, _, nodes) = super::review_tests::paint(Default::default(), |canvas| {
            super::widgets::text_field(
                canvas,
                &view,
                [10.0, 20.0, 410.0, 68.0],
                "Text",
                "",
                true,
                Some(MenuAction::AddName),
            )
            .unwrap();
        });
        let caret = super::test_support::solids(&nodes)
            .iter()
            .any(|(_, color)| *color == super::theme::FIELD_CARET);
        assert_eq!(caret, expected);
    }
}

#[test]
fn focused_fields_draw_selection_over_the_selected_text() {
    let mut view = MenuView::new(true, "Fixture".into());
    for (focused, selection, visible) in [
        (true, Some([1, 3]), true),
        (true, Some([0, usize::MAX]), true),
        (false, Some([1, 3]), false),
        (true, Some([2, 2]), false),
        (true, None, false),
    ] {
        view.caret.selection = selection;
        let (_, _, nodes) = super::review_tests::paint(Default::default(), |canvas| {
            super::widgets::text_field(
                canvas,
                &view,
                [10.0, 20.0, 410.0, 68.0],
                "0é02",
                "",
                focused,
                Some(MenuAction::AddName),
            )
            .unwrap();
        });
        let highlights: Vec<_> = nodes
            .iter()
            .filter(|node| matches!(node.visual(), ui::UiVisual::InvertedSprite { .. }))
            .collect();
        assert_eq!(highlights.len(), usize::from(visible));
        if let Some(highlight) = highlights.first() {
            let label = nodes
                .iter()
                .find(|node| matches!(node.visual(), ui::UiVisual::Text { .. }))
                .unwrap();
            assert!(highlight.bounds().min().x() >= label.bounds().min().x());
            assert!(highlight.bounds().max().x() <= label.bounds().max().x());
            assert!(highlight.bounds().max().x() > highlight.bounds().min().x());
        }
    }
}

#[test]
fn text_fields_keep_their_inset_even_on_every_edge() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.hovered = Some(MenuAction::AddName);
    let bounds = [10.0, 20.0, 410.0, 68.0];
    for focused in [false, true] {
        let (_, _, nodes) = super::review_tests::paint(Default::default(), |canvas| {
            super::widgets::text_field(
                canvas,
                &view,
                bounds,
                "",
                "My server",
                focused,
                Some(MenuAction::AddName),
            )
            .unwrap();
        });
        let (face, _) = super::test_support::solids(&nodes)
            .into_iter()
            .find(|(_, color)| *color == super::theme::NEUTRAL80.hovered)
            .unwrap();
        let insets = [
            face[0] - bounds[0],
            face[1] - bounds[1],
            bounds[2] - face[2],
            bounds[3] - face[3],
        ];
        assert!(
            insets.iter().all(|value| (*value - insets[0]).abs() < 0.01),
            "field insets: {insets:?}"
        );
    }
}

#[test]
fn text_fields_center_visible_glyphs_instead_of_the_font_line_box() {
    use ui::oreui_theme::OreUiFont;
    let base = fixture_font();
    let native = base
        .as_ref()
        .clone()
        .with_line_metrics(assets::FontLineMetrics {
            em_64: 32 * 64,
            ascent_64: 26 * 64,
            descent_64: 3 * 64,
        })
        .unwrap();
    let font = base
        .with_named_font(OreUiFont::Seven.name(), &native)
        .unwrap();
    let metrics = TextMetrics::for_viewport([1280, 720], ui::DpiScale::new(1.0).unwrap(), Some(2));
    let view = MenuView::new(true, "Fixture".into());
    for hovered in [None, Some(MenuAction::AddName)] {
        let mut view = view.clone();
        view.hovered = hovered;
        let (mut nodes, mut next, mut layouts) =
            (Vec::new(), 1, ui::TextLayoutCache::new(16, 65536));
        let mut canvas = Canvas::new(&mut nodes, &mut next, &mut layouts, &font, metrics, 0, None);
        super::widgets::text_field(
            &mut canvas,
            &view,
            [0.0, 0.0, 400.0, 48.0],
            "",
            "My server",
            false,
            Some(MenuAction::AddName),
        )
        .unwrap();
        let ink = nodes
            .iter()
            .filter_map(|node| {
                let ui::UiVisual::Text { layout, .. } = node.visual() else {
                    return None;
                };
                Some(
                    layout
                        .glyphs()
                        .iter()
                        .filter(|glyph| !glyph.codepoint.is_whitespace())
                        .fold(
                            [f32::INFINITY, f32::NEG_INFINITY],
                            |[top, bottom], glyph| {
                                [
                                    top.min(
                                        node.bounds().min().y() + glyph.bounds_64[1] as f32 / 64.0,
                                    ),
                                    bottom.max(
                                        node.bounds().min().y() + glyph.bounds_64[3] as f32 / 64.0,
                                    ),
                                ]
                            },
                        ),
                )
            })
            .next()
            .unwrap();
        assert!(
            ((ink[0] + ink[1]) * 0.5 - 24.0).abs() <= 0.5,
            "visible glyph bounds: {ink:?}"
        );
    }
}

#[test]
fn pause_character_name_follows_the_current_account_and_profile() {
    let mut view = MenuView::new(true, launcher::PRODUCT_NAME.into());
    view.feeds.accounts = vec![launcher::accounts::AccountProfile {
        id: "active".into(),
        gamertag: "Saved username".into(),
        ..Default::default()
    }];
    view.feeds.account_active_id = Some("active".into());
    for (profile_name, wanted) in [
        ("", "Saved username"),
        ("Updated username", "Updated username"),
    ] {
        view.feeds.profile.gamertag = profile_name.into();
        let (_, _, nodes) = super::review_tests::paint(Default::default(), |canvas| {
            super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
        });
        let labels = nodes
            .iter()
            .filter_map(|node| match node.visual() {
                ui::UiVisual::Text { layout, .. } => Some(
                    layout
                        .glyphs()
                        .iter()
                        .map(|glyph| glyph.codepoint)
                        .collect::<String>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(labels.iter().any(|label| label == wanted), "{labels:?}");
        assert!(!labels.iter().any(|label| label == launcher::PRODUCT_NAME));
    }
}
