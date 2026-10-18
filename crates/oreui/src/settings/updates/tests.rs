use super::*;
use crate::review_tests::paint;
use launcher::menu::{MenuView, settings_options::SettingId, updates::Status};
use std::collections::HashMap;

/// Paints the update section without clipping so tests can inspect its user-visible controls.
fn render(view: &MenuView, width: f32) -> (Vec<MenuAction>, String) {
    let (_, hits, nodes) = paint(HashMap::new(), |canvas| {
        let mut content = Content {
            canvas,
            view,
            span: [0.0, width],
            column_width: width,
            y: 0.0,
            translate: &|_| None,
            nested: false,
        };
        draw(&mut content).unwrap();
    });
    let text = nodes
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
        .collect::<String>()
        .replace(' ', "");
    (hits.into_iter().map(|(action, _)| action).collect(), text)
}

/// Manual checks remain available with automatic checks off; busy or in-world clients cannot retry.
#[test]
fn check_now_respects_worker_progress_and_the_home_menu_boundary() {
    for width in [320.0, 800.0] {
        let mut view = MenuView::new(true, "Player".into());
        std::sync::Arc::make_mut(&mut view.settings_options)
            .set(SettingId::AutoUpdateEnabled.index(), 0);
        for status in [
            Status::Idle,
            Status::Checking,
            Status::Downloading,
            Status::UpToDate,
            Status::Failed,
        ] {
            view.updates.status = status;
            let (actions, text) = render(&view, width);
            assert_eq!(
                actions.contains(&MenuAction::Update(Action::CheckNow)),
                !status.is_busy()
            );
            assert!(text.contains(&installed_version().replace(' ', "")));
            assert!(text.contains(&view.updates.status_text().replace(' ', "")));
            assert!(text.contains("Beta") && text.contains("Nightly"));
        }
        view.over_world = true;
        assert!(
            !render(&view, width)
                .0
                .contains(&MenuAction::Update(Action::CheckNow))
        );
        view.updates.active = false;
        let (actions, text) = render(&view, width);
        assert!(actions.is_empty() && text.is_empty());
    }
}
