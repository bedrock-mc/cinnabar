//! The section picker uses the menu's existing button faces and saved preferences.

use super::super::super::{TextMetrics, UiPresentationError, UiPresentationRuntime};
use super::paint::{Bounds, Canvas};
use super::theme::{
    Appearance, BODY, BORDER, CAPTION, EDGE, NEUTRAL, NEUTRAL80, OVERLAY_MODAL, TEXT,
};
use super::widgets::{Variant, button};
use crate::menu::{MenuAction, MenuView};
use launcher::menu::server_list::{ServerGroup, ServerListAction};
use ui::{UiNode, UiRect};

const SCROLL: &str = "servers.filter.sections";

impl UiPresentationRuntime {
    /// The picker owns input while section edits retain the underlying server selection.
    pub(in super::super) fn append_oreui_server_filter(
        &mut self,
        view: &MenuView,
        nodes: &mut Vec<UiNode>,
        next: &mut u32,
        metrics: TextMetrics,
        size: [f32; 2],
    ) -> Result<Vec<(MenuAction, UiRect)>, UiPresentationError> {
        let originals = self
            .form_presentation
            .oreui_originals
            .clone()
            .filter(|_| self.form_presentation.oreui_look == super::Look::Originals);
        let rollback = (nodes.len(), *next);
        let mut offsets = self.menu_scrolls.offsets().clone();
        for attempt in 0..2 {
            nodes.truncate(rollback.0);
            *next = rollback.1;
            let mut canvas = Canvas::new(
                nodes,
                next,
                &mut self.layouts,
                &self.font,
                metrics,
                self.solid_texture_page,
                originals.as_deref(),
            );
            canvas.appearance = Appearance::from_dark(view.settings_options.oreui_dark_mode());
            canvas.seconds = self.menu_seconds;
            canvas.transitions = Some(&mut self.form_presentation.oreui_transitions);
            canvas.offsets = offsets;
            draw(&mut canvas, view, size)?;
            let revealed = if view.focused_action == Some(MenuAction::DismissDialog) {
                self.menu_scrolls.observe_focus(view.focused_action);
                false
            } else {
                super::scroll_focus::reveal(&mut canvas, &mut self.menu_scrolls, view)
            };
            if !revealed || attempt == 1 {
                self.menu_scrolls.set_areas(canvas.scrolls);
                return Ok(canvas.hits);
            }
            offsets = canvas.offsets;
        }
        Ok(Vec::new())
    }
}

/// Every section remains configurable even when it has no current servers.
fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
    canvas.hits.clear();
    canvas.clear_focus_geometry();
    canvas.capture_focus = true;
    canvas.overlay(size, OVERLAY_MODAL)?;
    let width = canvas.r(60.0).min(size[0]);
    let rows = ServerGroup::ALL.len() as f32;
    let height = canvas
        .r(4.8 + 3.2 + rows * 5.2 + 4.4 + 2.0 * EDGE)
        .min(size[1]);
    let left = (size[0] - width) * 0.5;
    let top = (size[1] - height) * 0.5;
    let panel = [left, top, left + width, top + height];
    let edge = canvas.r(EDGE);
    canvas.fill(panel, BORDER)?;
    let inside = [left + edge, top + edge, panel[2] - edge, panel[3] - edge];
    let title = [inside[0], inside[1], inside[2], inside[1] + canvas.r(4.8)];
    canvas.fill(title, NEUTRAL.fill)?;
    canvas.specular(title, NEUTRAL.specular[0], NEUTRAL.specular[1])?;
    canvas.text_centred("Server sections", title, BODY, NEUTRAL.text, false)?;
    let body = [inside[0], title[3], inside[2], inside[3]];
    canvas.fill(body, NEUTRAL80.fill)?;
    let pad = canvas.r(1.6);
    let prefs = view.settings_options.server_list();
    let done_top = body[3] - pad - canvas.r(4.4);
    canvas.settings_scrollbars = true;
    let scroll = canvas.begin_scroll(SCROLL, [body[0], title[3], body[2], done_top])?;
    let content_top = title[3] - scroll.offset;
    let mut y = content_top + pad;
    for group in prefs.order() {
        let row = [body[0] + pad, y, body[2] - pad, y + canvas.r(4.4)];
        let gap = canvas.r(0.8);
        let down_left = row[2] - canvas.r(7.2);
        let up_left = down_left - gap - canvas.r(5.2);
        let show_left = up_left - gap - canvas.r(6.4);
        canvas.text_line_vertically_centred(
            group.label(),
            [row[0], row[1], show_left - gap, row[3]],
            CAPTION,
            TEXT,
        )?;
        control(
            canvas,
            view,
            [show_left, row[1], up_left - gap, row[3]],
            if prefs.visible(group) { "Hide" } else { "Show" },
            Some(ServerListAction::ToggleVisibility(group)),
        )?;
        control(
            canvas,
            view,
            [up_left, row[1], down_left - gap, row[3]],
            "Up",
            prefs.move_action(group, false),
        )?;
        control(
            canvas,
            view,
            [down_left, row[1], row[2], row[3]],
            "Down",
            prefs.move_action(group, true),
        )?;
        y = row[3] + canvas.r(0.8);
    }
    canvas.end_scroll(scroll, y - content_top)?;
    button(
        canvas,
        view,
        [
            body[0] + pad,
            done_top,
            body[2] - pad,
            done_top + canvas.r(4.4),
        ],
        Variant::Secondary,
        "Done",
        Some(MenuAction::DismissDialog),
    )
}

/// Disabled boundary moves have no hit target and cannot reorder past the list.
fn control(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    label: &str,
    action: Option<ServerListAction>,
) -> Result<(), UiPresentationError> {
    button(
        canvas,
        view,
        bounds,
        Variant::Secondary,
        label,
        action.map(MenuAction::ServerList),
    )
}

#[cfg(test)]
mod tests;
