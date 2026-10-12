//! A retryable disconnect shows distinct retry and dismiss targets.

use crate::ui_runtime::UiRuntime;
use launcher::menu::{MenuAction, MenuView};
use ui::DpiScale;

#[test]
fn retryable_disconnect_renders_distinct_retry_and_dismiss_targets() {
    let Some(mut presentation) = super::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping retryable_disconnect_renders_distinct_retry_and_dismiss_targets: missing local UI carrier (make assets)"
        );
        return;
    };
    for size in [[1280, 720], [854, 480]] {
        let mut view = MenuView::new(true, "Player".into());
        view.disconnect_message = Some("network session failed: closed".into());
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
    }
}

#[test]
fn a_disconnect_without_a_retry_target_keeps_only_the_acknowledgement() {
    let Some(mut presentation) = super::pack_harness::engine_presentation() else {
        eprintln!(
            "skipping a_disconnect_without_a_retry_target_keeps_only_the_acknowledgement: missing local UI carrier (make assets)"
        );
        return;
    };
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
