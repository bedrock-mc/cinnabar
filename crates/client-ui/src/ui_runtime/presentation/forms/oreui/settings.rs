//! Settings use the OreUI registry's groups and native controls over the shared launcher state.

mod account_icon;
mod button;
mod controls;
mod gui_scale;
mod layout;
mod picker;
mod resources;
mod sections;
mod services;
mod sidebar;

#[cfg(test)]
mod header_tests;
#[cfg(test)]
mod row_tests;

use super::super::super::UiPresentationError;
use super::super::menu_screens::{SETTINGS_SECTIONS, Translate};
use super::focus;
use super::paint::{Bounds, Canvas};
use super::theme::{self, BODY, CAPTION, EDGE, NEUTRAL, TEXT, TEXT_DIMMER};
use super::widgets;
use crate::menu::{
    MenuAction, MenuView,
    settings_options::{SETTINGS_OPTIONS, SettingKind},
    view::SettingsFocusAxis,
};
use crate::ui_runtime::oreui_assets::CHEVRON_LEFT_IMAGE;
use crate::ui_runtime::presentation::IconRef;

pub(super) fn section_index(name: &str) -> u8 {
    SETTINGS_SECTIONS
        .iter()
        .find_map(|(key, index)| (*key == name).then_some(*index))
        .expect("registered settings section")
}

pub(super) fn draw(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    size: [f32; 2],
    translate: Translate<'_>,
    gamerpic: Option<IconRef>,
) -> Result<(), UiPresentationError> {
    widgets::screen_overlay(canvas, size)?;
    canvas.settings_scrollbars = true;
    canvas.capture_focus = true;
    let screen_focus =
        canvas.begin_focus_region(focus::SCREEN, [0.0, 0.0, size[0], size[1]], None, true)?;
    canvas.focus_delegate(None, Some(focus::CONTENT));
    let header = header(canvas, view, size[0], translate)?;
    let columns = layout::Columns::new(canvas.rem, size[0]);
    let (nav, body) = (columns.navigation, columns.content);
    let top = header + canvas.r(0.8);
    let bottom = size[1];
    let content_focus =
        canvas.begin_focus_region(focus::CONTENT, [0.0, header, size[0], bottom], None, true)?;
    sidebar::draw(
        canvas,
        view,
        [nav[0], top, nav[1], bottom],
        translate,
        gamerpic,
    )?;
    let detail_focus =
        canvas.begin_focus_region(focus::DETAIL, [body[0], top, body[1], bottom], None, false)?;
    let panel = [body[0], top, body[1] - canvas.r(1.6), bottom];
    canvas.fill(panel, NEUTRAL.fill)?;
    canvas.frame(panel, EDGE, theme::BORDER)?;
    let viewport = [
        panel[0] + canvas.r(EDGE),
        top + canvas.r(EDGE),
        body[1],
        bottom,
    ];
    let section = SETTINGS_SECTIONS
        .iter()
        .find_map(|(key, index)| (*index == view.settings_section).then_some(*key))
        .unwrap_or("accessibility_forced_index");
    let key = format!("oreui_settings/{section}");
    let tab = focus::tab(section_index(section));
    let entrance = canvas.begin_entrance(super::motion::Surface::Settings(section_index(section)));
    let tab_focus = canvas.begin_focus_region(tab, [body[0], top, body[1], bottom], None, false)?;
    // A resize can shrink the scroll range. Redraw once with the new clamped offset.
    for attempt in 0..2 {
        let rollback = (
            canvas.nodes.len(),
            *canvas.next,
            canvas.hits.len(),
            canvas.scrolls.len(),
            canvas.focus_hits.len(),
            canvas.focus_targets.len(),
            canvas.focus_landmarks.len(),
            canvas.slider_tracks.len(),
        );
        let body_focus = canvas.begin_focus_region(
            tab + 1,
            viewport,
            Some(SettingsFocusAxis::Vertical),
            true,
        )?;
        let scroll = canvas.begin_scroll(&key, viewport)?;
        let offset = scroll.offset;
        let content_right = panel[2] - canvas.r(EDGE);
        let mut content = Content {
            canvas,
            view,
            span: [viewport[0], content_right],
            column_width: columns.content_width,
            y: viewport[1] - offset,
            translate,
            nested: false,
        };
        sections::draw(&mut content, section)?;
        let height = content.y + offset - viewport[1];
        content.canvas.end_scroll(scroll, height)?;
        content.canvas.end_focus_region(body_focus);
        let max = (height - (viewport[3] - viewport[1])).max(0.0);
        if offset <= max || attempt == 1 {
            break;
        }
        canvas.nodes.truncate(rollback.0);
        *canvas.next = rollback.1;
        canvas.hits.truncate(rollback.2);
        canvas.scrolls.truncate(rollback.3);
        canvas.focus_hits.truncate(rollback.4);
        canvas.focus_targets.truncate(rollback.5);
        canvas.focus_landmarks.truncate(rollback.6);
        canvas.slider_tracks.truncate(rollback.7);
        canvas.offsets.insert(key.clone(), max);
    }
    canvas.end_focus_region(tab_focus);
    canvas.end_focus_region(detail_focus);
    canvas.end_focus_region(content_focus);
    canvas.end_focus_region(screen_focus);
    canvas.end_entrance(entrance, size)?;
    if resources::draw_picker(canvas, view, size)? {
        return Ok(());
    }
    picker::draw(canvas, view, size, translate)
}

fn header(
    canvas: &mut Canvas<'_>,
    view: &MenuView,
    width: f32,
    translate: Translate<'_>,
) -> Result<f32, UiPresentationError> {
    let height = canvas.r(4.4);
    let header_focus = canvas.begin_focus_region(
        focus::HEADER,
        [0.0, 0.0, width, height + canvas.r(0.4)],
        None,
        true,
    )?;
    let role = canvas.role(theme::NEUTRAL20);
    canvas.fill([0.0, 0.0, width, height], role.fill)?;
    canvas.fill(
        [0.0, height, width, height + canvas.r(0.4)],
        theme::HEADER_STRIP,
    )?;
    let title = translate("hbui.Settings.title")
        .or_else(|| translate("menu.settings.caps"))
        .or_else(|| translate("menu.settings"))
        .map_or_else(|| "Settings".to_owned(), |text| text.to_string());
    canvas.text_centred(
        &title.to_uppercase(),
        [canvas.r(4.4), 0.0, width - canvas.r(4.4), height],
        theme::HEADER5,
        role.text,
        false,
    )?;
    let back = [0.0, 0.0, canvas.r(4.4), height];
    let state = canvas.interaction(view, Some(MenuAction::AddBack));
    let motion = canvas.feedback(state, true, false, super::motion::Kind::Surface);
    if motion.hover > 0.0 || motion.press > 0.0 {
        canvas.fill(back, motion.color(role.fill, role.hovered, role.pressed))?;
    }
    if state.focused {
        canvas.frame(back, EDGE, role.text)?;
    }
    let pixel = canvas.r(EDGE);
    let [x, y] = [canvas.r(2.0), height * 0.5];
    let glyph = [x, y - canvas.r(0.7), x + canvas.r(0.8), y + canvas.r(0.7)];
    if !canvas.masked_sprite(CHEVRON_LEFT_IMAGE, glyph, role.text)? {
        for step in 0..4 {
            let dx = step as f32 * pixel;
            canvas.fill([x + dx, y - dx - pixel, x + dx + pixel, y - dx], role.text)?;
            canvas.fill([x + dx, y + dx, x + dx + pixel, y + dx + pixel], role.text)?;
        }
    }
    canvas.hit(MenuAction::AddBack, back)?;
    canvas.end_focus_region(header_focus);
    Ok(height + canvas.r(0.4))
}

pub(super) struct Content<'a, 'b> {
    pub(super) canvas: &'a mut Canvas<'b>,
    pub(super) view: &'a MenuView,
    pub(super) span: [f32; 2],
    column_width: f32,
    pub(super) y: f32,
    pub(super) translate: Translate<'a>,
    pub(super) nested: bool,
}

impl Content<'_, '_> {
    pub(super) fn word(&self, key: &str) -> String {
        if key.is_empty() {
            return String::new();
        }
        (self.translate)(key).map_or_else(
            || sections::fallback(key).to_owned(),
            |text| text.to_string(),
        )
    }

    pub(super) fn inset(&self) -> [f32; 2] {
        [
            self.span[0] + self.canvas.r(2.4),
            self.span[1] - self.canvas.r(2.4),
        ]
    }

    pub(super) fn heading(
        &mut self,
        title: &str,
        description: &str,
    ) -> Result<(), UiPresentationError> {
        let title = self.word(title).to_uppercase();
        let description = self.word(description);
        let [left, right] = self.inset();
        let mut top = self.y + self.canvas.r(2.4);
        let height = self.canvas.text(
            &title,
            [left, top],
            (right - left).max(1.0),
            theme::SECTION_HEADER,
            TEXT,
            false,
        )?;
        top += height.max(self.canvas.r(theme::SECTION_HEADER.line));
        if !description.is_empty() {
            top += self
                .canvas
                .text(
                    &description,
                    [left, top],
                    (right - left).max(1.0),
                    CAPTION,
                    TEXT_DIMMER,
                    false,
                )?
                .max(self.canvas.r(CAPTION.line));
        }
        self.y = top + self.canvas.r(0.8);
        Ok(())
    }

    pub(super) fn label(
        &mut self,
        title: &str,
        description: &str,
    ) -> Result<(), UiPresentationError> {
        let title = self.word(title);
        let description = self.word(description);
        let [left, right] = self.inset();
        let b = self.row(&title, &description, right - left, None, 0.0)?;
        self.texts(b, &title, &description, right - left, 0.0)?;
        Ok(())
    }

    fn row(
        &mut self,
        title: &str,
        description: &str,
        width: f32,
        action: Option<MenuAction>,
        extra: f32,
    ) -> Result<Bounds, UiPresentationError> {
        let title_height = self
            .canvas
            .measure_height(title, width.max(1.0), BODY)?
            .max(self.canvas.r(BODY.line));
        let description_height = if description.is_empty() {
            0.0
        } else {
            self.canvas
                .measure_height(description, width.max(1.0), CAPTION)?
                .max(self.canvas.r(CAPTION.line))
        };
        let stack = title_height + description_height;
        let height = self.canvas.r(2.4)
            + if extra == 0.0 {
                stack.max(self.canvas.r(4.0))
            } else {
                stack
            }
            + extra;
        let b = [self.span[0], self.y, self.span[1], self.y + height];
        let is_switch = action.is_some_and(|action| match action {
            MenuAction::SettingsFullscreen(_) => true,
            MenuAction::SettingsOption(index, _) => SETTINGS_OPTIONS
                .get(usize::from(index))
                .is_some_and(|option| matches!(option.kind, SettingKind::Toggle)),
            _ => false,
        });
        let interaction = if is_switch {
            self.canvas.switch_interaction(self.view, action)
        } else {
            self.canvas.interaction(self.view, action)
        };
        let role = if self.nested {
            theme::NEUTRAL80
        } else {
            NEUTRAL
        };
        self.canvas.fill(b, role.fill)?;
        let [upper, lower] = if self.nested {
            [NEUTRAL.border; 2]
        } else {
            [[255, 255, 255, 26], [0, 0, 0, 77]]
        };
        let edge = self.canvas.r(EDGE);
        self.canvas.fill([b[0], b[1], b[2], b[1] + edge], upper)?;
        self.canvas.fill([b[0], b[3] - edge, b[2], b[3]], lower)?;
        if interaction.focused && !is_switch {
            self.canvas.frame(b, EDGE, theme::OUTLINE)?;
        }
        self.y = b[3];
        Ok(b)
    }

    fn texts(
        &mut self,
        b: Bounds,
        title: &str,
        description: &str,
        width: f32,
        extra: f32,
    ) -> Result<(), UiPresentationError> {
        let left = self.inset()[0];
        let title_height = self
            .canvas
            .measure_height(title, width.max(1.0), BODY)?
            .max(self.canvas.r(BODY.line));
        let description_height = if description.is_empty() {
            0.0
        } else {
            self.canvas
                .measure_height(description, width.max(1.0), CAPTION)?
                .max(self.canvas.r(CAPTION.line))
        };
        let top = (b[1] + b[3] - extra - title_height - description_height) * 0.5;
        let height = self
            .canvas
            .text(title, [left, top], width.max(1.0), BODY, TEXT, false)?
            .max(self.canvas.r(BODY.line));
        if !description.is_empty() {
            self.canvas.text(
                description,
                [left, top + height],
                width.max(1.0),
                CAPTION,
                TEXT_DIMMER,
                false,
            )?;
        }
        Ok(())
    }

    pub(super) fn option(&mut self, name: &str) -> Result<(), UiPresentationError> {
        if name == "full_screen" {
            let action = MenuAction::SettingsFullscreen(!self.view.fullscreen);
            let title = self.word("options.fullscreen");
            let description = self.word("options.fullscreen.description");
            return self.boolean(&title, &description, self.view.fullscreen, action, true);
        }
        if name == "gui_scale" {
            return gui_scale::draw(self);
        }
        let Some((index, option)) = SETTINGS_OPTIONS
            .iter()
            .enumerate()
            .find(|(_, option)| option.name == name)
        else {
            return Ok(());
        };
        let value = self.view.settings_options.get(index);
        let label_key = sections::option_label(name, option.label);
        let title = self.word(label_key);
        let base_key = label_key.strip_suffix(".name").unwrap_or(label_key);
        let description_key = match name {
            "keyboard_mouse_invert_y_axis" => "options.invertYAxis.Mouse.description".to_owned(),
            "controller_invert_y_axis" => "options.invertYAxis.description".to_owned(),
            _ => format!("{base_key}.description"),
        };
        let description =
            (self.translate)(&description_key).map_or_else(String::new, |text| text.to_string());
        let description = if name == crate::menu::settings_options::VRR_OPTION.name {
            "Automatic uses display detection. Choose On if variable refresh is enabled in your display and system settings, or Off to disable the VRR frame cap.".to_owned()
        } else if name == "screen_animations" {
            "Smooth highlights, button presses and screen transitions. Turn off for an instant interface.".to_owned()
        } else if (name == "render_clouds"
            || name == "max_framerate"
            || name == crate::menu::settings_options::SHOW_EXACT_SERVER_PING
            || name == crate::menu::settings_options::OREUI_DARK_MODE)
            && description.is_empty()
        {
            sections::fallback(&description_key).to_owned()
        } else {
            description
        };
        match option.kind {
            SettingKind::Toggle => {
                let (on, enabled) = match name {
                    "vsync" => (
                        self.view.vsync_override.unwrap_or(value != 0),
                        self.view.vsync_override.is_none(),
                    ),
                    _ => (value != 0, true),
                };
                self.boolean(
                    &title,
                    &description,
                    on,
                    MenuAction::SettingsOption(index as u16, 1 - value),
                    enabled,
                )?;
            }
            SettingKind::Slider => {
                let [left, right] = self.inset();
                let shown = match name {
                    "field_of_view" => format!("{value}°"),
                    "render_distance" => self
                        .word("options.renderDistanceFormat")
                        .replace("%s", &value.to_string()),
                    "max_framerate" => {
                        super::super::vsync_setting::frame_rate_label(value, &|key: &str| {
                            self.word(key)
                        })
                    }
                    "msaa" => value.to_string(),
                    _ => format!("{value}%"),
                };
                let amount = self.canvas.measure(&shown, BODY)?;
                let width = (right - left - amount - self.canvas.r(1.6)).max(1.0);
                let b = self.row(&title, &description, width, None, self.canvas.r(4.0))?;
                self.texts(b, &title, &description, width, self.canvas.r(4.0))?;
                self.canvas.text(
                    &shown,
                    [right - amount, b[1] + self.canvas.r(1.2)],
                    amount + 1.0,
                    BODY,
                    TEXT,
                    false,
                )?;
                let actions: Vec<_> = if name == "msaa" {
                    self.view
                        .settings_options
                        .anti_aliasing_support()
                        .counts()
                        .map(|samples| MenuAction::SettingsOption(index as u16, samples as i32))
                        .collect()
                } else {
                    (option.min..=option.max)
                        .step_by(option.step as usize)
                        .map(|value| MenuAction::SettingsOption(index as u16, value))
                        .collect()
                };
                let selected = actions
                    .iter()
                    .position(|action| *action == MenuAction::SettingsOption(index as u16, value))
                    .unwrap_or(0);
                controls::slider(
                    self.canvas,
                    self.view,
                    [
                        left,
                        b[3] - self.canvas.r(4.4),
                        right,
                        b[3] - self.canvas.r(1.2),
                    ],
                    &actions,
                    selected,
                    name == "render_distance",
                )?;
            }
            SettingKind::Dropdown(choices) => {
                let [left, right] = self.inset();
                let labels: Vec<_> = choices
                    .iter()
                    .map(|choice| self.word(choice.label))
                    .collect();
                let picker = choices.len() > 5
                    || self.column_width < self.canvas.r(15.0 * choices.len() as f32)
                    || labels.iter().any(|label| {
                        label.encode_utf16().count()
                            > if self.column_width < self.canvas.r(70.0) {
                                26
                            } else {
                                40
                            }
                    });
                let cell_width = if labels.is_empty() {
                    right - left
                } else {
                    (right - left) / labels.len() as f32
                };
                let control_height = if picker {
                    self.canvas.r(self::picker::SELECT_HEIGHT)
                } else {
                    labels.iter().try_fold(0.0_f32, |height, label| {
                        widgets::choice_height(self.canvas, label, cell_width)
                            .map(|choice| height.max(choice))
                    })?
                };
                let extra = control_height + self.canvas.r(if picker { 0.8 } else { 0.4 });
                let b = self.row(&title, &description, right - left, None, extra)?;
                self.texts(b, &title, &description, right - left, extra)?;
                let current = self.word(choices[value as usize].label);
                let control = [
                    left,
                    b[3] - control_height - self.canvas.r(1.2),
                    right,
                    b[3] - self.canvas.r(1.2),
                ];
                if picker {
                    picker::select(
                        self.canvas,
                        self.view,
                        control,
                        &current,
                        MenuAction::SettingsDropdown(index as u16),
                    )?;
                } else {
                    for (choice, label) in labels.iter().enumerate() {
                        widgets::choice(
                            self.canvas,
                            self.view,
                            [
                                left + choice as f32 * cell_width - self.canvas.r(0.2),
                                control[1],
                                left + (choice + 1) as f32 * cell_width,
                                control[3],
                            ],
                            label,
                            choice == value as usize,
                            MenuAction::SettingsOption(index as u16, choice as i32),
                        )?;
                    }
                    for choice in 0..labels.len() {
                        widgets::choice_focus(
                            self.canvas,
                            self.view,
                            [
                                left + choice as f32 * cell_width - self.canvas.r(0.2),
                                control[1],
                                left + (choice + 1) as f32 * cell_width,
                                control[3],
                            ],
                            choice == value as usize,
                            MenuAction::SettingsOption(index as u16, choice as i32),
                        )?;
                    }
                }
            }
        }
        Ok(())
    }

    fn boolean(
        &mut self,
        title: &str,
        description: &str,
        on: bool,
        action: MenuAction,
        enabled: bool,
    ) -> Result<(), UiPresentationError> {
        let [left, right] = self.inset();
        let width = (right - left - self.canvas.r(7.6)).max(1.0);
        let b = self.row(title, description, width, enabled.then_some(action), 0.0)?;
        self.texts(b, title, description, width, 0.0)?;
        controls::toggle_control(
            self.canvas,
            self.view,
            [
                right - self.canvas.r(6.0),
                (b[1] + b[3]) * 0.5 - self.canvas.r(1.6),
            ],
            on,
            action,
            enabled,
        )?;
        if enabled {
            self.canvas.hit(action, b)?;
        }
        Ok(())
    }
}
