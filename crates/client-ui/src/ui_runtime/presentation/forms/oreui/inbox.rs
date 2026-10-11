//! Inbox layout from the owner's vanilla inbox capture; date grouping is independent of read state.
use super::super::super::UiPresentationError;
use super::super::menu_screens::Translate;
use super::grid::{Grid, space};
use super::motion::Surface;
use super::paint::Canvas;
use super::theme::{BODY, CAPTION, NEUTRAL, TEXT, TEXT_DIMMER};
use super::widgets::{header, row, screen_overlay};
use {
    launcher::menu::inbox::Action,
    launcher::menu::{InboxItem, MenuAction, MenuScreen, MenuView, inbox},
};

mod actions;
mod detail;
mod empty;
mod icons;
mod sidebar;
#[cfg(test)]
mod tests;
use icons::{filter_button, trash_icon};

const UNREAD: [u8; 4] = [255, 128, 133, 255];

/// Draws category navigation and independently scrollable Recent/History groups.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    translate: Translate<'_>,
) -> Result<(), UiPresentationError> {
    if view.feeds.inbox_state.filters {
        return actions::settings(canvas, view, size);
    }
    if let Some(item) =
        view.feeds.home.inbox.iter().find(|item| {
            view.feeds.inbox_state.opened.as_deref() == Some(item.instance_id.as_str())
        })
    {
        return detail::draw(canvas, view, item, size);
    }
    let [width, height] = size;
    screen_overlay(canvas, size)?;
    let header_bottom = header(
        canvas,
        view,
        "INBOX",
        width,
        Some(MenuAction::Navigate(MenuScreen::Home)),
    )?;
    filter_button(canvas, view, width)?;
    let top = header_bottom + space(canvas, 2);
    let bottom = height - space(canvas, 2);
    let grid = Grid::with_breakpoint(canvas.r(1.0), width, 102.0);
    let (menu_span, list_span) = if grid.narrow {
        ((0, 2), (2, 6))
    } else {
        ((1, 3), (4, 7))
    };
    let [left, right] = grid.span(menu_span.0, menu_span.1);
    sidebar::draw(canvas, view, [left, top, right, bottom], grid.narrow)?;
    let pad = space(canvas, 4);
    let state = &view.feeds.inbox_state;
    let [left, right] = grid.span(list_span.0, list_span.1);
    let list_top = top;
    let entrance = canvas.begin_entrance(Surface::Inbox(state.category as u8));
    let scroll = canvas.begin_scroll(
        &format!("inbox_messages_{}", state.category),
        [left, list_top, right, bottom],
    )?;
    let start = list_top - scroll.offset;
    let mut y = start;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |time| (time.as_secs() / 86_400) as i64);
    let mut items: Vec<_> = view
        .feeds
        .home
        .inbox
        .iter()
        .enumerate()
        .filter(|(_, item)| {
            launcher::menu::inbox::category_index(&item.category) == Some(state.category)
        })
        .collect();
    items.sort_by(|(_, a), (_, b)| b.received.cmp(&a.received));
    for (label, recent) in [("Recent", true), ("History", false)] {
        let group: Vec<_> = items
            .iter()
            .copied()
            .filter(|(_, item)| {
                launcher::menu::inbox::day(&item.received).is_none_or(|day| now - day <= 7)
                    == recent
            })
            .collect();
        if group.is_empty() {
            continue;
        }
        let label_width = canvas.measure(label, BODY)? + pad * 2.0;
        let tab = [left, y, left + label_width, y + canvas.r(2.8)];
        canvas.fill(tab, NEUTRAL.fill)?;
        canvas.text_centred(label, tab, BODY, TEXT, false)?;
        y = tab[3];
        for (index, item) in group {
            y = card(canvas, view, index, item, [left, right - canvas.r(1.2)], y)?;
        }
        y += canvas.r(4.0);
    }
    if items.is_empty() {
        y = empty::draw(canvas, state.category, [left, y, right, bottom], translate)?;
    }
    canvas.end_scroll(scroll, y - start)?;
    canvas.end_entrance(entrance, size)?;
    actions::delete_dialog(canvas, view, size)?;
    Ok(())
}

/// Title, source and date share a row with a separate delete target.
fn card(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    index: usize,
    item: &InboxItem,
    span: [f32; 2],
    top: f32,
) -> Result<f32, UiPresentationError> {
    let height = canvas.r(6.8);
    let bounds = [span[0], top, span[1], top + height];
    let delete_x = span[1] - height;
    row(
        canvas,
        view,
        [span[0], top, delete_x, top + height],
        false,
        Some(MenuAction::Inbox(Action::Open(index))),
    )?;
    let delete = [delete_x, top, span[1], top + height];
    row(
        canvas,
        view,
        delete,
        false,
        Some(MenuAction::Inbox(Action::Delete(index))),
    )?;
    trash_icon(
        canvas,
        [delete_x + height * 0.5 - canvas.r(0.8), top + canvas.r(1.2)],
    )?;
    canvas.text_centred(
        "Delete",
        [delete_x, top + canvas.r(4.1), span[1], top + height],
        CAPTION,
        TEXT,
        false,
    )?;
    let date = launcher::menu::inbox::date(&item.received);
    let date_width = canvas.measure(&date, BODY)?;
    let pad = canvas.r(1.2);
    let x = span[0] + canvas.r(3.6);
    let title_width = (delete_x - pad - date_width - pad - x).max(0.0);
    if item.unread {
        canvas.fill(
            [
                span[0] + pad,
                top + height * 0.5 - canvas.r(0.4),
                span[0] + pad + canvas.r(0.8),
                top + height * 0.5 + canvas.r(0.4),
            ],
            UNREAD,
        )?;
    }
    canvas.text_line(
        &item.header,
        [x, top + canvas.r(1.3)],
        title_width,
        BODY,
        TEXT,
    )?;
    canvas.text_line(
        if item.source.is_empty() {
            "Minecraft"
        } else {
            &item.source
        },
        [x, top + canvas.r(3.4)],
        title_width,
        CAPTION,
        TEXT_DIMMER,
    )?;
    canvas.text_line(
        &date,
        [delete_x - pad - date_width, top + canvas.r(2.5)],
        date_width + 1.0,
        BODY,
        TEXT_DIMMER,
    )?;
    Ok(bounds[3])
}
