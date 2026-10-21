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

/// Keeps font sizing visible when the carrier ties it to the removed typeface selector.
pub(crate) fn keep_font_size_visible(catalog: &mut json_ui::Catalog) {
    let Some(control) = catalog
        .controls_mut()
        .find(|control| control.owner_ns == "chat_settings" && control.name == "font_size_slider")
    else {
        return;
    };
    if let Some(bindings) = control
        .props
        .get_mut("bindings")
        .and_then(serde_json::Value::as_array_mut)
    {
        bindings.retain(|binding| binding["binding_name"] != "#chat_typeface_visible");
    }
    control
        .props
        .insert("visible".into(), serde_json::Value::Bool(true));
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
    data.set_global("#chat_typeface_visible", Scalar::Bool(false));
    data.set_global("#chat_font_type", Scalar::Text("default".into()));
    data.set_global("#chat_font_size_enabled", Scalar::Bool(true));
    let size_label = translate("chat.settings.fontSize")
        .replace("%s", &options.value(SettingId::ChatFontSize).to_string());
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
        if !matches!(option.id, SettingId::ChatColor | SettingId::MentionsColor) {
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Chat keeps the bundled face and exposes sizing without a second typeface.
    #[test]
    fn chat_popup_hides_typeface_and_keeps_font_sizing_enabled() {
        for locale in ["en_US", "ja_JP"] {
            let mut options = SettingsOptions::default();
            options.set_language(locale);
            options.set(SettingId::ChatFontSize.index(), 15);
            let mut settings = ChatSettings::default();
            settings.set_snapshot((Arc::new(options), None));
            settings.set_open(true);
            let mut data = DataSource::default();
            bind(&settings, &mut data, &|key| {
                if key == "chat.settings.fontSize" {
                    "Size: %s".into()
                } else {
                    key.into()
                }
            });
            let mut expected = data.clone();
            expected.set_global("#chat_typeface_visible", Scalar::Bool(false));
            expected.set_global("#chat_font_type", Scalar::Text("default".into()));
            expected.set_global("#chat_font_size_enabled", Scalar::Bool(true));
            expected.set_global(
                "#chat_font_size_custom_label",
                Scalar::Text("Size: 15".into()),
            );
            assert_eq!(data, expected);
        }
    }

    /// The pack may share a visibility binding between the typeface and font-size rows.
    #[test]
    fn font_size_visibility_is_independent_of_the_removed_typeface_row() {
        let mut catalog = json_ui::Catalog::default();
        let bindings = serde_json::json!([{"binding_name":"#chat_typeface_visible", "binding_name_override":"#visible"}]);
        let content = serde_json::json!({
            "namespace":"chat_settings",
            "typeface_dropdown":{"type":"panel", "bindings":bindings},
            "font_size_slider":{"type":"panel", "bindings":bindings}
        });
        catalog.overlay_text("ui/chat_settings.json", &content.to_string());
        keep_font_size_visible(&mut catalog);
        let mut data = DataSource::default();
        data.set_global("#chat_typeface_visible", Scalar::Bool(false));
        for (reference, visible) in [
            ("chat_settings.typeface_dropdown", false),
            ("chat_settings.font_size_slider", true),
        ] {
            let root = json_ui::resolve(&catalog, reference, &json_ui::Context::empty())
                .control
                .unwrap();
            let (bound, _) = json_ui::bind_stateful(
                &Arc::new(root),
                &data,
                &json_ui::EmptyLibrary,
                &mut json_ui::BindState::new(),
            );
            assert_eq!(bound.properties["visible"].as_bool(), Some(visible));
        }
    }
}
