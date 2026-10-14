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
use ui_engine::engine::screen::{PaintError, Translate, translated};

/// Draws the formatted server reason in a scrolling OreUI panel with pinned actions.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    error: &str,
    translate: Translate<'_>,
) -> Result<(), PaintError> {
    let title = translated(
        translate,
        "disconnectionScreen.disconnected",
        "Disconnected from Server",
    );
    let body = match disconnect::describe(error).body {
        DisconnectBody::Key(key) => translated(translate, key, key),
        DisconnectBody::Server(reason) => reason,
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
