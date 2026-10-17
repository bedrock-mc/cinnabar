//! The chat settings modal is created by the vanilla chat screen's popup factory.

use launcher::menu::settings_options::SettingId;
use std::sync::Arc;

use json_ui::{CollectionItem, DataSource, HitRegion, HudModel, Scalar};

use launcher::menu::{MenuAction, settings_options::SettingsOptions};

/// The chat settings popup's state and the persisted options gameplay chat binds.
#[derive(Default)]
pub struct ChatSettings {
    pub(crate) open: bool,
    pub(crate) options: Arc<SettingsOptions>,
    dropdown: Option<u16>,
}

impl ChatSettings {
    /// Keeps gameplay chat bound to current settings without opening the launcher menu.
    pub fn set_snapshot(&mut self, snapshot: (Arc<SettingsOptions>, Option<u16>)) {
        self.options = snapshot.0;
        self.dropdown = snapshot.1;
    }

    /// Opens or closes only the native chat settings popup, retaining the chat draft.
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
    }

    /// A modal consumes editing keys until it is closed.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The persisted options gameplay chat and the HUD bind.
    pub fn options(&self) -> &SettingsOptions {
        &self.options
    }
}

/// Binds the same persisted toggles used by the full settings host.
pub(crate) fn bind(
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
    let smooth = options.chat_smooth_available() && options.value(SettingId::ChatTypeface) != 0;
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
            .replace("%s", &options.value(SettingId::ChatFontSize).to_string())
    } else {
        translate("chat.settings.fontSize.disabled").replace("%s", &translate("typeface.notoSans"))
    };
    data.set_global("#chat_font_size_custom_label", Scalar::Text(size_label));
    data.set_global(
        "#chat_line_spacing_slider_label",
        Scalar::Text(format!(
            "{}: {}",
            translate("chat.settings.lineSpacing"),
            f64::from(options.value(SettingId::ChatLineSpacing)) / 10.0
        )),
    );
    for option in launcher::menu::settings_options::SETTINGS_OPTIONS {
        if !matches!(
            option.name,
            "chat_typeface" | "chat_color" | "mentions_color"
        ) {
            continue;
        }
        if let launcher::menu::settings_options::SettingKind::Dropdown(choices) = option.kind {
            data.set_global(
                format!("#{}_dropdown_label", option.name),
                Scalar::Text(translate(choices[options.value(option.id) as usize].label)),
            );
        }
    }
    let palette = launcher::menu::settings_options::chat::CHAT_PALETTE;
    let choices = launcher::menu::settings_options::SETTINGS_OPTIONS
        .iter()
        .find(|option| option.id == SettingId::ChatColor)
        .expect("chat colors are registered");
    let launcher::menu::settings_options::SettingKind::Dropdown(labels) = choices.kind else {
        unreachable!()
    };
    let colors: Vec<_> = palette
        .iter()
        .map(|color| {
            let [r, g, b] = color.color.rgb().unwrap_or([255; 3]);
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
    for (prefix, name) in [
        ("chat", SettingId::ChatColor),
        ("mentions", SettingId::MentionsColor),
    ] {
        data.set_global(
            format!("#{prefix}_toggle_color"),
            Scalar::Text(colors[options.value(name) as usize].clone()),
        );
    }
}

/// Routes popup edits through the existing settings persistence controller.
pub(crate) fn action(settings: &ChatSettings, region: &HitRegion) -> Option<MenuAction> {
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
        let index = launcher::menu::settings_options::SETTINGS_OPTIONS
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
pub fn apply_hud(settings: &SettingsOptions, model: &mut HudModel) {
    if settings.value(SettingId::HideChat) != 0 {
        model.chat_visible = false;
        model.chat.clear();
    }
    model.chat_background_opacity = background_opacity(settings);
    model.chat_lifetime = settings.chat_lifetime();
    model.chat_color = default_color(settings);
}

/// The saved percentage shared by closed-chat notifications and the open chat screen.
pub(crate) fn background_opacity(settings: &SettingsOptions) -> f64 {
    f64::from(settings.value(SettingId::ChatBackgroundOpacity)) / 100.0
}

/// The optional presentation prefix; white keeps the pack's own default text color.
pub fn default_color(settings: &SettingsOptions) -> Option<char> {
    let code = settings.chat_color_code();
    (ui::BedrockColor::from_code(code) != Some(ui::BedrockColor::White)).then_some(code)
}

/// Selects history descendants without tinting the editor, buttons or other HUD text.
pub(crate) fn message_color(key: &str, root: &str, color: Option<char>) -> Option<char> {
    color.filter(|_| {
        key.split('/')
            .any(|part| part.split(['[', '~']).next() == Some(root))
    })
}
