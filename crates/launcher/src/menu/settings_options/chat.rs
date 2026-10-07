//! Chat option definitions and normalized display settings.
use super::{SettingsOptions, definitions::SettingChoice};

pub const TYPEFACES: &[SettingChoice] = &[
    SettingChoice {
        name: "typeface_radio_mojangles",
        label: "typeface.mojangles",
    },
    SettingChoice {
        name: "typeface_radio_notoSans",
        label: "typeface.notoSans",
    },
];
pub const CHAT_COLORS: &[SettingChoice] = &[
    SettingChoice {
        name: "chat_0",
        label: "color.white",
    },
    SettingChoice {
        name: "chat_1",
        label: "color.green",
    },
    SettingChoice {
        name: "chat_2",
        label: "color.aqua",
    },
    SettingChoice {
        name: "chat_3",
        label: "color.red",
    },
    SettingChoice {
        name: "chat_4",
        label: "color.light_purple",
    },
    SettingChoice {
        name: "chat_5",
        label: "color.yellow",
    },
    SettingChoice {
        name: "chat_6",
        label: "color.gold",
    },
];
pub const MENTIONS_COLORS: &[SettingChoice] = &[
    SettingChoice {
        name: "mentions_0",
        label: "color.white",
    },
    SettingChoice {
        name: "mentions_1",
        label: "color.green",
    },
    SettingChoice {
        name: "mentions_2",
        label: "color.aqua",
    },
    SettingChoice {
        name: "mentions_3",
        label: "color.red",
    },
    SettingChoice {
        name: "mentions_4",
        label: "color.light_purple",
    },
    SettingChoice {
        name: "mentions_5",
        label: "color.yellow",
    },
    SettingChoice {
        name: "mentions_6",
        label: "color.gold",
    },
];

impl SettingsOptions {
    /// Vanilla disables smooth chat for these four locales.
    pub fn chat_smooth_available(&self) -> bool {
        !matches!(self.language(), Some("zh_TW" | "zh_CN" | "ko_KR" | "ja_JP"))
    }

    /// Scales the open chat's text; the exact retail font-size option range remains unverified.
    pub fn chat_font_scale(&self) -> f64 {
        if !self.chat_smooth_available() || self.value("chat_typeface") == 0 {
            1.0
        } else {
            f64::from(self.value("chat_font_size")) / 10.0
        }
    }

    /// Applies vanilla's one-decimal padding plus its nonzero epsilon.
    pub fn chat_line_padding(&self) -> f64 {
        f64::from(self.value("chat_line_spacing")) / 10.0 + 0.001
    }

    /// Uses the seven legacy chat colors from the vanilla indexed palette.
    pub fn chat_color_code(&self) -> char {
        ['f', 'a', 'b', 'c', 'd', 'e', '6'][self.value("chat_color") as usize]
    }

    /// Matches the three authored notification-duration radio choices.
    pub fn chat_lifetime(&self) -> f64 {
        notification_millis(self.value("chat_message_duration")) as f64 / 1_000.0
    }

    /// Uses the pack's three toast-duration choices for new notification requests.
    pub fn toast_lifetime_millis(&self) -> u64 {
        notification_millis(self.value("toast_notification_duration"))
    }
}

/// Both notification menus author the same three duration choices.
fn notification_millis(index: i32) -> u64 {
    match index {
        1 => 10_000,
        2 => 30_000,
        _ => ui::TOAST_DISPLAY_MILLIS,
    }
}
