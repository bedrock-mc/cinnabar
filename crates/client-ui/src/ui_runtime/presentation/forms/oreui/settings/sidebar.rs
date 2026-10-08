//! Flat category rows and group dividers in the independently scrolling settings sidebar.

use super::super::super::super::UiPresentationError;
use super::super::super::menu_screens::Translate;
use super::super::focus;
use super::super::motion::Kind;
use super::super::paint::{Bounds, Canvas};
use super::super::sidebar::background as row_background;
use super::super::theme::{self, BODY, CAPTION, EDGE, NEUTRAL80, TEXT, TEXT_DIMMER};
use super::{section_index, sections};
use crate::menu::view::SettingsFocusAxis;
use crate::menu::{MenuAction, MenuView};
use crate::ui_runtime::oreui_assets::{SETTINGS_ICON_HIGHLIGHT_IMAGE, SETTINGS_ICONS};
use crate::ui_runtime::presentation::IconRef;

const TABS: [(&str, &str); 14] = [
    ("accessibility_forced_index", "menu.accessibility.tab.title"),
    (
        "keyboard_and_mouse_forced_index",
        "menu.keyboardAndMouse.tab.title",
    ),
    (
        "controller_and_switch_forced_index",
        "menu.controller.tab.title",
    ),
    ("touch_forced_index", "menu.touch.tab.title"),
    ("party_forced_index", "options.party"),
    ("general_forced_index", "menu.general.tab.title"),
    ("video_forced_index", "menu.video.tab.title"),
    ("sound_forced_index", "menu.audio.tab.title"),
    ("account_forced_index", "menu.account.tab.title"),
    (
        "view_subscriptions_forced_index",
        "options.viewSubscriptions",
    ),
    ("global_texture_pack_forced_index", "menu.globalpacks"),
    ("storage_management_forced_index", "menu.storage.tab.title"),
    ("language_forced_index", "menu.language.tab.title"),
    ("creator_forced_index", "menu.creator.tab.title"),
];

#[cfg(test)]
mod tests;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    b: Bounds,
    translate: Translate<'_>,
    gamerpic: Option<IconRef>,
) -> Result<(), UiPresentationError> {
    let panel = [b[0], b[1], b[2] - canvas.r(1.6), b[3]];
    canvas.fill(panel, NEUTRAL80.fill)?;
    canvas.frame(panel, EDGE, theme::BORDER)?;
    let edge = canvas.r(EDGE);
    let viewport = [b[0] + edge, b[1] + edge, b[2] - edge, b[3]];
    let sidebar_focus = canvas.begin_focus_region(focus::SIDEBAR, b, None, false)?;
    let sidebar_scroll_focus = canvas.begin_focus_region(
        focus::SIDEBAR_SCROLL,
        viewport,
        Some(SettingsFocusAxis::Vertical),
        true,
    )?;
    canvas.focus_delegate(
        Some(MenuAction::SettingsSection(if view.settings_section == 0 {
            section_index(TABS[0].0)
        } else {
            view.settings_section
        })),
        None,
    );
    let right = viewport[2] - canvas.r(1.6);
    let width = (right - viewport[0] - canvas.r(6.4)).max(1.0);
    let labels: Vec<_> = TABS
        .iter()
        .map(|(_, key)| {
            translate(key).map_or_else(
                || sections::fallback(key).to_owned(),
                |text| text.to_string(),
            )
        })
        .collect();
    let heights: Vec<_> = labels
        .iter()
        .map(|label| {
            canvas
                .measure_height(label, width, BODY)
                .map(|height| canvas.r(4.8).max(height + canvas.r(1.6)))
        })
        .collect::<Result<_, _>>()?;
    let content = heights.iter().sum::<f32>() + canvas.r(1.6 + 3.0 * (4.8 + EDGE));
    let max = (content - (viewport[3] - viewport[1])).max(0.0);
    let key = "oreui_settings_sidebar";
    if let Some(offset) = canvas.offsets.get_mut(key) {
        *offset = offset.min(max);
    }
    let scroll = canvas.begin_scroll(key, viewport)?;
    let mut y = viewport[1] + canvas.r(1.6) - scroll.offset;
    let mut focused_outline = None;
    for (index, ((selector, _), label)) in TABS.iter().zip(&labels).enumerate() {
        let group = match index {
            1 => Some("options.group.input"),
            4 => Some("options.social"),
            5 => Some("options.general"),
            _ => None,
        };
        if let Some(group) = group {
            let text = translate(group).map_or_else(
                || sections::fallback(group).to_owned(),
                |text| text.to_string(),
            );
            y = group_label(canvas, &text, [viewport[0], right], y)?;
        }
        let action = MenuAction::SettingsSection(section_index(selector));
        let mut state = canvas.interaction(view, Some(action));
        state.pressed &= state.hovered;
        let selected = view.settings_section == section_index(selector)
            || view.settings_section == 0 && index == 0;
        let row = [viewport[0], y, right, y + heights[index]];
        let motion = canvas.feedback(state, true, selected, Kind::Surface);
        row_background(canvas, row, motion)?;
        if state.focused {
            let overlap = canvas.r(if selected || state.hovered { 0.4 } else { EDGE });
            focused_outline = Some([row[0], row[1] - overlap, row[2], row[3] + overlap]);
        }
        let side = super::super::icons::native_side(canvas);
        let at = [row[0] + canvas.r(1.6), (row[1] + row[3] - side) * 0.5];
        let icon_bounds = [at[0], at[1], at[0] + side, at[1] + side];
        if index != 8 || !super::account_icon::draw(canvas, view, icon_bounds, gamerpic)? {
            icon(canvas, index, at)?;
        }
        if let Some(frame) = canvas
            .transitions
            .as_deref()
            .and_then(|transitions| transitions.icon_frame(section_index(selector)))
        {
            canvas.sprite_frame(
                SETTINGS_ICON_HIGHLIGHT_IMAGE,
                [at[0], at[1], at[0] + side, at[1] + side],
                [255; 4],
                frame,
                super::super::transitions::ICON_HIGHLIGHT_FRAMES,
            )?;
        }
        let left = at[0] + side + canvas.r(0.8);
        let text_height = canvas.measure_height(label, width, BODY)?;
        canvas.text(
            label,
            [left, (row[1] + row[3] - text_height) * 0.5],
            width,
            BODY,
            TEXT,
            false,
        )?;
        canvas.hit(action, row)?;
        y = row[3];
    }
    if let Some(bounds) = focused_outline {
        canvas.frame(bounds, EDGE, theme::OUTLINE)?;
    }
    canvas.end_scroll(scroll, content)?;
    canvas.end_focus_region(sidebar_scroll_focus);
    canvas.end_focus_region(sidebar_focus);
    Ok(())
}

fn group_label(
    canvas: &mut Canvas<'_>,
    label: &str,
    span: [f32; 2],
    top: f32,
) -> Result<f32, UiPresentationError> {
    let bottom = top + canvas.r(4.8);
    let edge = canvas.r(EDGE);
    let pad = canvas.r(1.6);
    canvas.text_line(
        label,
        [span[0] + pad, bottom - canvas.r(0.8 + CAPTION.line)],
        (span[1] - span[0] - pad * 2.0).max(1.0),
        CAPTION,
        TEXT_DIMMER,
    )?;
    canvas.fill(
        [span[0], bottom - edge, span[1], bottom],
        NEUTRAL80.specular[1],
    )?;
    canvas.fill(
        [span[0], bottom, span[1], bottom + edge],
        NEUTRAL80.specular[0],
    )?;
    Ok(bottom + edge)
}

fn icon(canvas: &mut Canvas<'_>, index: usize, at: [f32; 2]) -> Result<(), UiPresentationError> {
    let side = super::super::icons::native_side(canvas);
    let b = [at[0], at[1], at[0] + side, at[1] + side];
    if canvas.sprite(SETTINGS_ICONS[index], b, [255; 4])? {
        return Ok(());
    }
    let pixel = side / 16.0;
    let mut block = |x: f32, y: f32, w: f32, h: f32, color| {
        canvas.fill(
            [
                at[0] + x * pixel,
                at[1] + y * pixel,
                at[0] + (x + w) * pixel,
                at[1] + (y + h) * pixel,
            ],
            color,
        )
    };
    block(0.0, 0.0, 16.0, 16.0, theme::BORDER)?;
    let colors = [
        [65, 99, 216, 255],
        [147, 145, 132, 255],
        [147, 145, 132, 255],
        [147, 145, 132, 255],
        [158, 101, 64, 255],
        [152, 100, 50, 255],
        [193, 142, 62, 255],
        [141, 90, 59, 255],
        [121, 76, 45, 255],
        [151, 116, 27, 255],
        [159, 108, 44, 255],
        [141, 126, 73, 255],
        [55, 110, 187, 255],
        [159, 105, 65, 255],
    ];
    block(1.0, 1.0, 14.0, 14.0, colors[index])?;
    match index {
        0 => {
            block(5.0, 2.0, 6.0, 5.0, TEXT)?;
            block(3.0, 8.0, 10.0, 2.0, TEXT)?;
            block(7.0, 8.0, 2.0, 6.0, TEXT)?;
            block(4.0, 11.0, 2.0, 3.0, TEXT)?;
            block(10.0, 11.0, 2.0, 3.0, TEXT)?;
        }
        1 => {
            block(5.0, 2.0, 6.0, 12.0, theme::BORDER)?;
            block(6.0, 3.0, 4.0, 8.0, [208, 209, 212, 255])?;
            block(7.0, 3.0, 2.0, 3.0, colors[index])?;
        }
        2 => {
            block(3.0, 4.0, 10.0, 8.0, theme::BORDER)?;
            block(4.0, 5.0, 8.0, 6.0, [178, 175, 160, 255])?;
            block(5.0, 6.0, 1.0, 3.0, theme::BORDER)?;
            block(4.0, 7.0, 3.0, 1.0, theme::BORDER)?;
            block(10.0, 6.0, 1.0, 1.0, theme::BORDER)?;
            block(11.0, 8.0, 1.0, 1.0, theme::BORDER)?;
        }
        6 => {
            block(2.0, 2.0, 12.0, 11.0, [99, 169, 218, 255])?;
            block(3.0, 3.0, 4.0, 4.0, [255, 230, 122, 255])?;
            block(3.0, 10.0, 10.0, 3.0, [97, 141, 64, 255])?;
            block(7.0, 7.0, 3.0, 5.0, [97, 141, 64, 255])?;
        }
        8 => {
            block(3.0, 2.0, 10.0, 5.0, [64, 44, 35, 255])?;
            block(3.0, 7.0, 10.0, 7.0, [178, 129, 94, 255])?;
            block(4.0, 8.0, 3.0, 2.0, TEXT)?;
            block(9.0, 8.0, 3.0, 2.0, TEXT)?;
            block(5.0, 8.0, 1.0, 2.0, [63, 90, 174, 255])?;
            block(10.0, 8.0, 1.0, 2.0, [63, 90, 174, 255])?;
            block(5.0, 12.0, 6.0, 2.0, [74, 45, 30, 255])?;
        }
        9 => {
            block(2.0, 4.0, 12.0, 3.0, [159, 46, 34, 255])?;
            block(2.0, 9.0, 12.0, 2.0, [218, 182, 59, 255])?;
        }
        10 => {
            block(2.0, 7.0, 12.0, 2.0, theme::BORDER)?;
            block(7.0, 6.0, 2.0, 5.0, [216, 208, 177, 255])?;
        }
        11 => {
            for row in [3.0, 7.0, 11.0] {
                block(2.0, row, 12.0, 2.0, [185, 166, 98, 255])?;
            }
        }
        _ => {
            for row in 0..3 {
                for column in 0..3 {
                    block(
                        3.0 + column as f32 * 4.0,
                        3.0 + row as f32 * 4.0,
                        2.0,
                        2.0,
                        theme::BORDER,
                    )?;
                }
            }
        }
    }
    Ok(())
}
