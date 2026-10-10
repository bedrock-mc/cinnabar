//! Selected server content scrolls independently and ends with its last section.

use super::*;
use crate::ui_runtime::oreui_assets::{SERVER_PING_IMAGES, SERVER_PLAYERS_IMAGE};

pub(super) fn details(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    server: &MenuServerCard,
    index: usize,
    b: Bounds,
    images: &HashMap<String, IconRef>,
) -> Result<(), UiPresentationError> {
    let scroll = canvas.begin_scroll(&format!("servers.details.{index}"), b)?;
    let top = b[1] - scroll.offset;
    let right = b[2] - canvas.r(1.6);
    let bottom = details_content(canvas, view, server, index, [b[0], top, right], images)?;
    canvas.frame(
        [b[0], top, right, bottom],
        super::super::theme::EDGE,
        super::super::theme::BORDER,
    )?;
    canvas.end_scroll(scroll, bottom - top)
}

pub(super) fn details_content(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    server: &MenuServerCard,
    index: usize,
    [left, top, right]: [f32; 3],
    images: &HashMap<String, IconRef>,
) -> Result<f32, UiPresentationError> {
    let details = view.feeds.details.get(&server.address);
    let banner = [left, top, right, top + (right - left) * 0.3];
    let art = details.and_then(|details| images.get(&details.banner));
    match art {
        Some(icon) => canvas.icon_ref(*icon, banner)?,
        None => canvas.fill(banner, NEUTRAL100)?,
    }
    if pingable(&server.address) {
        ping_strip(
            canvas,
            banner,
            view.feeds.pings.get(&server.address),
            view.settings_options.exact_server_ping(),
        )?;
    } else if let Some(count) = details
        .and_then(|details| details.player_count)
        .filter(|count| *count > 0)
    {
        let overlay = status_overlay(canvas, banner)?;
        let x = overlay[0] + canvas.r(2.4);
        player_count(canvas, overlay, x, &count.to_string())?;
    }
    let pad = canvas.r(2.4);
    let play_height = canvas.r(4.4);
    let play_width = canvas.r(32.0).min((right - left - pad * 2.0) * 0.5);
    let row_bottom = banner[3] + play_height + canvas.r(2.4);
    canvas.fill([left, banner[3], right, row_bottom], NEUTRAL80.fill)?;
    let name_bounds = [
        left + pad,
        banner[3],
        right - pad * 2.0 - play_width,
        row_bottom,
    ];
    canvas.text_line_vertically_centred(&server.name, name_bounds, BODY, TEXT)?;
    button(
        canvas,
        view,
        [
            right - pad - play_width,
            banner[3] + canvas.r(1.2),
            right - pad,
            row_bottom - canvas.r(1.2),
        ],
        Variant::Hero,
        "Play",
        play_featured(view, index),
    )?;
    let mut y = row_bottom;
    let Some(details) = details else {
        return Ok(y);
    };
    if !details.description.is_empty() {
        y = paragraph(
            canvas,
            [left, y, right],
            "Description",
            "",
            &details.description,
        )?;
    }
    if !details.games.is_empty() {
        let section_top = y;
        let first = canvas.nodes.len();
        canvas.fill([left, y, right, y + canvas.r(2.4)], NEUTRAL80.fill)?;
        divider(canvas, left, right, y)?;
        y += canvas.r(2.4);
        y += canvas.text(
            "Activities",
            [left + pad, y],
            right - left - pad * 2.0,
            BODY,
            TEXT,
            false,
        )? + canvas.r(1.2);
        for game in &details.games {
            let start = y;
            let image_side = canvas.r(15.2).min((right - left - pad * 2.0) * 0.35);
            let art = images.get(&game.image_path);
            let text_left = if art.is_some() {
                left + pad + image_side + canvas.r(1.2)
            } else {
                left + pad
            };
            let width = right - pad - text_left;
            let mut line = y + canvas.r(1.2);
            line += canvas.text(&game.title, [text_left, line], width, BODY, TEXT, false)?;
            if !game.subtitle.is_empty() {
                line += canvas.text(
                    &game.subtitle,
                    [text_left, line],
                    width,
                    CAPTION,
                    TEXT_DIMMER,
                    false,
                )?;
            }
            line += canvas.r(0.8);
            line += canvas.text(
                &game.description,
                [text_left, line],
                width,
                CAPTION,
                TEXT_DIMMER,
                false,
            )?;
            if let Some(icon) = art {
                let b = [
                    left + pad,
                    start + canvas.r(1.2),
                    left + pad + image_side,
                    start + canvas.r(1.2) + image_side,
                ];
                canvas.icon_ref(*icon, b)?;
                canvas.frame(b, super::super::theme::EDGE, super::super::theme::BORDER)?;
                line = line.max(b[3]);
            }
            y = line + canvas.r(1.2);
        }
        extend_fill(canvas, first, [left, section_top, right, y])?;
    }
    if !details.news_title.is_empty() || !details.news.is_empty() {
        y = paragraph(
            canvas,
            [left, y, right],
            "News",
            &details.news_title,
            &details.news,
        )?;
    }
    Ok(y)
}

fn paragraph(
    canvas: &mut Canvas<'_>,
    [left, top, right]: [f32; 3],
    title: &str,
    subtitle: &str,
    text: &str,
) -> Result<f32, UiPresentationError> {
    let first = canvas.nodes.len();
    canvas.fill([left, top, right, top + canvas.r(1.2)], NEUTRAL80.fill)?;
    divider(canvas, left, right, top)?;
    let pad = canvas.r(2.4);
    let width = right - left - pad * 2.0;
    let mut y = top + canvas.r(1.2);
    y += canvas.text(title, [left + pad, y], width, BODY, TEXT, false)? + canvas.r(0.8);
    if !subtitle.is_empty() {
        y += canvas.text(
            subtitle,
            [left + pad, y],
            width,
            CAPTION,
            TEXT_DIMMER,
            false,
        )? + canvas.r(1.2);
    }
    y += canvas.text(text, [left + pad, y], width, CAPTION, TEXT_DIMMER, false)? + canvas.r(1.2);
    extend_fill(canvas, first, [left, top, right, y])?;
    Ok(y)
}

fn extend_fill(
    canvas: &mut Canvas<'_>,
    first: usize,
    bounds: Bounds,
) -> Result<(), UiPresentationError> {
    let node = &canvas.nodes[first];
    let b = node.bounds();
    let rect = super::super::super::super::rect(
        b.min().x(),
        b.min().y(),
        b.min().x() + bounds[2] - bounds[0],
        b.min().y() + bounds[3] - bounds[1],
    )?;
    canvas.nodes[first] = node.clone().with_bounds(rect);
    Ok(())
}

fn ping_strip(
    canvas: &mut Canvas<'_>,
    banner: Bounds,
    ping: Option<&PingInfo>,
    exact: bool,
) -> Result<(), UiPresentationError> {
    let overlay = status_overlay(canvas, banner)?;
    let icon_side = 20.0 * super::super::icons::native_scale(canvas);
    let x = overlay[0] + canvas.r(2.4);
    let line = overlay[1] + (canvas.r(6.0) - canvas.r(BODY.line)) * 0.5;
    let icon_top = overlay[1] + (canvas.r(6.0) - icon_side) * 0.5;
    if let Some(ping) = ping.filter(|p| p.online) {
        let tier = if ping.ping_ms < 80 {
            0
        } else if ping.ping_ms < 160 {
            1
        } else {
            2
        };
        canvas.sprite(
            SERVER_PING_IMAGES[tier],
            [x, icon_top, x + icon_side, icon_top + icon_side],
            [255; 4],
        )?;
    } else {
        let frame = ((canvas.seconds.max(0.0) * 1000.0 / 700.0 * 6.0) as u16) % 6;
        canvas.sprite_frame(
            SERVER_PING_IMAGES[3],
            [x, icon_top, x + icon_side, icon_top + icon_side],
            [255; 4],
            frame,
            6,
        )?;
    }
    let label = ping_label(ping, exact);
    let label_x = x + icon_side + canvas.r(0.4);
    canvas.text_line(
        &label,
        [label_x, line],
        overlay[2] - label_x,
        BODY,
        TEXT_DIMMER,
    )?;
    let players_x = label_x + canvas.measure(&label, BODY)? + canvas.r(2.4);
    let count = ping
        .filter(|p| p.online)
        .map_or(0, |p| p.players)
        .to_string();
    player_count(canvas, overlay, players_x, &count)
}

fn status_overlay(canvas: &mut Canvas<'_>, banner: Bounds) -> Result<Bounds, UiPresentationError> {
    let overlay = [banner[0], banner[3] - canvas.r(6.0), banner[2], banner[3]];
    canvas.fill(overlay, [0, 0, 0, 179])?;
    Ok(overlay)
}

fn player_count(
    canvas: &mut Canvas<'_>,
    overlay: Bounds,
    players_x: f32,
    count: &str,
) -> Result<(), UiPresentationError> {
    let player_side = canvas.r(2.4);
    let player_top = overlay[1] + (canvas.r(6.0) - player_side) * 0.5;
    canvas.sprite(
        SERVER_PLAYERS_IMAGE,
        [
            players_x,
            player_top,
            players_x + player_side,
            player_top + player_side,
        ],
        [255; 4],
    )?;
    let count_x = players_x + player_side + canvas.r(0.4);
    let line = overlay[1] + (canvas.r(6.0) - canvas.r(BODY.line)) * 0.5;
    canvas.text_line(
        count,
        [count_x, line],
        (overlay[2] - count_x).max(1.0),
        BODY,
        TEXT_DIMMER,
    )?;
    Ok(())
}

pub(super) fn saved_details(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    index: usize,
    b: Bounds,
) -> Result<(), UiPresentationError> {
    let server = &view.servers[index];
    let pad = canvas.r(2.4);
    let right = b[2] - canvas.r(1.6);
    let bottom = b[1] + canvas.r(23.6);
    canvas.fill([b[0], b[1], right, bottom], NEUTRAL80.fill)?;
    canvas.frame(
        [b[0], b[1], right, bottom],
        super::super::theme::EDGE,
        super::super::theme::BORDER,
    )?;
    let strip = [b[0], b[1], right, b[1] + canvas.r(6.0)];
    ping_strip(
        canvas,
        strip,
        view.feeds.pings.get(&server.address),
        view.settings_options.exact_server_ping(),
    )?;
    let y = strip[3] + canvas.r(1.2);
    canvas.text_line(
        &server.name,
        [b[0] + pad, y],
        right - b[0] - pad * 2.0,
        BODY,
        TEXT,
    )?;
    canvas.text_line(
        &server.address,
        [b[0] + pad, y + canvas.r(2.4)],
        right - b[0] - pad * 2.0,
        CAPTION,
        TEXT_DIMMER,
    )?;
    let gap = canvas.r(0.8);
    let left = b[0] + pad;
    let width = right - pad - left;
    let play_y = y + canvas.r(5.6);
    button(
        canvas,
        view,
        [left, play_y, right - pad, play_y + canvas.r(4.4)],
        Variant::Hero,
        "Play",
        Some(MenuAction::PlaySaved(index)),
    )?;
    let edit_y = play_y + canvas.r(5.2);
    button(
        canvas,
        view,
        [
            left,
            edit_y,
            left + (width - gap) * 0.5,
            edit_y + canvas.r(4.4),
        ],
        Variant::Secondary,
        "Edit",
        Some(MenuAction::EditSaved(index)),
    )?;
    button(
        canvas,
        view,
        [
            left + (width + gap) * 0.5,
            edit_y,
            right - pad,
            edit_y + canvas.r(4.4),
        ],
        Variant::Secondary,
        "Remove",
        Some(MenuAction::RemoveSavedDialog(index)),
    )
}
