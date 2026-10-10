//! Group headers retain visibility and order preferences independently of the picked server.

use super::super::motion::{Feedback, Kind, opacity};
use crate::ui_runtime::oreui_assets::CHEVRON_DOWN_IMAGE;
use launcher::menu::server_list::{ServerGroup, ServerListAction};
use {
    super::*,
    launcher::menu::{MenuAction, MenuView},
    ui::IconRef,
};

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    span: [f32; 2],
    viewport: [f32; 2],
    mut y: f32,
    selected: Option<Selection>,
    images: &HashMap<String, IconRef>,
) -> Result<f32, UiPresentationError> {
    let groups: Vec<_> = view
        .settings_options
        .server_list()
        .order()
        .into_iter()
        .map(|group| (group, entries(view, group)))
        .filter(|(_, entries)| !entries.is_empty())
        .collect();
    if let Some(transitions) = canvas.transitions.as_deref_mut() {
        transitions
            .server_list
            .begin_layout([span[0], viewport[0], span[1], viewport[1]]);
    }
    for (group, entries) in &groups {
        let collapsed = view.settings_options.server_list().collapsed(*group);
        let header_bounds = [span[0], y + canvas.r(1.6), span[1], y + canvas.r(4.8)];
        let progress = header(
            canvas,
            view,
            *group,
            entries.len(),
            [span[0], y, span[1], y + canvas.r(4.8)],
        )?;
        y += canvas.r(4.8);
        let total: f32 = entries
            .iter()
            .map(|&index| row_height(canvas, view, *group, index, images))
            .sum();
        let bottom = y + total * progress;
        let clip_top = y.max(viewport[0]);
        let clip_bottom = bottom.min(viewport[1]);
        if progress > 0.0 && clip_bottom > clip_top {
            let clip = canvas.begin_clip([span[0], clip_top, span[1], clip_bottom])?;
            let alpha = canvas.alpha;
            canvas.alpha *= progress;
            let first_hit = canvas.hits.len();
            let first_focus = canvas.focus_targets.len();
            let first_focus_hit = canvas.focus_hits.len();
            let mut row_top = y;
            for &index in entries {
                let height = row_height(canvas, view, *group, index, images);
                if row_top + height > clip_top && row_top < clip_bottom {
                    entry(
                        canvas,
                        view,
                        *group,
                        index,
                        [span[0], row_top, span[1], row_top + height],
                        selected,
                        images,
                    )?;
                }
                row_top += height;
            }
            if collapsed {
                canvas.hits.truncate(first_hit);
                canvas.focus_targets.truncate(first_focus);
                canvas.focus_hits.truncate(first_focus_hit);
            }
            canvas.alpha = alpha;
            canvas.end_clip(clip);
        }
        if let Some(transitions) = canvas.transitions.as_deref_mut() {
            transitions
                .server_list
                .section(*group, header_bounds, bottom);
        }
        y = bottom;
    }
    if let Some(preview) = canvas
        .transitions
        .as_deref()
        .and_then(|t| t.server_list.preview())
    {
        if let Some(marker) = preview.marker {
            canvas.fill(
                [
                    span[0],
                    marker - canvas.r(0.1),
                    span[1],
                    marker + canvas.r(0.1),
                ],
                TEXT_DIMMER,
            )?;
        }
        let count = groups
            .iter()
            .find(|(group, _)| *group == preview.group)
            .map_or(0, |(_, entries)| entries.len());
        let b = preview.bounds;
        canvas.fill(
            [
                b[0] + canvas.r(0.2),
                b[1] + canvas.r(0.4),
                b[2],
                b[3] + canvas.r(0.4),
            ],
            [0, 0, 0, 90],
        )?;
        canvas.fill(b, NEUTRAL80.hovered)?;
        canvas.frame(b, super::super::theme::EDGE, super::super::theme::BORDER)?;
        let expanded =
            u8::from(!view.settings_options.server_list().collapsed(preview.group)) as f32;
        header_label(canvas, preview.group, count, b, expanded)?;
    }
    Ok(y)
}

fn entries(view: &MenuView, group: ServerGroup) -> Vec<usize> {
    match group {
        ServerGroup::Featured => group_entries(view, "featured"),
        ServerGroup::Creator => group_entries(view, "creator"),
        ServerGroup::Saved => (0..view.servers.len()).collect(),
    }
}

fn row_height(
    canvas: &Canvas<'_>,
    view: &MenuView,
    group: ServerGroup,
    index: usize,
    images: &HashMap<String, IconRef>,
) -> f32 {
    let tall = group == ServerGroup::Saved || {
        let server = &view.featured[index];
        images.contains_key(&server.image_path) || !server_caption(view, server).is_empty()
    };
    canvas.r(if tall { 6.0 } else { 4.0 })
}

fn entry(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    group: ServerGroup,
    index: usize,
    b: Bounds,
    selected: Option<Selection>,
    images: &HashMap<String, IconRef>,
) -> Result<(), UiPresentationError> {
    let pad = canvas.r(1.6);
    if group == ServerGroup::Saved {
        let server = &view.servers[index];
        server_row(
            canvas,
            view,
            b,
            selected == Some(Selection::Saved(index)),
            MenuAction::SelectSaved(index),
        )?;
        let caption = view
            .feeds
            .pings
            .get(&server.address)
            .filter(|p| !p.motd.is_empty())
            .map_or(server.address.as_str(), |p| p.motd.as_str());
        return server_text(
            canvas,
            [b[0] + pad, b[1], b[2] - pad, b[3]],
            &server.name,
            caption,
        );
    }
    let server = &view.featured[index];
    server_row(
        canvas,
        view,
        b,
        selected == Some(Selection::Featured(index)),
        MenuAction::SelectFeatured(index),
    )?;
    let mut text_left = b[0] + pad;
    if let Some(icon) = images.get(&server.image_path) {
        let side = 40.0 * super::super::icons::native_scale(canvas);
        let top = (b[1] + b[3] - side) * 0.5;
        let icon_bounds = [text_left, top, text_left + side, top + side];
        canvas.icon_ref(*icon, icon_bounds)?;
        canvas.frame(
            icon_bounds,
            super::super::theme::EDGE,
            super::super::theme::BORDER,
        )?;
        if let Some(frame) = canvas
            .transitions
            .as_deref()
            .and_then(|t| t.server_icon_frame(index))
        {
            canvas.sprite_frame(
                crate::ui_runtime::oreui_assets::SETTINGS_ICON_HIGHLIGHT_IMAGE,
                icon_bounds,
                [255; 4],
                frame,
                super::super::transitions::ICON_HIGHLIGHT_FRAMES,
            )?;
        }
        text_left = icon_bounds[2] + canvas.r(0.8);
    }
    server_text(
        canvas,
        [text_left, b[1], b[2] - pad, b[3]],
        &server.name,
        &server_caption(view, server),
    )
}

fn header(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    group: ServerGroup,
    count: usize,
    b: Bounds,
) -> Result<f32, UiPresentationError> {
    let action = MenuAction::ServerList(ServerListAction::Toggle(group));
    let state = canvas.interaction(view, Some(action));
    let expanded = !view.settings_options.server_list().collapsed(group);
    let motion = canvas.feedback(state, true, expanded, Kind::Disclosure);
    let row = [b[0], b[1] + canvas.r(1.6), b[2], b[3]];
    let alpha = canvas.alpha;
    if canvas
        .transitions
        .as_deref()
        .and_then(|t| t.server_list.dragged_group())
        == Some(group)
    {
        canvas.alpha *= 0.35;
    }
    super::super::sidebar::transparent_background(
        canvas,
        row,
        Feedback {
            selected: 0.0,
            ..motion
        },
    )?;
    if motion.focus > 0.0 {
        canvas.frame(
            row,
            super::super::theme::EDGE,
            opacity(super::super::theme::OUTLINE, motion.focus),
        )?;
    }
    header_label(canvas, group, count, row, motion.selected)?;
    canvas.alpha = alpha;
    canvas.hit(action, row)?;
    divider(
        canvas,
        b[0],
        b[2],
        b[3] - canvas.r(super::super::theme::EDGE),
    )?;
    Ok(motion.selected)
}

fn header_label(
    canvas: &mut Canvas<'_>,
    group: ServerGroup,
    count: usize,
    b: Bounds,
    expanded: f32,
) -> Result<(), UiPresentationError> {
    let pad = canvas.r(1.6);
    let side = canvas.r(1.2);
    let top = (b[1] + b[3] - side) * 0.5;
    let icon = [b[2] - pad - side, top, b[2] - pad, top + side];
    canvas.text_line_vertically_centred(
        &format!("{} ({count})", group.label()),
        [b[0] + pad, b[1], icon[0] - canvas.r(0.8), b[3]],
        CAPTION,
        TEXT_DIMMER,
    )?;
    let angle = -std::f32::consts::FRAC_PI_2 * (1.0 - expanded);
    if !canvas.rotated_masked_sprite(CHEVRON_DOWN_IMAGE, icon, TEXT_DIMMER, angle)? {
        canvas.text_centred(
            if expanded > 0.5 { "⌄" } else { "›" },
            icon,
            CAPTION,
            TEXT_DIMMER,
            false,
        )?;
    }
    Ok(())
}
