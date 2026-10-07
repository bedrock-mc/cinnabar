//! Long setting enumerations use the shared modal menu and commit one selected value.

use std::borrow::Cow;

use super::super::super::super::UiPresentationError;
use super::super::super::menu_screens::Translate;
use super::super::modal::{self, Modal};
use super::super::paint::{Bounds, Canvas};
use super::super::theme::{self, BODY, EDGE, TEXT};
use super::super::widgets::MenuItem;
use super::{button, gui_scale, sections};
use crate::menu::{
    MenuAction, MenuView,
    settings_options::{SETTINGS_OPTIONS, SettingKind},
};
use crate::ui_runtime::oreui_assets::CHEVRON_DOWN_IMAGE;

pub(super) const SELECT_HEIGHT: f32 = 4.6;

#[cfg(test)]
mod tests;

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    translate: Translate<'_>,
) -> Result<(), UiPresentationError> {
    if view.settings_scale_picker {
        return gui_scale::draw_picker(canvas, view, size, translate);
    }
    let Some(index) = view.settings_dropdown else {
        return Ok(());
    };
    let Some(option) = SETTINGS_OPTIONS.get(usize::from(index)) else {
        return Ok(());
    };
    let SettingKind::Dropdown(choices) = option.kind else {
        return Ok(());
    };
    let word = |key: &str| {
        translate(key).map_or_else(
            || sections::fallback(key).to_owned(),
            |text| text.to_string(),
        )
    };
    let title = word(sections::option_label(option.name, option.label));
    let labels: Vec<_> = choices.iter().map(|choice| word(choice.label)).collect();
    let value = view.settings_options.get(usize::from(index));
    let actions = (0..labels.len())
        .map(|choice| MenuAction::SettingsOption(index, choice as i32))
        .collect();
    draw_choices(
        canvas,
        view,
        size,
        Picker {
            title,
            labels,
            actions,
            selected: value as usize,
            close: MenuAction::SettingsDropdown(index),
            scroll_key: format!("oreui_settings_picker/{index}"),
        },
    )
}

pub(super) struct Picker {
    pub title: String,
    pub labels: Vec<String>,
    pub actions: Vec<MenuAction>,
    pub selected: usize,
    pub close: MenuAction,
    pub scroll_key: String,
}

pub(super) fn draw_choices(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    picker: Picker,
) -> Result<(), UiPresentationError> {
    let modal = Modal {
        title: &picker.title,
        items: picker
            .labels
            .iter()
            .enumerate()
            .map(|(choice, label)| MenuItem {
                label,
                picture_slot: false,
                picture: None,
                selected: choice == picker.selected,
                enabled: true,
                action: picker.actions.get(choice).copied(),
            })
            .collect(),
        body: Cow::Borrowed(""),
        body_color: TEXT,
        buttons: Vec::new(),
        close: Some(picker.close),
    };
    canvas.hits.clear();
    canvas.clear_focus_geometry();
    canvas.slider_tracks.clear();
    canvas.scrolls.clear();
    canvas.spots.clear();
    canvas.settings_scrollbars = false;
    let scroll_key = picker.scroll_key;
    if let Some(offset) = canvas.offsets.get(&scroll_key).copied() {
        canvas.offsets.insert(modal::SCROLL.to_owned(), offset);
    } else {
        canvas.offsets.remove(modal::SCROLL);
    }
    modal::draw_picker(canvas, view, size, &modal)?;
    if let Some(area) = canvas
        .scrolls
        .iter_mut()
        .find(|area| area.key == modal::SCROLL)
    {
        area.key = scroll_key;
    }
    Ok(())
}

/// The selected label and down chevron share the secondary control's 1.6rem insets.
pub(super) fn select(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    label: &str,
    action: MenuAction,
) -> Result<(), UiPresentationError> {
    let state = canvas.interaction(view, Some(action));
    let role = canvas.role(theme::SECONDARY);
    let (_, face) = button::procedural_face(canvas, bounds, state, true)?;
    let pad = canvas.r(1.6);
    let top = (face[1] + face[3] - canvas.r(BODY.line)) * 0.5;
    canvas.text_line(
        label,
        [face[0] + pad, top],
        (face[2] - face[0] - 2.0 * pad - canvas.r(2.4)).max(1.0),
        BODY,
        role.text,
    )?;
    let centre = [
        face[2] - pad - canvas.r(1.2),
        (face[1] + face[3]) * 0.5 + canvas.r(0.2),
    ];
    let pixel = canvas.r(EDGE);
    let icon = [
        centre[0] - 3.5 * pixel,
        centre[1] - 2.0 * pixel,
        centre[0] + 3.5 * pixel,
        centre[1] + 2.0 * pixel,
    ];
    if !canvas.masked_sprite(CHEVRON_DOWN_IMAGE, icon, role.text)? {
        for row in 0..4 {
            let side = row as f32 * pixel;
            let left = icon[0] + side;
            let right = icon[2] - side;
            let top = icon[1] + row as f32 * pixel;
            canvas.fill(
                [left, top, (left + pixel).min(right), top + pixel],
                role.text,
            )?;
            canvas.fill(
                [(right - pixel).max(left), top, right, top + pixel],
                role.text,
            )?;
        }
    }
    button::stationary_hit(canvas, action, bounds)
}
