//! The client-owned disconnect reason and reconnect actions.

use super::{
    modal::{self, Modal},
    paint::Canvas,
    theme,
    widgets::Variant,
};
use launcher::menu::{
    MenuAction, MenuView,
    disconnect::{self, DisconnectBody},
};
use ui_engine::engine::{
    screen::{PaintError, Translate, translated},
    text_paint::localized,
};

/// Draws the formatted server reason in a scrolling OreUI panel with pinned actions.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    error: &str,
    translate: Translate<'_>,
) -> Result<(), PaintError> {
    let words = disconnect::describe(error);
    let title = translated(translate, words.title.0, words.title.1);
    let body = match words.body {
        DisconnectBody::Key(key) => translated(translate, key, disconnect::body_fallback(key)),
        // Servers may send a whole lang key or embed `%` tokens, as the JSON-UI label resolved.
        DisconnectBody::Server(reason) => localized(&reason, translate).into_owned(),
    };
    let mut buttons = Vec::new();
    if view.can_reconnect {
        buttons.push((
            translated(translate, "cinnabar.reconnect", "Reconnect").into(),
            Variant::Primary,
            Some(MenuAction::Reconnect),
        ));
    }
    buttons.push((
        translated(translate, "gui.ok", "OK").into(),
        Variant::Secondary,
        Some(MenuAction::DismissDialog),
    ));
    modal::draw(
        canvas,
        view,
        size,
        &Modal {
            title: &title,
            body: body.into(),
            body_color: theme::TEXT,
            items: Vec::new(),
            buttons,
            close: None,
        },
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, sync::Arc};

    use launcher::menu::{MenuView, disconnect::from_server};

    /// Paints a server disconnect reason and returns its text runs joined.
    fn painted(reason: &str) -> String {
        let view = MenuView::new(true, "Player".into());
        let lang = |key: &str| -> Option<Arc<str>> {
            match key {
                "disconnectionScreen.serverFull" => Some("Wow this server is popular!".into()),
                "disconnect.kicked" => Some("Kicked by an operator".into()),
                _ => None,
            }
        };
        let (_, _, nodes) = crate::review_tests::paint(HashMap::new(), |canvas| {
            super::draw(canvas, &view, [1280.0, 720.0], &from_server(reason), &lang).unwrap();
        });
        nodes
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
            .collect::<Vec<_>>()
            .join(" ")
    }

    // A reason that is one lang key reads as its translation, not the raw key.
    #[test]
    fn whole_key_reasons_are_translated() {
        let text = painted("disconnectionScreen.serverFull");
        assert!(text.contains("Wow this server is popular!"), "{text}");
        assert!(!text.contains("disconnectionScreen"), "{text}");
    }

    // Embedded `%` tokens resolve while the surrounding server text stays.
    #[test]
    fn embedded_tokens_are_translated() {
        let text = painted("§c%disconnect.kicked: AFK");
        assert!(text.contains("Kicked by an operator: AFK"), "{text}");
        assert!(!text.contains('%'), "{text}");
    }
}
