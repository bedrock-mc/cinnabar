//! Bind vanilla option controls to the menu's persisted settings snapshot.

use json_ui::{DataSource, HitKind, HitRegion, Scalar};

use crate::menu::settings_options::{
    SETTINGS_OPTIONS, SettingDefinition, SettingKind, SettingsOptions,
};
use crate::menu::{MenuAction, MenuView};

/// Supply control values, labels and enabled states using the pack's binding names.
pub(super) fn bind(view: &MenuView, data: &mut DataSource, translate: &dyn Fn(&str) -> String) {
    bind_values(
        &view.settings_options,
        view.settings_dropdown,
        data,
        translate,
    );
    super::vsync_setting::bind(view, data);
    bind_visibility(view, data, translate);
}

/// Shares persisted control values with settings popups hosted outside the menu.
pub(super) fn bind_values(
    options: &SettingsOptions,
    dropdown: Option<u16>,
    data: &mut DataSource,
    translate: &dyn Fn(&str) -> String,
) {
    for (index, option) in SETTINGS_OPTIONS.iter().enumerate() {
        let value = options.get(index);
        data.set_global(format!("#{}_enabled", option.name), Scalar::Bool(true));
        match option.kind {
            SettingKind::Toggle => {
                data.set_global(format!("#{}", option.name), Scalar::Bool(value != 0));
            }
            SettingKind::Slider => {
                let (position, steps) = if option.name == "msaa" {
                    let counts: Vec<_> = options.anti_aliasing_support().counts().collect();
                    let selected = counts
                        .iter()
                        .position(|count| *count == value as u32)
                        .unwrap_or(0);
                    (selected as f64, counts.len() as f64)
                } else {
                    (
                        f64::from(value - option.min) / f64::from(option.max - option.min),
                        1.0,
                    )
                };
                data.set_global(format!("#{}", option.name), Scalar::Num(position));
                data.set_global(format!("#{}_steps", option.name), Scalar::Num(steps));
                if option.name == "msaa" {
                    data.set_global("#msaa_enabled", Scalar::Bool(steps > 1.0));
                }
                let shown = display_value(option, value, translate);
                data.set_global(
                    format!("#{}_text_value", option.name),
                    Scalar::Text(shown.clone()),
                );
                data.set_global(
                    format!("#{}_slider_label", option.name),
                    Scalar::Text(format!("{}: {shown}", translate(option.label))),
                );
            }
            SettingKind::Dropdown(choices) => {
                let choice = &choices[value as usize];
                data.set_global(
                    format!("#{}_dropdown_enabled", option.name),
                    Scalar::Bool(true),
                );
                data.set_global(
                    format!("#{}_dropdown", option.name),
                    Scalar::Bool(dropdown == Some(index as u16)),
                );
                data.set_global(
                    format!("#{}_dropdown_toggle_label", option.name),
                    Scalar::Text(translate(choice.label)),
                );
                for (choice_index, choice) in choices.iter().enumerate() {
                    data.set_global(
                        format!("#{}", choice.name),
                        Scalar::Bool(choice_index == value as usize),
                    );
                }
            }
        }
    }
}

/// Supplies capability flags and section-only labels for the full settings host.
fn bind_visibility(view: &MenuView, data: &mut DataSource, translate: &dyn Fn(&str) -> String) {
    for flag in [
        "#hint_toggles_enabled",
        "#screen_animations_visible",
        "#keyboard_show_standard_keyboard_options",
        "#gui_scale_visible",
        "#show_render_distance",
        "#show_msaa",
        "#max_framerate_slider_visible",
        "#advanced_graphics_options_button_visible",
    ] {
        data.set_global(flag, Scalar::Bool(true));
    }
    // Without Vibrant Visuals or ray tracing support vanilla locks those rows.
    for flag in [
        "#graphics_mode_radio_deferred_enabled",
        "#graphics_mode_radio_ray_traced_enabled",
    ] {
        data.set_global(flag, Scalar::Bool(false));
    }
    data.set_global(
        "#advanced_graphics_options_grid_visible",
        Scalar::Bool(view.settings_advanced_graphics),
    );
    // controls_section.json binds these captions separately from the toggle values.
    for (binding, label) in [
        ("#swap_gamepad_ab", "options.swapGamepadAB"),
        ("#swap_gamepad_xy", "options.swapGamepadXY"),
        ("#swap_gamepad_ab_tts", "options.swapGamepadAB.tts"),
        ("#swap_gamepad_xy_tts", "options.swapGamepadXY.tts"),
    ] {
        data.set_global(binding, Scalar::Text(translate(label)));
    }
    let graphics_label = if view.settings_options.value("graphics_mode") == 0 {
        "options.graphicsModeOptions.simple"
    } else {
        "options.graphicsModeOptions.fancy"
    };
    data.set_global(
        "#graphics_mode_toggle_label",
        Scalar::Text(translate(graphics_label)),
    );
}

/// Format controller units; volume labels contain only the percentage in vanilla.
fn display_value(
    option: &SettingDefinition,
    value: i32,
    translate: &dyn Fn(&str) -> String,
) -> String {
    match option.name {
        "field_of_view" | "gui_scale" | "msaa" => value.to_string(),
        "max_framerate" if value == 0 => translate("options.framerateLimit.max"),
        "max_framerate" => value.to_string(),
        "render_distance" => {
            translate("options.renderDistanceFormat").replace("%s", &value.to_string())
        }
        _ => format!("{value}%"),
    }
}

/// Translate the vanilla toggle or dropdown identity into a persisted option edit.
pub(super) fn action(view: &MenuView, region: &HitRegion) -> Option<MenuAction> {
    if region.pressed.as_deref() == Some("button.expand_advanced_graphics") {
        return Some(MenuAction::SettingsAdvancedGraphics);
    }
    action_values(&view.settings_options, region)
}

/// Maps a control edit without requiring a visible launcher menu.
pub(super) fn action_values(options: &SettingsOptions, region: &HitRegion) -> Option<MenuAction> {
    let name = region.control_name.as_deref()?.trim_start_matches('#');
    for (index, option) in SETTINGS_OPTIONS.iter().enumerate() {
        let index = index as u16;
        match option.kind {
            SettingKind::Toggle if region.kind == HitKind::Toggle && name == option.name => {
                return Some(MenuAction::SettingsOption(
                    index,
                    1 - options.get(usize::from(index)),
                ));
            }
            SettingKind::Dropdown(choices) => {
                if name.strip_suffix("_dropdown") == Some(option.name) {
                    return Some(MenuAction::SettingsDropdown(index));
                }
                if let Some(choice) = choices.iter().position(|choice| choice.name == name) {
                    return Some(MenuAction::SettingsOption(index, choice as i32));
                }
            }
            _ => {}
        }
    }
    None
}

/// Enumerate slider stops from the same range used for validation and persistence.
pub(super) fn slider_actions(
    options: &SettingsOptions,
    region: &HitRegion,
) -> Option<Vec<MenuAction>> {
    if region.kind != HitKind::Slider {
        return None;
    }
    let name = region.control_name.as_deref()?;
    let (index, option) = SETTINGS_OPTIONS
        .iter()
        .enumerate()
        .find(|(_, option)| option.name == name && matches!(option.kind, SettingKind::Slider))?;
    if option.name == "msaa" {
        return Some(
            options
                .anti_aliasing_support()
                .counts()
                .map(|samples| MenuAction::SettingsOption(index as u16, samples as i32))
                .collect(),
        );
    }
    Some(
        (option.min..=option.max)
            .step_by(option.step as usize)
            .map(|value| MenuAction::SettingsOption(index as u16, value))
            .collect(),
    )
}
