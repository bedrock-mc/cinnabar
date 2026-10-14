//! A retryable disconnect shows distinct retry and dismiss targets.

use crate::ui_runtime::UiRuntime;
use launcher::menu::{MenuAction, MenuView};
use ui::DpiScale;

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

#[test]
fn long_launcher_questions_scroll_without_moving_the_acknowledgement() {
    use launcher::menu::settings_options::SettingId;
    let mut presentation =
        super::super::UiPresentationRuntime::new(ui_engine::test_support::fixture_font()).unwrap();
    let mut view = MenuView::new(true, "Player".into());
    view.dialog = Some(launcher::menu::MenuDialog::StorageError);
    std::sync::Arc::make_mut(&mut view.storage).error =
        Some("Long notice: Please read this paragraph before continuing.\n".repeat(80));
    std::sync::Arc::make_mut(&mut view.settings_options)
        .set(SettingId::ScreenAnimations.index(), 0);
    presentation.set_menu_view(Some(view));
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
    let before = question_body_top(&nodes);
    assert!(
        presentation
            .menu_scrolls
            .wheel(ui::UiPoint::new(640.0, 360.0).unwrap(), -120.0, true)
    );
    nodes.clear();
    let after_hits = presentation
        .append_menu(&runtime, &mut nodes, &mut 1, metrics, 1280.0, 720.0)
        .unwrap();
    assert!(question_body_top(&nodes) < before);
    assert_eq!(
        after_hits, hits,
        "the acknowledgement remains pinned while the text scrolls"
    );
}
