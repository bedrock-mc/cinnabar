//! Inbox layout from the owner's vanilla inbox capture; date grouping is independent of read state.
use super::super::super::UiPresentationError;
use super::grid::{Grid, space};
use super::paint::Canvas;
use super::theme::{BODY, CAPTION, NEUTRAL, TEXT, TEXT_DIMMER};
use super::widgets::{header, panel, row, screen_overlay};
use crate::menu::{
    InboxItem, MenuAction, MenuScreen, MenuView,
    inbox::{self, Action, CATEGORIES},
};

mod detail;
mod icons;
use icons::{category_icon, filter_icon, trash_icon};

const UNREAD: [u8; 4] = [255, 128, 133, 255];

/// Draws category navigation and independently scrollable Recent/History groups.
pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
) -> Result<(), UiPresentationError> {
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
    let filter = [width - canvas.r(4.4), 0.0, width, canvas.r(4.4)];
    canvas.hit(MenuAction::Inbox(Action::Filters), filter)?;
    filter_icon(canvas, [filter[0] + canvas.r(1.2), canvas.r(1.2)])?;
    let top = header_bottom + space(canvas, 4);
    let bottom = height - space(canvas, 2);
    let grid = Grid::new(canvas.r(1.0), width);
    let (menu_span, list_span) = if grid.narrow {
        ((0, 2), (2, 6))
    } else {
        ((1, 3), (4, 7))
    };
    let [left, right] = grid.span(menu_span.0, menu_span.1);
    let pad = space(canvas, 4);
    let row_height = canvas.r(4.8);
    panel(
        canvas,
        [
            left,
            top,
            right,
            top + row_height * CATEGORIES.len() as f32 + pad * 2.0,
        ],
    )?;
    let state = &view.feeds.inbox_state;
    for (index, category) in CATEGORIES.iter().enumerate() {
        let y = top + pad + index as f32 * row_height;
        let bounds = [left, y, right, y + row_height];
        let action = MenuAction::Inbox(Action::Category(index));
        if state.category == index
            || view.hovered == Some(action)
            || view.focused_action == Some(action)
        {
            row(canvas, view, bounds, state.category == index, Some(action))?;
        } else {
            canvas.hit(action, bounds)?;
        }
        let text_y = y + (row_height - canvas.r(BODY.line)) * 0.5;
        category_icon(canvas, index, [left + pad, text_y])?;
        canvas.text_line(
            category,
            [left + pad + canvas.r(3.0), text_y],
            right - left - pad * 2.0 - canvas.r(6.0),
            BODY,
            TEXT,
        )?;
        let count = view
            .feeds
            .home
            .inbox_counts
            .get(&index)
            .copied()
            .unwrap_or_else(|| {
                view.feeds
                    .home
                    .inbox
                    .iter()
                    .filter(|item| {
                        item.unread && inbox::category_index(&item.category) == Some(index)
                    })
                    .count() as u32
            });
        if count > 0 {
            let count = count.to_string();
            let badge_width = canvas.measure(&count, CAPTION)? + canvas.r(0.8);
            let badge = [
                right - pad - badge_width,
                text_y,
                right - pad,
                text_y + canvas.r(2.0),
            ];
            canvas.fill(badge, UNREAD)?;
            canvas.text_centred(&count, badge, CAPTION, [30, 30, 31, 255], false)?;
        }
    }
    let [left, right] = grid.span(list_span.0, list_span.1);
    let mut list_top = top;
    if state.filters {
        for (label, action) in [
            ("Mark all as read", Action::MarkAllRead),
            ("Delete all read messages", Action::DeleteAllRead),
        ] {
            let bounds = [left, list_top, right, list_top + row_height];
            row(canvas, view, bounds, false, Some(MenuAction::Inbox(action)))?;
            canvas.text_centred(label, bounds, BODY, TEXT, false)?;
            list_top += row_height;
        }
    }
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
        .filter(|(_, item)| inbox::category_index(&item.category) == Some(state.category))
        .collect();
    items.sort_by(|(_, a), (_, b)| b.received.cmp(&a.received));
    for (label, recent) in [("Recent", true), ("History", false)] {
        let group: Vec<_> = items
            .iter()
            .copied()
            .filter(|(_, item)| {
                inbox::day(&item.received).is_none_or(|day| now - day <= 7) == recent
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
        canvas.text_centred(
            "No messages",
            [left, y, right, y + canvas.r(8.0)],
            BODY,
            TEXT,
            false,
        )?;
        y += canvas.r(8.0);
    }
    canvas.end_scroll(scroll, y - start)?;
    if state.delete_pending.is_some() {
        canvas.hits.clear();
        canvas.fill([0., 0., width, height], [0, 0, 0, 180])?;
        let b = [width * 0.25, height * 0.35, width * 0.75, height * 0.65];
        panel(canvas, b)?;
        canvas.text_centred(
            "Delete message?",
            [b[0], b[1], b[2], b[1] + row_height],
            BODY,
            TEXT,
            false,
        )?;
        for (i, label, action) in [
            (0, "Cancel", Action::Cancel),
            (1, "Delete", Action::ConfirmDelete),
        ] {
            let half = (b[2] - b[0]) * 0.5;
            let bounds = [
                b[0] + i as f32 * half,
                b[3] - row_height,
                b[0] + (i + 1) as f32 * half,
                b[3],
            ];
            row(canvas, view, bounds, false, Some(MenuAction::Inbox(action)))?;
            canvas.text_centred(label, bounds, BODY, TEXT, false)?;
        }
    }
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
    let date = inbox::date(&item.received);
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
