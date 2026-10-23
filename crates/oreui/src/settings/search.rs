//! Settings search uses the displayed registry labels and the existing editable controls.

use super::super::{
    focus,
    paint::{Bounds, Canvas},
    widgets,
};
use super::{Content, sections, services};
use launcher::menu::{
    MenuAction, MenuField, MenuView,
    settings_options::{
        COMMAND_MACRO_COUNT, EXTRA_GAMEPAD, EXTRA_KEYS, GAMEPAD_BINDINGS, GAMEPAD_OFFSET,
        KEY_BINDINGS, SETTINGS_OPTIONS, SettingsOptions, layout::SettingPanel, macro_title,
    },
    settings_sections::{SECTION_DEFINITIONS, SettingSection},
};
use ui_engine::engine::screen::{PaintError, Translate};

/// Keeps the editor outside the independently scrolling category and result lists.
pub(super) fn field(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    bounds: Bounds,
    translate: Translate<'_>,
) -> Result<(), PaintError> {
    let parent = canvas.begin_focus_region(focus::SEARCH, bounds, None, true)?;
    let clear = !view.settings_query.is_empty();
    let clear_label =
        translate("gui.clear").map_or_else(|| "Clear".to_owned(), |text| text.to_string());
    let clear_width = (canvas.measure(&clear_label, super::super::theme::BODY)? + canvas.r(3.2))
        .max(canvas.r(8.0));
    let right = if clear {
        bounds[2] - clear_width - canvas.r(0.8)
    } else {
        bounds[2]
    };
    let placeholder = translate("cinnabar.settings.search")
        .map_or_else(|| "Search settings".to_owned(), |text| text.to_string());
    widgets::search_field(
        canvas,
        view,
        [bounds[0], bounds[1], right, bounds[3]],
        &view.settings_query,
        &placeholder,
        view.field == Some(MenuField::SettingsQuery),
        Some(MenuAction::SearchSettings),
    )?;
    if clear {
        widgets::button(
            canvas,
            view,
            [right + canvas.r(0.8), bounds[1], bounds[2], bounds[3]],
            widgets::Variant::Secondary,
            &clear_label,
            Some(MenuAction::ClearSettingsSearch),
        )?;
    }
    canvas.end_focus_region(parent);
    Ok(())
}

/// Requires every whitespace-separated query term, independent of case and word order.
fn matches(terms: &[String], text: &str) -> bool {
    let text = text.to_lowercase();
    terms.iter().all(|term| text.contains(term))
}

/// Shows matching controls once, grouped by their registry category, with category shortcuts.
pub(super) fn results(content: &mut Content<'_, '_>) -> Result<(), PaintError> {
    let terms: Vec<_> = content
        .view
        .settings_query
        .split_whitespace()
        .map(str::to_lowercase)
        .collect();
    content.heading("Search results", "")?;
    let mut seen = [false; SETTINGS_OPTIONS.len()];
    let mut found = false;
    for section in SECTION_DEFINITIONS
        .iter()
        .filter(|section| section.tab_title.is_some())
    {
        if !content.view.settings_section_shown(section.id) {
            continue;
        }
        let title = content.word(section.tab_title.unwrap());
        let category_match = matches(&terms, &title);
        let mut options = Vec::new();
        for panel in SettingPanel::ALL
            .iter()
            .filter(|panel| panel.section() == section.id)
        {
            for option in panel.options() {
                if seen[option.id.index()] || !content.option_available(option.id) {
                    continue;
                }
                let (label, description) = content.option_text(option.id);
                let text = format!(
                    "{title} {label} {description} {}",
                    option.name.replace('_', " ")
                );
                if matches(&terms, &text) {
                    seen[option.id.index()] = true;
                    options.push(option.id);
                }
            }
        }
        let mut bindings = Vec::new();
        let gamepad = section.id == SettingSection::ControllerAndSwitch;
        if gamepad || section.id == SettingSection::KeyboardAndMouse {
            let labels: Vec<_> = if gamepad {
                GAMEPAD_BINDINGS
                    .iter()
                    .map(|(_, label)| *label)
                    .chain(EXTRA_GAMEPAD.iter().map(|(label, _)| *label))
                    .collect()
            } else {
                KEY_BINDINGS
                    .iter()
                    .map(|(_, label)| *label)
                    .chain(EXTRA_KEYS.iter().map(|(label, _)| *label))
                    .collect()
            };
            for (index, key) in labels.into_iter().enumerate() {
                if matches(&terms, &format!("{title} {}", content.word(key))) {
                    bindings.push((
                        if gamepad {
                            GAMEPAD_OFFSET + index
                        } else {
                            index
                        },
                        key,
                    ));
                }
            }
        }
        let macros: Vec<_> = (0..COMMAND_MACRO_COUNT)
            .filter(|slot| {
                section.id == SettingSection::KeyboardAndMouse
                    && matches(
                        &terms,
                        &format!("{title} {}", content.word(&macro_title(*slot))),
                    )
            })
            .collect();
        if !category_match && options.is_empty() && bindings.is_empty() && macros.is_empty() {
            continue;
        }
        found = true;
        services::action_row(
            content,
            &title,
            "",
            "Open category",
            Some(MenuAction::SettingsSearchSection(section.index)),
        )?;
        for id in options {
            content.option(id)?;
        }
        let defaults = SettingsOptions::default();
        for (index, key) in bindings {
            sections::binding(content, &defaults, index, key)?;
        }
        for slot in macros {
            sections::draw_macro_slot(content, slot)?;
        }
    }
    if !found {
        content.label(
            "No matching settings",
            "Try a different search or clear the search to browse all settings.",
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
