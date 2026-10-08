//! The chat settings modal is created by the vanilla chat screen's popup factory.

use std::sync::Arc;

use json_ui::{CollectionItem, DataSource, HitRegion, HudModel, Scalar};

use super::super::UiPresentationRuntime;
use crate::menu::{MenuAction, settings_options::SettingsOptions};

#[derive(Default)]
pub(super) struct ChatSettings {
    pub(super) open: bool,
    pub(super) options: Arc<SettingsOptions>,
    dropdown: Option<u16>,
}

impl UiPresentationRuntime {
    /// Keeps gameplay chat bound to current settings without opening the launcher menu.
    pub fn set_chat_settings_snapshot(&mut self, snapshot: (Arc<SettingsOptions>, Option<u16>)) {
        let settings = &mut self.form_presentation.chat.settings;
        settings.options = snapshot.0;
        settings.dropdown = snapshot.1;
    }

    /// Opens or closes only the native chat settings popup, retaining the chat draft.
    pub fn set_chat_settings_open(&mut self, open: bool) {
        self.form_presentation.chat.settings.open = open;
    }

    /// A modal consumes editing keys until it is closed.
    pub fn chat_settings_open(&self) -> bool {
        self.form_presentation.chat.settings.open
    }
}

/// Binds the same persisted toggles used by the full settings host.
pub(super) fn bind(
    settings: &ChatSettings,
    data: &mut DataSource,
    translate: &dyn Fn(&str) -> String,
) {
    if !settings.open {
        return;
    }
    data.set_factory_id("chat_setting_popup");
    data.set_global("#close_button_visible", Scalar::Bool(true));
    super::settings_controls::bind_values(&settings.options, settings.dropdown, data, translate);
    let options = &settings.options;
    let smooth = options.chat_smooth_available() && options.value("chat_typeface") != 0;
    data.set_global(
        "#chat_typeface_visible",
        Scalar::Bool(options.chat_smooth_available()),
    );
    data.set_global(
        "#chat_font_type",
        Scalar::Text(if smooth { "smooth" } else { "default" }.into()),
    );
    data.set_global("#chat_font_size_enabled", Scalar::Bool(smooth));
    let size_label = if smooth {
        translate("chat.settings.fontSize")
            .replace("%s", &options.value("chat_font_size").to_string())
    } else {
        translate("chat.settings.fontSize.disabled").replace("%s", &translate("typeface.notoSans"))
    };
    data.set_global("#chat_font_size_custom_label", Scalar::Text(size_label));
    data.set_global(
        "#chat_line_spacing_slider_label",
        Scalar::Text(format!(
            "{}: {}",
            translate("chat.settings.lineSpacing"),
            f64::from(options.value("chat_line_spacing")) / 10.0
        )),
    );
    for option in crate::menu::settings_options::SETTINGS_OPTIONS {
        if !matches!(
            option.name,
            "chat_typeface" | "chat_color" | "mentions_color"
        ) {
            continue;
        }
        if let crate::menu::settings_options::SettingKind::Dropdown(choices) = option.kind {
            data.set_global(
                format!("#{}_dropdown_label", option.name),
                Scalar::Text(translate(
                    choices[options.value(option.name) as usize].label,
                )),
            );
        }
    }
    let palette = [
        ui::BedrockColor::White,
        ui::BedrockColor::Green,
        ui::BedrockColor::Aqua,
        ui::BedrockColor::Red,
        ui::BedrockColor::LightPurple,
        ui::BedrockColor::Yellow,
        ui::BedrockColor::Gold,
    ];
    let choices = crate::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .find(|option| option.name == "chat_color")
        .expect("chat colors are registered");
    let crate::menu::settings_options::SettingKind::Dropdown(labels) = choices.kind else {
        unreachable!()
    };
    let colors: Vec<_> = palette
        .iter()
        .map(|color| {
            let [r, g, b] = color.rgb().unwrap_or([255; 3]);
            format!("#{r:02x}{g:02x}{b:02x}")
        })
        .collect();
    data.set_collection(
        "font_colors",
        labels
            .iter()
            .enumerate()
            .map(|(index, choice)| {
                CollectionItem::default()
                    .with("#font_color_label", Scalar::Text(translate(choice.label)))
                    .with("#font_color", Scalar::Text(colors[index].clone()))
            })
            .collect(),
    );
    for (prefix, name) in [("chat", "chat_color"), ("mentions", "mentions_color")] {
        data.set_global(
            format!("#{prefix}_toggle_color"),
            Scalar::Text(colors[options.value(name) as usize].clone()),
        );
    }
}

/// Routes popup edits through the existing settings persistence controller.
pub(super) fn action(settings: &ChatSettings, region: &HitRegion) -> Option<MenuAction> {
    if region.pressed.as_deref() == Some("button.reset_chat_settings") {
        return Some(MenuAction::SettingsResetChat);
    }
    if region.collection.as_deref() == Some("font_colors")
        && region.kind == json_ui::HitKind::Toggle
    {
        let name = if region.key.contains("mentions_color") {
            "mentions_color"
        } else {
            "chat_color"
        };
        let index = crate::menu::settings_options::SETTINGS_OPTIONS
            .iter()
            .position(|option| option.name == name)?;
        return Some(MenuAction::SettingsOption(
            index as u16,
            i32::try_from(region.collection_index?).ok()?,
        ));
    }
    super::settings_controls::action_values(&settings.options, region)
}

/// Applies implemented chat visibility, background and notification-duration options.
pub(super) fn apply_hud(settings: &SettingsOptions, model: &mut HudModel) {
    if settings.value("hide_chat") != 0 {
        model.chat_visible = false;
        model.chat.clear();
    }
    model.chat_background_opacity = f64::from(settings.value("chat_background_opacity")) / 100.0;
    model.chat_lifetime = settings.chat_lifetime();
}

/// Applies the selected default color without discarding server-authored formatting codes.
pub(super) fn message_text(settings: &SettingsOptions, text: &str) -> String {
    let text = super::super::bounded_visible_text(text);
    if settings.chat_color_code() == 'f' {
        text.to_owned()
    } else {
        format!("§{}{text}", settings.chat_color_code())
    }
}
