//! Pause and text-field drawing, without the menu adapter.

use super::*;
use launcher::menu::{MenuAction, MenuField, MenuScreen, MenuView};
use ui_engine::test_support::fixture_font;

#[test]
fn pause_dims_the_whole_world_evenly_and_keeps_the_character_clear_of_its_actions() {
    let mut view = MenuView::new(true, "Fixture".into());
    view.screen = MenuScreen::Pause;
    view.over_world = true;
    let mut preview = None;
    let (_, hits, nodes) = super::review_tests::paint(Default::default(), |canvas| {
        preview = super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let overlays: Vec<_> = super::test_support::solids(&nodes)
        .into_iter()
        .filter(|(_, color)| *color == super::theme::OVERLAY_SCREEN)
        .collect();
    assert_eq!(
        overlays,
        vec![([0.0, 0.0, 1280.0, 720.0], super::theme::OVERLAY_SCREEN)]
    );
    let preview = preview.expect("desktop Pause includes the interactive character");
    let wardrobe = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::Navigate(MenuScreen::DressingRoom))
        .unwrap()
        .1;
    assert!(preview.control[1] < preview.control[3]);
    assert!(preview.control[3] < wardrobe.min().y());
    assert!(preview.control[0] >= wardrobe.min().x());
    assert!(preview.control[2] <= wardrobe.max().x());
}

#[test]
fn pause_card_has_no_green_stripe_above_the_logo() {
    let view = MenuView::new(true, "Fixture".into());
    let (_, hits, nodes) = super::review_tests::paint(Default::default(), |canvas| {
        super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let resume = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::PauseResume)
        .unwrap()
        .1;
    for (bounds, color) in super::test_support::solids(&nodes) {
        if color == super::theme::PRIMARY_ROLE.fill {
            assert!(
                bounds[1] >= resume.min().y() && bounds[3] <= resume.max().y(),
                "green belongs to the Resume action, not the card edge"
            );
        }
    }
}

#[test]
fn pause_resume_follows_its_caption_without_a_large_empty_gap() {
    let view = MenuView::new(true, "Fixture".into());
    let mut rem = 0.0;
    let (_, hits, nodes) = super::review_tests::paint(Default::default(), |canvas| {
        rem = canvas.rem;
        super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let caption = nodes.iter().find(|node| matches!(node.visual(), ui::UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == "Pick up where you left off."
    )).unwrap().bounds();
    let resume = hits
        .iter()
        .find(|(action, _)| *action == MenuAction::PauseResume)
        .unwrap()
        .1;
    let gap = resume.min().y() - caption.max().y();
    assert!(
        gap >= 0.0 && gap <= rem * 2.4,
        "caption-to-action gap: {gap}"
    );
}

#[test]
fn pause_actions_align_with_the_character_footer_and_logo_has_extra_top_space() {
    let view = MenuView::new(true, "Fixture".into());
    let (_, hits, nodes) = super::review_tests::paint(Default::default(), |canvas| {
        canvas.title_artwork = Some(ui::IconRef {
            page: 1,
            uv: [0, 0, 300, 100],
            glint: false,
        });
        super::pause::draw(canvas, &view, [1280.0, 720.0], &|_| None).unwrap();
    });
    let bounds = |action| hits.iter().find(|(found, _)| *found == action).unwrap().1;
    let leave = bounds(MenuAction::PauseDisconnect);
    let wardrobe = bounds(MenuAction::Navigate(MenuScreen::DressingRoom));
    assert!(
        (leave.max().y() - wardrobe.max().y()).abs() < 0.01,
        "the menu and character actions share the panel's bottom inset"
    );
    let heading = nodes.iter().find(|node| matches!(node.visual(), ui::UiVisual::Text { layout, .. }
        if layout.glyphs().iter().map(|glyph| glyph.codepoint).collect::<String>() == "YOUR CHARACTER"
    )).unwrap().bounds();
    let logo = nodes
        .iter()
        .find(|node| {
            matches!(
                node.visual(),
                ui::UiVisual::Sprite {
                    texture_page: 1,
                    ..
                }
            )
        })
        .unwrap()
        .bounds();
    assert!(
        logo.min().y() > heading.min().y(),
        "the logo has more top breathing room than the text heading"
    );
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
