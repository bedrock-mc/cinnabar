//! A retryable disconnect shows distinct retry and dismiss targets.

use crate::ui_runtime::UiRuntime;
use launcher::menu::{MenuAction, MenuScreen, MenuView};
use ui::DpiScale;

#[test]
fn external_notice_is_visible_and_owns_input_on_each_menu_route() {
    let message = "Browser preferences could not be saved.";
    for screen in [MenuScreen::Home, MenuScreen::Settings, MenuScreen::Servers] {
        let mut presentation =
            super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
                .unwrap();
        let mut view = MenuView::new(true, "Fixture".into());
        view.screen = screen;
        view.dialog = Some(launcher::menu::MenuDialog::Notice);
        view.message = Some(message.into());
        presentation.set_menu_view(Some(view));
        let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
            Some(2),
        );
        let mut nodes = Vec::new();
        let hits = presentation
            .append_menu(
                &UiRuntime::new(1),
                &mut nodes,
                &mut 1,
                metrics,
                1280.0,
                720.0,
            )
            .unwrap();
        assert!(nodes.iter().any(|node| {
            match node.visual() {
                ui::UiVisual::Text { layout, .. } => layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>()
                    .contains(message),
                _ => false,
            }
        }));
        assert!(
            hits.iter()
                .any(|(action, _)| *action == MenuAction::DismissDialog)
        );
        assert!(
            hits.iter()
                .all(|(action, _)| *action == MenuAction::DismissDialog)
        );
    }
}

#[test]
fn retryable_disconnect_renders_distinct_retry_and_dismiss_targets() {
    let mut presentation =
        super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    for size in [[1280, 720], [854, 480]] {
        let mut view = MenuView::new(true, "Player".into());
        view.disconnect_message = Some(launcher::menu::disconnect::from_server(
            "§cYou are banned.\n§fPlease contact the moderation team.",
        ));
        view.can_reconnect = true;
        view.focused_action = Some(MenuAction::Reconnect);
        presentation.set_menu_view(Some(view));
        let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
            size,
            DpiScale::new(1.0).unwrap(),
            None,
        );
        let mut nodes = Vec::new();
        let hits = presentation
            .append_menu(
                &UiRuntime::new(1),
                &mut nodes,
                &mut 1,
                metrics,
                size[0] as f32,
                size[1] as f32,
            )
            .unwrap();
        let retry = hits
            .iter()
            .find(|(action, _)| *action == MenuAction::Reconnect)
            .expect("retry hit target")
            .1;
        let dismiss = hits
            .iter()
            .find(|(action, _)| *action == MenuAction::DismissDialog)
            .expect("dismiss hit target")
            .1;
        assert!(retry.max().y() <= dismiss.min().y());
        assert!(retry.min().x() >= 0.0 && dismiss.max().y() <= size[1] as f32);
        let texts = super::pack_harness::drawn_texts(&nodes);
        assert!(texts.iter().any(|text| text == "Reconnect"), "{texts:?}");
        let words = texts.join(" ");
        assert!(
            words.contains("Disconnected from Server") && words.contains("OK"),
            "{words}"
        );
        assert!(
            words.contains("You are banned.") && words.contains("Please contact"),
            "{words}"
        );
        assert_eq!(
            presentation.projections.menu_focus,
            [MenuAction::Reconnect, MenuAction::DismissDialog]
        );
        assert!(nodes.iter().any(|node| matches!(node.visual(), ui::UiVisual::Text { layout, .. } if layout.glyphs().iter().any(|glyph| glyph.style.color == ui::BedrockColor::Red))));
    }
}

#[test]
fn a_disconnect_without_a_retry_target_keeps_only_the_acknowledgement() {
    let mut presentation =
        super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.disconnect_message = Some("network session failed: closed".into());
    view.focused_action = Some(MenuAction::DismissDialog);
    presentation.set_menu_view(Some(view));
    let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
        [1280, 720],
        DpiScale::new(1.0).unwrap(),
        None,
    );
    let mut nodes = Vec::new();
    let hits = presentation
        .append_menu(
            &UiRuntime::new(1),
            &mut nodes,
            &mut 1,
            metrics,
            1280.0,
            720.0,
        )
        .unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, MenuAction::DismissDialog);
    let texts = super::pack_harness::drawn_texts(&nodes);
    assert!(
        !texts
            .iter()
            .any(|text| text.contains("Reconnect") || text.contains("cinnabar_reconnect")),
        "{texts:?}"
    );
}

// An unreachable destination is titled as a failed connection, not a server disconnect.
#[test]
fn connection_failures_take_the_failed_connection_title() {
    let mut presentation =
        super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.disconnect_message =
        Some("bridge connection failed: Connection refused (os error 61)".into());
    presentation.set_menu_view(Some(view));
    let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
        [1280, 720],
        DpiScale::new(1.0).unwrap(),
        None,
    );
    let mut nodes = Vec::new();
    presentation
        .append_menu(
            &UiRuntime::new(1),
            &mut nodes,
            &mut 1,
            metrics,
            1280.0,
            720.0,
        )
        .unwrap();
    let words = super::pack_harness::drawn_texts(&nodes).join(" ");
    assert!(words.contains("Failed to connect to the server"), "{words}");
    assert!(!words.contains("Disconnected from Server"), "{words}");
}

#[test]
fn launcher_questions_draw_without_a_json_ui_carrier() {
    let mut presentation =
        super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.connecting = true;
    view.feeds.server_trust = Some(launcher::menu::ServerTrustPrompt {
        id: 1,
        url: "https://example.invalid".into(),
        from_session_core: false,
    });
    presentation.set_menu_view(Some(view));
    let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
        [1280, 720],
        DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let mut nodes = Vec::new();
    let hits = presentation
        .append_menu(
            &UiRuntime::new(1),
            &mut nodes,
            &mut 1,
            metrics,
            1280.0,
            720.0,
        )
        .unwrap();
    assert_eq!(
        hits.iter().map(|(action, _)| *action).collect::<Vec<_>>(),
        [
            MenuAction::ServerTrust(true),
            MenuAction::ServerTrust(false)
        ]
    );
    assert_eq!(
        presentation.projections.menu_focus,
        [
            MenuAction::ServerTrust(true),
            MenuAction::ServerTrust(false)
        ]
    );
}

/// Finds the long question's body without depending on layout-node identities.
fn question_body_top(nodes: &[ui::UiNode]) -> f32 {
    nodes
        .iter()
        .find_map(|node| match node.visual() {
            ui::UiVisual::Text { layout, .. }
                if layout
                    .glyphs()
                    .iter()
                    .map(|glyph| glyph.codepoint)
                    .collect::<String>()
                    .starts_with("Long") =>
            {
                Some(node.bounds().min().y())
            }
            _ => None,
        })
        .expect("the question displays its body")
}

/// Opens a long carrier-free question with caller-selected wheel animation.
fn long_question(animated: bool) -> super::super::UiPresentationRuntime {
    use launcher::menu::settings_options::SettingId;
    let mut presentation =
        super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.dialog = Some(launcher::menu::MenuDialog::StorageError);
    std::sync::Arc::make_mut(&mut view.storage).error =
        Some("Long notice: Please read this paragraph before continuing.\n".repeat(80));
    std::sync::Arc::make_mut(&mut view.settings_options)
        .set(SettingId::ScreenAnimations.index(), i32::from(animated));
    presentation.set_menu_view(Some(view));
    presentation
}

/// Paints the question and records body position and active answer bounds.
fn paint_question(
    presentation: &mut super::super::UiPresentationRuntime,
) -> (f32, Vec<(MenuAction, ui::UiRect)>) {
    let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
        [1280, 720],
        DpiScale::new(1.0).unwrap(),
        Some(2),
    );
    let runtime = UiRuntime::new(1);
    let mut nodes = Vec::new();
    let hits = presentation
        .append_menu(&runtime, &mut nodes, &mut 1, metrics, 1280.0, 720.0)
        .unwrap();
    (question_body_top(&nodes), hits)
}

#[test]
fn long_launcher_questions_scroll_without_moving_the_acknowledgement() {
    let mut presentation = long_question(false);
    let (before, hits) = paint_question(&mut presentation);
    assert!(
        presentation
            .menu_scrolls
            .wheel(ui::UiPoint::new(640.0, 360.0).unwrap(), -120.0, true)
    );
    let (after, after_hits) = paint_question(&mut presentation);
    assert!(after < before);
    assert_eq!(
        after_hits, hits,
        "the acknowledgement remains pinned while the text scrolls"
    );
}

#[test]
fn animated_launcher_questions_keep_scrolling_across_frames() {
    let mut presentation = long_question(true);
    presentation.menu_seconds = 1.0;
    let (_, hits) = paint_question(&mut presentation);
    assert!(
        presentation
            .menu_scrolls
            .wheel(ui::UiPoint::new(640.0, 360.0).unwrap(), -120.0, true)
    );
    presentation.menu_seconds += 1.0 / 60.0;
    let (first, _) = paint_question(&mut presentation);
    presentation.menu_seconds += 1.0;
    let (settled, after_hits) = paint_question(&mut presentation);
    assert!(
        settled < first,
        "wheel motion must survive the underlying menu paint"
    );
    assert_eq!(
        hits, after_hits,
        "answers stay pinned throughout wheel motion"
    );
}

// A disconnect published before the join settles keeps the OreUI progress, not the legacy launcher.
#[test]
fn connecting_progress_survives_a_pending_disconnect() {
    let mut presentation =
        super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.connecting = true;
    view.disconnect_message = Some("network session failed: closed".into());
    let title =
        launcher::menu::join_progress::shown(&view.feeds.join, &|_, fallback| fallback.to_owned())
            .title;
    presentation.set_menu_view(Some(view));
    let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
        [1280, 720],
        DpiScale::new(1.0).unwrap(),
        None,
    );
    let mut nodes = Vec::new();
    presentation
        .append_menu(
            &UiRuntime::new(1),
            &mut nodes,
            &mut 1,
            metrics,
            1280.0,
            720.0,
        )
        .unwrap();
    // OreUI draws progress titles in capitals and may wrap them.
    let squash = |text: &str| {
        text.chars()
            .filter(|ch| !ch.is_whitespace())
            .collect::<String>()
            .to_lowercase()
    };
    let words = squash(&super::pack_harness::drawn_texts(&nodes).concat());
    assert!(words.contains(&squash(&title)), "{title}: {words}");
}

// Safe-area insets move the popup's visuals, so its answers and scroll area must move with them.
#[test]
fn launcher_question_targets_follow_the_safe_area_origin() {
    let origin = [600.0, 24.0];
    let mut flush = long_question(false);
    let (_, flush_hits) = paint_question(&mut flush);
    let mut inset = long_question(false);
    inset.set_safe_area(ui::SafeArea::new(origin[0], origin[1], 0.0, 0.0).unwrap());
    let (_, inset_hits) = paint_question(&mut inset);
    let shifted: Vec<_> = flush_hits
        .iter()
        .map(|(action, bounds)| {
            let moved = ui::UiRect::new(
                ui::UiPoint::new(bounds.min().x() + origin[0], bounds.min().y() + origin[1])
                    .unwrap(),
                ui::UiPoint::new(bounds.max().x() + origin[0], bounds.max().y() + origin[1])
                    .unwrap(),
            )
            .unwrap();
            (*action, moved)
        })
        .collect();
    assert_eq!(inset_hits, shifted);
    // The body's centre, moved past the popup's unshifted right edge.
    let point = ui::UiPoint::new(640.0 + origin[0], 360.0 + origin[1]).unwrap();
    assert!(inset.menu_scrolls.wheel(point, -120.0, true));
}

// OreUI routes publish window-space targets, so insets move them with the painted controls.
#[test]
fn oreui_route_targets_follow_the_safe_area_origin() {
    let origin = [48.0, 32.0];
    let mut disconnect = MenuView::new(true, "Player".into());
    disconnect.disconnect_message = Some(launcher::menu::disconnect::from_server("Closed"));
    disconnect.can_reconnect = true;
    let home = MenuView::new(true, "Player".into());
    for view in [disconnect, home] {
        let hits = |inset: [f32; 2]| {
            let mut presentation =
                super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font())
                    .unwrap();
            presentation.set_safe_area(ui::SafeArea::new(inset[0], inset[1], 0.0, 0.0).unwrap());
            presentation.set_menu_view(Some(view.clone()));
            let metrics = ui_engine::text_metrics::TextMetrics::for_viewport(
                [1280, 720],
                DpiScale::new(1.0).unwrap(),
                Some(2),
            );
            presentation
                .append_menu(
                    &UiRuntime::new(1),
                    &mut Vec::new(),
                    &mut 1,
                    metrics,
                    1280.0,
                    720.0,
                )
                .unwrap()
        };
        let flush = hits([0.0, 0.0]);
        assert!(!flush.is_empty());
        let moved: Vec<_> = flush
            .iter()
            .map(|(action, bounds)| (*action, bounds.translated(origin).unwrap()))
            .collect();
        assert_eq!(hits(origin), moved, "{:?}", view.flow());
    }
}
