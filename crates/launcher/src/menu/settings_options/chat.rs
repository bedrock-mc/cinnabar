//! Chat option definitions and normalized display settings.
use super::{
    SettingsOptions,
    definitions::{SettingChoice, SettingDefinition},
};

use super::SettingId;
/// Client presentation preference; the built-in bottom placement remains the default.
pub const CHAT_POSITION_OPTION: SettingDefinition =
    super::SettingId::CinnabarChatPosition.definition();

pub const CHAT_POSITIONS: &[SettingChoice] = &[
    SettingChoice {
        name: "cinnabar_chat_bottom",
        label: "Bottom",
    },
    SettingChoice {
        name: "cinnabar_chat_top",
        label: "Top",
    },
];

/// One color's text code and rendered swatch.
#[derive(Clone, Copy, Debug)]
pub struct ChatColor {
    pub code: char,
    pub color: ui::BedrockColor,
}

macro_rules! chat_colors {
    ($($index:literal => $label:literal, $code:literal, $color:ident;)+) => {
        pub const CHAT_COLORS: &[SettingChoice] = &[$(
            SettingChoice { name: concat!("chat_", stringify!($index)), label: $label },
        )+];
        pub const MENTIONS_COLORS: &[SettingChoice] = &[$(
            SettingChoice { name: concat!("mentions_", stringify!($index)), label: $label },
        )+];
        pub const CHAT_PALETTE: &[ChatColor] = &[$(
            ChatColor { code: $code, color: ui::BedrockColor::$color },
        )+];
    }
}

chat_colors! {
    0 => "color.white", 'f', White;
    1 => "color.green", 'a', Green;
    2 => "color.aqua", 'b', Aqua;
    3 => "color.red", 'c', Red;
    4 => "color.light_purple", 'd', LightPurple;
    5 => "color.yellow", 'e', Yellow;
    6 => "color.gold", '6', Gold;
    7 => "color.gray", '7', Gray;
    8 => "color.dark_gray", '8', DarkGray;
    9 => "color.blue", '9', Blue;
    10 => "color.dark_green", '2', DarkGreen;
    11 => "color.dark_aqua", '3', DarkAqua;
    12 => "color.dark_purple", '5', DarkPurple;
    13 => "color.dark_red", '4', DarkRed;
    14 => "color.dark_blue", '1', DarkBlue;
    15 => "color.black", '0', Black;
}

impl SettingsOptions {
    /// Selects top placement only for the built-in chat presentation.
    pub fn chat_at_top(&self) -> bool {
        self.value(CHAT_POSITION_OPTION.id) == 1
    }

    /// Scales the bundled chat font independently of any retired typeface preference.
    pub fn chat_font_scale(&self) -> f64 {
        f64::from(self.value(SettingId::ChatFontSize)) / 10.0
    }

    /// Applies vanilla's one-decimal padding plus its nonzero epsilon.
    pub fn chat_line_padding(&self) -> f64 {
        f64::from(self.value(SettingId::ChatLineSpacing)) / 10.0 + 0.001
    }

    /// Uses the saved index in the chat presentation palette.
    pub fn chat_color_code(&self) -> char {
        let val = self.value(SettingId::ChatColor) as usize;
        CHAT_PALETTE.get(val).map_or('f', |c| c.code)
    }

    /// Uses the saved index in the mention presentation palette.
    pub fn mentions_color_code(&self) -> char {
        let val = self.value(SettingId::MentionsColor) as usize;
        CHAT_PALETTE.get(val).map_or('e', |c| c.code)
    }

    /// Matches the three authored notification-duration radio choices.
    pub fn chat_lifetime(&self) -> f64 {
        notification_millis(self.value(SettingId::ChatMessageDuration)) as f64 / 1_000.0
    }

    /// Uses the pack's three toast-duration choices for new notification requests.
    pub fn toast_lifetime_millis(&self) -> u64 {
        notification_millis(self.value(SettingId::ToastNotificationDuration))
    }
}

/// Both notification menus author the same three duration choices.
fn notification_millis(index: i32) -> u64 {
    super::definitions::NOTIFICATION_MILLIS
        .get(index as usize)
        .copied()
        .unwrap_or(ui::TOAST_DISPLAY_MILLIS)
}
