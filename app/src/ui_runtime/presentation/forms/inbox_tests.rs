//! Real-carrier Inbox rows keep long service text inside their card.

use ui::{DpiScale, UiVisual};

use super::{pack_harness, snapshot};
use crate::menu::{InboxItem, MenuRuntime, MenuScreen};
use crate::ui_runtime::UiRuntime;

/// Builds one inbox frame without networking or a running game session.
fn draw_inbox(
    presentation: &mut super::super::UiPresentationRuntime,
    view: &crate::menu::MenuView,
) {
    presentation.set_menu_view(Some(view.clone()));
    presentation
        .build(
            &crate::player_runtime::PlayerRuntime::new(1),
            &UiRuntime::new(1),
            0,
            [1280, 720],
            DpiScale::new(1.0).unwrap(),
        )
        .unwrap();
}

#[test]
fn inbox_categories_keep_independent_scroll_positions() {
    use crate::menu::{MenuAction, inbox::Action};
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let mut view = MenuRuntime::new(true, 2, "Test".into()).view();
    view.screen = MenuScreen::Inbox;
    view.feeds.home.inbox = (0..30)
        .map(|index| InboxItem {
            instance_id: format!("news-{index}"),
            header: format!("News {index}"),
            category: "News".into(),
            ..Default::default()
        })
        .collect();
    let short_index = view.feeds.home.inbox.len();
    view.feeds.home.inbox.push(InboxItem {
        instance_id: "realm".into(),
        header: "Realm invitation".into(),
        category: "Realms".into(),
        ..Default::default()
    });
    draw_inbox(&mut presentation, &view);
    assert!(presentation.scroll_menu(ui::UiPoint::new(800.0, 360.0).unwrap(), -20.0, false));
    draw_inbox(&mut presentation, &view);
    let saved_offsets = presentation.menu_scrolls.offsets().clone();
    assert!(saved_offsets.values().any(|offset| *offset > 0.0));
    view.feeds.inbox_state.category = 1;
    for _ in 0..3 {
        draw_inbox(&mut presentation, &view);
        assert!(
            presentation
                .menu_hit_targets
                .iter()
                .any(|(action, _)| { *action == MenuAction::Inbox(Action::Open(short_index)) }),
            "short category must remain visible and clickable without wheel input"
        );
    }
    view.feeds.inbox_state.category = 0;
    draw_inbox(&mut presentation, &view);
    for (key, offset) in saved_offsets {
        assert_eq!(presentation.menu_scrolls.offsets().get(&key), Some(&offset));
    }
}

#[test]
fn inbox_rows_ellipsize_titles_and_omit_the_body_summary() {
    let player_runtime = crate::player_runtime::PlayerRuntime::new(1);
    let Some(mut presentation) = pack_harness::engine_presentation() else {
        return;
    };
    let mut view = MenuRuntime::new(true, 2, "Test".into()).view();
    view.screen = MenuScreen::Inbox;
    view.feeds.home.inbox = ["First", "Second"]
        .map(|name| InboxItem {
            header: format!("{name} {}", "long title ".repeat(60)),
            body: format!(
                "Summary {}\n{}",
                "wide body ".repeat(80),
                "more text ".repeat(80)
            ),
            category: "News".into(),
            unread: true,
            ..Default::default()
        })
        .into();
    for size in [[1280, 720], [800, 600]] {
        presentation.set_menu_view(Some(view.clone()));
        let frame = presentation
            .build(
                &player_runtime,
                &UiRuntime::new(1),
                0,
                size,
                DpiScale::new(1.0).unwrap(),
            )
            .unwrap();
        let nodes = pack_harness::menu_nodes(&presentation);
        let mut rows = 0;
        for node in nodes {
            let UiVisual::Text { layout, .. } = node.visual() else {
                continue;
            };
            let text: String = layout
                .glyphs()
                .iter()
                .map(|glyph| glyph.codepoint)
                .collect();
            if !["First", "Second", "Summary"]
                .iter()
                .any(|prefix| text.starts_with(prefix))
            {
                continue;
            }
            rows += 1;
            assert_eq!(layout.line_count(), 1, "{text}");
            assert!(
                text.ends_with('…'),
                "long row must advertise truncation: {text}"
            );
        }
        assert_eq!(rows, 2);
        snapshot::write(&frame, &format!("inbox-bounded-{}", size[0]));
    }
}
