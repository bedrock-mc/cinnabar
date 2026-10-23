//! The single registry for persisted settings, reset membership and displayed rows.

use super::{layout::SettingPanel, reset::SettingsGroup};
use assets::audio_category::AudioCategory;

#[derive(Clone, Copy, Debug)]
pub struct SettingChoice {
    pub name: &'static str,
    pub label: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub enum SettingKind {
    Toggle,
    Slider,
    Dropdown(&'static [SettingChoice]),
    Host,
}

#[derive(Clone, Copy, Debug)]
pub struct SettingDefinition {
    pub id: SettingId,
    pub section: &'static [(SettingPanel, u16)],
    pub reset_group: &'static [SettingsGroup],
    pub volume: Option<AudioCategory>,
    pub oreui_label: Option<&'static str>,
    pub name: &'static str,
    pub label: &'static str,
    pub kind: SettingKind,
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub default: i32,
}

/// Variable-refresh choices shared by the persisted setting and both Video menus.
pub const VRR_CHOICES: &[SettingChoice] = &[
    SettingChoice {
        name: "vrr_radio_automatic",
        label: render_api::VrrPreference::Automatic.label(),
    },
    SettingChoice {
        name: "vrr_radio_on",
        label: render_api::VrrPreference::On.label(),
    },
    SettingChoice {
        name: "vrr_radio_off",
        label: render_api::VrrPreference::Off.label(),
    },
];

/// Automatic uses reliable platform reports; On declares VRR enabled by the player.
pub const VRR_OPTION: SettingDefinition = SettingId::Vrr.definition();

/// Ordered animation choices shared by the registry and the Video selector.
pub const ANIMATION_CHOICES: &[SettingChoice] = &[
    SettingChoice {
        name: "animations_radio_java",
        label: "Java 1.7",
    },
    SettingChoice {
        name: "animations_radio_bedrock",
        label: "Bedrock",
    },
];

/// Bedrock animation is the default; Java 1.7 stays selectable.
pub const ANIMATIONS_OPTION: SettingDefinition = SettingId::Animations.definition();

/// Bobbing choices retain the saved toggle values and append the hand-only mode.
pub const VIEW_BOBBING_CHOICES: &[SettingChoice] = &[
    SettingChoice {
        name: "view_bobbing_radio_off",
        label: ui::ViewBobbing::Off.label(),
    },
    SettingChoice {
        name: "view_bobbing_radio_full",
        label: ui::ViewBobbing::Full.label(),
    },
    SettingChoice {
        name: "view_bobbing_radio_minimal",
        label: ui::ViewBobbing::Minimal.label(),
    },
];

/// Native placement is enabled; disabling it centers the authored cube.
pub const NATIVE_SKYBOX_OFFSET_OPTION: SettingDefinition =
    SettingId::NativeSkyboxOffset.definition();

pub const MOTION_BLUR_CHOICES: &[SettingChoice] = &[
    SettingChoice {
        name: "motion_blur_radio_off",
        label: ui::MotionBlurQuality::Off.label(),
    },
    SettingChoice {
        name: "motion_blur_radio_low",
        label: ui::MotionBlurQuality::Low.label(),
    },
    SettingChoice {
        name: "motion_blur_radio_medium",
        label: ui::MotionBlurQuality::Medium.label(),
    },
    SettingChoice {
        name: "motion_blur_radio_high",
        label: ui::MotionBlurQuality::High.label(),
    },
];

pub const MOTION_BLUR_OPTION: SettingDefinition = SettingId::MotionBlur.definition();

/// Spatial edge smoothing uses the same choices in JSON-UI and OreUI.
pub const SMAA_CHOICES: &[SettingChoice] = &[
    SettingChoice {
        name: "smaa_radio_off",
        label: ui::SmaaMode::Off.label(),
    },
    SettingChoice {
        name: "smaa_radio_on",
        label: ui::SmaaMode::Smaa.label(),
    },
];

/// An extra silhouette filter independent of vanilla's coverage sample count.
pub const SMAA_OPTION: SettingDefinition = SettingId::Smaa.definition();

/// Controls background checks separately from the user's installation consent.
pub const AUTO_UPDATE_OPTION: SettingDefinition = SettingId::AutoUpdateEnabled.definition();

/// Applying on exit requires a separate opt-in and never overrides Later.
pub const INSTALL_ON_QUIT_OPTION: SettingDefinition = SettingId::InstallUpdatesOnQuit.definition();

/// A channel choice reads its name and label from the signed manifest channel contract.
const fn channel_choice(channel: update_manifest::channel::Channel) -> SettingChoice {
    SettingChoice {
        name: channel.binding(),
        label: channel.label(),
    }
}

pub const UPDATE_CHANNEL_CHOICES: &[SettingChoice] = &[
    channel_choice(update_manifest::channel::Channel::ALL[0]),
    channel_choice(update_manifest::channel::Channel::ALL[1]),
    channel_choice(update_manifest::channel::Channel::ALL[2]),
];
const _: () = assert!(UPDATE_CHANNEL_CHOICES.len() == update_manifest::channel::Channel::ALL.len());

/// Stable is offered only after its signed release exists; beta is the initial channel.
pub const UPDATE_CHANNEL_OPTION: SettingDefinition = SettingId::UpdateChannel.definition();

/// Lets other Cinnabar players see this player as one through the Cinnabar API.
pub const CINNABAR_VISIBILITY_OPTION: SettingDefinition = SettingId::CinnabarVisible.definition();

/// Publishes the client's state to Discord Rich Presence.
pub const DISCORD_PRESENCE_OPTION: SettingDefinition = SettingId::DiscordPresence.definition();

/// Controls whether the activity card names the current server or world.
pub const DISCORD_SERVER_DETAILS_OPTION: SettingDefinition =
    SettingId::DiscordServerDetails.definition();

/// Controls whether Discord receives the current and maximum player count.
pub const DISCORD_PARTY_SIZE_OPTION: SettingDefinition = SettingId::DiscordPartySize.definition();

/// Controls Discord join secrets and incoming join requests.
pub const DISCORD_ALLOW_JOINS_OPTION: SettingDefinition = SettingId::DiscordAllowJoins.definition();

/// Controls the elapsed-time timestamp on the activity card.
pub const DISCORD_TIMESTAMP_OPTION: SettingDefinition = SettingId::DiscordTimestamp.definition();

/// Optional crosshair visibility in both third-person camera views.
pub const THIRD_PERSON_CROSSHAIR_OPTION: SettingDefinition =
    SettingId::ThirdPersonCrosshair.definition();

/// Keeps the crosshair's background inversion enabled unless the player opts out.
pub const INVERT_CROSSHAIR_OPTION: SettingDefinition = SettingId::InvertCrosshair.definition();

/// Percent slider over vanilla's `0.0..=1.0` mouse sensitivity option.
pub const MOUSE_SENSITIVITY_OPTION: SettingDefinition =
    SettingId::KeyboardMouseSensitivity.definition();

/// Lowest selectable frame-rate cap, in frames per second.
pub const MIN_FIXED_FRAME_RATE: i32 = 1;
/// Highest selectable frame-rate cap, in frames per second.
pub const MAX_FIXED_FRAME_RATE: i32 = 240;
/// Unlimited follows the numeric caps on the frame-rate slider.
pub const FRAME_RATE_UNLIMITED: i32 = MAX_FIXED_FRAME_RATE + 1;

/// Supplies the common persisted fields; the registry adds section and reset membership.
const fn option(
    id: SettingId,
    label: &'static str,
    kind: SettingKind,
    min: i32,
    max: i32,
    default: i32,
) -> SettingDefinition {
    SettingDefinition {
        id,
        name: id.name(),
        label,
        kind,
        min,
        max,
        step: 1,
        default,
        section: &[],
        reset_group: &[],
        volume: None,
        oreui_label: None,
    }
}

/// Defines a boolean option with an integral saved value.
const fn toggle(id: SettingId, label: &'static str, default: bool) -> SettingDefinition {
    option(id, label, SettingKind::Toggle, 0, 1, default as i32)
}

/// Defines an integral slider, normalized by the JSON-UI adapter.
const fn slider(
    id: SettingId,
    label: &'static str,
    min: i32,
    max: i32,
    default: i32,
) -> SettingDefinition {
    option(id, label, SettingKind::Slider, min, max, default)
}

/// Defines a dropdown in its displayed radio-choice order.
const fn dropdown(
    id: SettingId,
    label: &'static str,
    choices: &'static [SettingChoice],
    default: i32,
) -> SettingDefinition {
    option(
        id,
        label,
        SettingKind::Dropdown(choices),
        0,
        choices.len() as i32 - 1,
        default,
    )
}

const PERSPECTIVES: &[SettingChoice] = &[
    SettingChoice {
        name: "thirdperson_radio_first",
        label: "options.thirdperson.firstperson",
    },
    SettingChoice {
        name: "thirdperson_radio_third_back",
        label: "options.thirdperson.thirdpersonback",
    },
    SettingChoice {
        name: "thirdperson_radio_third_front",
        label: "options.thirdperson.thirdpersonfront",
    },
];
const GRAPHICS: &[SettingChoice] = &[
    SettingChoice {
        name: "graphics_mode_radio_simple",
        label: "options.graphicsMode.simple",
    },
    SettingChoice {
        name: "graphics_mode_radio_fancy",
        label: "options.graphicsMode.fancy",
    },
];

const CONTENT_LEVELS: &[SettingChoice] = &[
    SettingChoice {
        name: "content_log_gui_level_verbose",
        label: "options.content_log_gui.level.verbose",
    },
    SettingChoice {
        name: "content_log_gui_level_info",
        label: "options.content_log_gui.level.info",
    },
    SettingChoice {
        name: "content_log_gui_level_warn",
        label: "options.content_log_gui.level.warn",
    },
    SettingChoice {
        name: "content_log_gui_level_error",
        label: "options.content_log_gui.level.error",
    },
];
macro_rules! notification_choices {
    ($($suffix:ident => $millis:expr;)+) => {
        const TOAST_DURATIONS: &[SettingChoice] = &[$(
            SettingChoice { name: concat!("notification_duration_radio_", stringify!($suffix)),
                label: concat!("options.notificationDuration.toast.", stringify!($suffix)) },
        )+];
        const CHAT_DURATIONS: &[SettingChoice] = &[$(
            SettingChoice { name: concat!("chat_message_duration_radio_", stringify!($suffix)),
                label: concat!("options.notificationDuration.chat.", stringify!($suffix)) },
        )+];
        pub(super) const NOTIFICATION_MILLIS: &[u64] = &[$($millis,)+];
    }
}

notification_choices! {
    ThreeSec => ui::TOAST_DISPLAY_MILLIS;
    TenSec => 10_000;
    ThirtySec => 30_000;
}

/// Defines a host-owned control that does not store an option value.
const fn host(id: SettingId, label: &'static str) -> SettingDefinition {
    SettingDefinition {
        kind: SettingKind::Host,
        ..toggle(id, label, false)
    }
}

// Defaults remain provisional where documented in plan.md.
macro_rules! settings {
    ($($id:ident => $name:literal, $kind:ident($($arg:expr),*),
        [$($placement:expr),*], [$($reset:expr),*], $volume:expr, $label:expr;)+) => {
        /// A registered setting; string names are confined to serialization and UI contracts.
        #[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash)]
        pub enum SettingId { $($id,)+ }

        impl SettingId {
            /// Returns the stable saved key and JSON-UI controller name.
            pub const fn name(self) -> &'static str {
                match self { $(Self::$id => $name,)+ }
            }

            /// Resolves a name at a persisted-file or JSON-UI boundary.
            pub fn from_name(name: &str) -> Option<Self> {
                match name { $($name => Some(Self::$id),)+ _ => None }
            }

            /// Returns the row index used by menu actions.
            pub const fn index(self) -> usize { self as usize }

            /// Returns the definition generated with this id.
            pub const fn definition(self) -> SettingDefinition { SETTINGS_OPTIONS[self.index()] }
        }

        /// Settings and their section, reset and mixer membership share this table.
        pub const SETTINGS_OPTIONS: &[SettingDefinition] = &[$(
            SettingDefinition {
                section: &[$($placement),*], reset_group: &[$($reset),*],
                volume: $volume, oreui_label: $label,
                ..$kind(SettingId::$id, $($arg),*)
            },
        )+];
    }
}

settings! {
    MacroModifier => "macro_modifier", dropdown("Macro Modifier", super::macro_modifier::MacroModifier::CHOICES, super::macro_modifier::MacroModifier::Alt as i32),
        [(SettingPanel::Keyboard, 6)], [], None, None;
    CinnabarChatPosition => "cinnabar_chat_position", dropdown("Chat Position", super::chat::CHAT_POSITIONS, 0),
        [(SettingPanel::VideoGeneral, 2), (SettingPanel::Chat, 5)], [SettingsGroup::Video, SettingsGroup::Chat], None, None;
    ContentLogGuiLevel => "content_log_gui_level", dropdown("options.content_log_gui.level", CONTENT_LEVELS, 0),
        [(SettingPanel::CreatorContentLog, 3)], [], None, None;
    ToastNotificationDuration => "toast_notification_duration", dropdown("options.notificationDuration.Toast", TOAST_DURATIONS, 0),
        [(SettingPanel::AccessibilityUi, 2)], [SettingsGroup::Accessibility], None, None;
    ChatMessageDuration => "chat_message_duration", dropdown("options.notificationDuration.Chat", CHAT_DURATIONS, 1),
        [(SettingPanel::Chat, 4)], [SettingsGroup::Chat], None, None;
    ControllerSensitivity => "controller_sensitivity", slider("options.sensitivity", 0, 100, 50),
        [(SettingPanel::Controller, 0)], [], None, Some("options.controller.sensitivity.name");
    SpyglassGamepadDampening => "spyglass_gamepad_dampening", slider("options.spyglassdampen", 0, 100, 50),
        [(SettingPanel::Controller, 1)], [], None, Some("options.spyglassdampen.name");
    GamepadCursorSensitivity => "gamepad_cursor_sensitivity", slider("options.gamepadcursorsensitivity", 0, 100, 50),
        [], [], None, None;
    HudTextBackgroundOpacity => "hud_text_background_opacity", slider("options.hudTextBackgroundOpacity", 0, 100, 50),
        [(SettingPanel::AccessibilityUi, 1)], [SettingsGroup::Accessibility], None, None;
    ChatBackgroundOpacity => "chat_background_opacity", slider("options.chatBackgroundOpacity", 0, 100, 50),
        [(SettingPanel::Chat, 1)], [SettingsGroup::Chat], None, None;
    ActionbarTextBackgroundOpacity => "actionbar_text_background_opacity", slider("options.actionBarTextBackgroundOpacity", 0, 100, 50),
        [(SettingPanel::AccessibilityUi, 0)], [SettingsGroup::Accessibility], None, None;
    Darkness => "darkness", slider("options.darknessEffectModifier", 0, 100, 100),
        [(SettingPanel::AccessibilityGameplay, 5)], [SettingsGroup::Accessibility], None, None;
    ScreenDistortion => "screen_distortion", slider("options.screenDistortion", 0, 100, 100),
        [(SettingPanel::AccessibilityGameplay, 6)], [SettingsGroup::Accessibility], None, None;
    GlintStrength => "glint_strength", slider("options.glintStrength", 0, 100, 100),
        [(SettingPanel::AccessibilityGameplay, 7)], [SettingsGroup::Accessibility], None, None;
    GlintSpeed => "glint_speed", slider("options.glintSpeed", 0, 100, 100),
        [(SettingPanel::AccessibilityGameplay, 8)], [SettingsGroup::Accessibility], None, None;
    RenderDistance => "render_distance", slider("options.renderDistance", ui::MIN_RENDER_DISTANCE_CHUNKS as i32, render_api::MAX_VIEW_RADIUS_CHUNKS, ui::MIN_RENDER_DISTANCE_CHUNKS as i32),
        [(SettingPanel::VideoGraphics, 10)], [SettingsGroup::Video], None, None;
    MaxFramerate => "max_framerate", slider("options.framerateLimit", MIN_FIXED_FRAME_RATE, FRAME_RATE_UNLIMITED, FRAME_RATE_UNLIMITED),
        [(SettingPanel::VideoGraphics, 1)], [SettingsGroup::Video], None, None;
    Msaa => "msaa", slider("options.msaa", ui::ANTI_ALIASING_SAMPLE_COUNTS[0] as i32, ui::ANTI_ALIASING_SAMPLE_COUNTS[ui::ANTI_ALIASING_SAMPLE_COUNTS.len() - 1] as i32, ui::DEFAULT_ANTI_ALIASING_SAMPLES as i32),
        [(SettingPanel::VideoGraphics, 4)], [SettingsGroup::Video], None, None;
    Smaa => "smaa", dropdown("Spatial Anti-Aliasing", SMAA_CHOICES, ui::DEFAULT_SMAA_MODE as i32),
        [(SettingPanel::VideoGraphics, 5)], [SettingsGroup::Video], None, None;
    MotionBlur => "motion_blur", dropdown("Motion Blur", MOTION_BLUR_CHOICES, ui::MotionBlurQuality::Off.index()),
        [(SettingPanel::VideoGraphics, 6)], [SettingsGroup::Video], None, None;
    Vsync => "vsync", toggle("options.vsync", true),
        [(SettingPanel::VideoGraphics, 2)], [SettingsGroup::Video], None, None;
    Vrr => "vrr", dropdown("Variable Refresh Rate", VRR_CHOICES, render_api::VrrPreference::Automatic as i32),
        [(SettingPanel::VideoGraphics, 3)], [SettingsGroup::Video], None, None;
    FieldOfView => "field_of_view", slider("options.fov", ui::MIN_FOV_DEGREES, ui::MAX_FOV_DEGREES, ui::DEFAULT_FOV_DEGREES),
        [(SettingPanel::VideoGeneral, 0)], [SettingsGroup::Video], None, Some("options.fov.name");
    Gamma => "gamma", slider("options.gamma", 0, 100, 50),
        [(SettingPanel::VideoGraphics, 0)], [SettingsGroup::Video], None, None;
    InterfaceOpacity => "interface_opacity", slider("options.hudOpacity", 0, 100, 100),
        [(SettingPanel::VideoCustomization, 8)], [SettingsGroup::Video], None, None;
    DamageBob => "damage_bob", slider("options.damageBobbing", 0, 100, 100),
        [(SettingPanel::VideoAccessibility, 6)], [SettingsGroup::Video], None, None;
    PanoramaSpeed => "panorama_speed", slider("options.panoramaSpeed", 0, 100, 100),
        [(SettingPanel::VideoAccessibility, 3)], [SettingsGroup::Video], None, None;
    HideHand => "hide_hand", toggle("options.hidehand", false),
        [(SettingPanel::VideoCustomization, 1)], [SettingsGroup::Video], None, None;
    HidePaperdoll => "hide_paperdoll", toggle("options.hidepaperdoll", false),
        [(SettingPanel::VideoCustomization, 2)], [SettingsGroup::Video], None, None;
    HideHud => "hide_hud", toggle("options.hidehud", false),
        [(SettingPanel::VideoCustomization, 3)], [SettingsGroup::Video], None, None;
    ThirdPersonCrosshair => "third_person_crosshair", toggle("Third Person Crosshair", false),
        [(SettingPanel::VideoCustomization, 4)], [SettingsGroup::Video], None, None;
    InvertCrosshair => "invert_crosshair", toggle("Invert Crosshair Colors", true),
        [(SettingPanel::VideoCustomization, 5)], [SettingsGroup::Video], None, None;
    ScreenAnimations => "screen_animations", toggle("options.screenAnimations", true),
        [(SettingPanel::VideoAccessibility, 2)], [SettingsGroup::Video], None, None;
    ShowAutoSaveIcon => "show_auto_save_icon", toggle("options.showautosaveicon", true),
        [(SettingPanel::VideoCustomization, 9)], [SettingsGroup::Video], None, Some("options.showautosaveicon.name");
    ClassicBoxSelection => "classic_box_selection", toggle("options.classic_box_selection", ui::DEFAULT_OUTLINE_SELECTION),
        [(SettingPanel::VideoCustomization, 6)], [SettingsGroup::Video], None, None;
    IngamePlayerNames => "ingame_player_names", toggle("options.ingamePlayerNames", true),
        [(SettingPanel::VideoCustomization, 7)], [SettingsGroup::Video], None, Some("options.ingamePlayerNames.name");
    ViewBobbing => "view_bobbing", dropdown("options.viewBobbing", VIEW_BOBBING_CHOICES, ui::ViewBobbing::Full as i32),
        [(SettingPanel::VideoAccessibility, 4)], [SettingsGroup::Video], None, None;
    Animations => "animations", dropdown("Animations", ANIMATION_CHOICES, 1),
        [(SettingPanel::VideoAccessibility, 5)], [SettingsGroup::Video], None, None;
    CameraShake => "camera_shake", toggle("options.screenShake", true),
        [(SettingPanel::AccessibilityGameplay, 1), (SettingPanel::VideoAccessibility, 7)], [SettingsGroup::Video, SettingsGroup::Accessibility], None, None;
    TransparentLeaves => "transparent_leaves", toggle("options.transparentleaves", true),
        [(SettingPanel::VideoClouds, 1)], [SettingsGroup::Video], None, None;
    BubbleParticles => "bubble_particles", toggle("options.bubbleparticles", true),
        [(SettingPanel::VideoClouds, 2)], [SettingsGroup::Video], None, None;
    RenderClouds => "render_clouds", toggle("options.renderclouds", true),
        [(SettingPanel::VideoClouds, 0)], [SettingsGroup::Video], None, Some("options.renderClouds");
    FancySkies => "fancy_skies", toggle("options.fancyskies", true),
        [(SettingPanel::VideoGraphics, 8)], [SettingsGroup::Video], None, None;
    SmoothLighting => "smooth_lighting", toggle("options.smooth_lighting", true),
        [(SettingPanel::VideoGraphics, 7)], [SettingsGroup::Video], None, None;
    FieldOfViewToggle => "field_of_view_toggle", toggle("options.fov.toggle", true),
        [(SettingPanel::VideoAccessibility, 8)], [SettingsGroup::Video], None, Some("options.fov.toggle.name");
    ThirdPerson => "third_person", dropdown("options.thirdperson", PERSPECTIVES, 0),
        [(SettingPanel::VideoGeneral, 1)], [SettingsGroup::Video], None, None;
    GraphicsMode => "graphics_mode", dropdown("options.graphicsMode", GRAPHICS, 1),
        [(SettingPanel::VideoPerformance, 0)], [SettingsGroup::Video], None, Some("menu.video.mode");
    KeyboardMouseSensitivity => "keyboard_mouse_sensitivity", slider("options.sensitivity", 0, 100, (semantic_input::DEFAULT_MOUSE_SENSITIVITY * 100.0) as i32),
        [(SettingPanel::Keyboard, 0)], [], None, Some("options.sensitivity.Mouse.name");
    SpyglassMouseDampening => "spyglass_mouse_dampening", slider("options.spyglassdampen", 0, 100, 50),
        [(SettingPanel::Keyboard, 1)], [], None, Some("options.spyglassdampen.name");
    KeyboardMouseInvertYAxis => "keyboard_mouse_invert_y_axis", toggle("options.invertYAxis", false),
        [(SettingPanel::Keyboard, 2)], [], None, Some("options.invertYAxis.name");
    KeyboardMouseAutojump => "keyboard_mouse_autojump", toggle("options.autojump", false),
        [(SettingPanel::Keyboard, 3)], [], None, Some("options.autojump.name");
    KeyboardShowFullKeyboardOptions => "keyboard_show_full_keyboard_options", toggle("options.fullKeyboardGameplay", false),
        [(SettingPanel::Keyboard, 5)], [], None, Some("options.fullKeyboardGameplay.name");
    MainVolume => "main_volume", slider("soundCategory.main", 0, 100, 100),
        [(SettingPanel::Audio, 0)], [SettingsGroup::Audio], Some(AudioCategory::Master), None;
    MusicVolume => "music_volume", slider("soundCategory.music", 0, 100, 100),
        [(SettingPanel::Audio, 1)], [SettingsGroup::Audio], Some(AudioCategory::Music), None;
    SoundVolume => "sound_volume", slider("soundCategory.sound", 0, 100, 100),
        [(SettingPanel::Audio, 2)], [SettingsGroup::Audio], Some(AudioCategory::Sound), None;
    AmbientVolume => "ambient_volume", slider("soundCategory.ambient", 0, 100, 100),
        [(SettingPanel::Audio, 3)], [SettingsGroup::Audio], Some(AudioCategory::Ambient), None;
    BlockVolume => "block_volume", slider("soundCategory.block", 0, 100, 100),
        [(SettingPanel::Audio, 4)], [SettingsGroup::Audio], Some(AudioCategory::Blocks), None;
    HostileVolume => "hostile_volume", slider("soundCategory.hostile", 0, 100, 100),
        [(SettingPanel::Audio, 5)], [SettingsGroup::Audio], Some(AudioCategory::Hostile), None;
    NeutralVolume => "neutral_volume", slider("soundCategory.neutral", 0, 100, 100),
        [(SettingPanel::Audio, 6)], [SettingsGroup::Audio], Some(AudioCategory::Neutral), None;
    PlayerVolume => "player_volume", slider("soundCategory.player", 0, 100, 100),
        [(SettingPanel::Audio, 7)], [SettingsGroup::Audio], Some(AudioCategory::Players), None;
    RecordVolume => "record_volume", slider("soundCategory.record", 0, 100, 100),
        [(SettingPanel::Audio, 8)], [SettingsGroup::Audio], Some(AudioCategory::Records), None;
    WeatherVolume => "weather_volume", slider("soundCategory.weather", 0, 100, 100),
        [(SettingPanel::Audio, 9)], [SettingsGroup::Audio], Some(AudioCategory::Weather), None;
    TexttospeechVolume => "texttospeech_volume", slider("soundCategory.texttospeech", 0, 100, 100),
        [(SettingPanel::AccessibilityTts, 2), (SettingPanel::Audio, 10)], [SettingsGroup::Accessibility, SettingsGroup::Audio], Some(AudioCategory::TextToSpeech), None;
    ControllerInvertYAxis => "controller_invert_y_axis", toggle("options.invertYAxis", false),
        [(SettingPanel::Controller, 2)], [], None, Some("options.invertYAxis.name");
    ControllerAutojump => "controller_autojump", toggle("options.autojump", false),
        [(SettingPanel::Controller, 3)], [], None, Some("options.autojump.name");
    HideTooltips => "hide_tooltips", toggle("options.hidetooltips", false),
        [(SettingPanel::Controller, 4)], [], None, None;
    HideGamepadCursor => "hide_gamepad_cursor", toggle("options.hidegamepadcursor", false),
        [(SettingPanel::Controller, 5)], [], None, None;
    ControllerClearHotbar => "controller_clear_hotbar", toggle("options.clearhotbar", false),
        [(SettingPanel::Controller, 6)], [], None, None;
    SwapGamepadAbButtons => "swap_gamepad_ab_buttons", toggle("options.swapGamepadAB", false),
        [(SettingPanel::Controller, 7)], [], None, None;
    SwapGamepadXyButtons => "swap_gamepad_xy_buttons", toggle("options.swapGamepadXY", false),
        [(SettingPanel::Controller, 8)], [], None, None;
    WebsocketsEnabled => "websockets_enabled", toggle("options.websocketsEnabled", false),
        [(SettingPanel::GeneralNetwork, 0)], [], None, Some("options.websocketsEnabled.name");
    WebsocketEncryption => "websocket_encryption", toggle("options.websocketEncryption", false),
        [(SettingPanel::GeneralNetwork, 1)], [], None, None;
    CinnabarVisible => "cinnabar_visible", toggle("Show me as a Cinnabar player", true),
        [(SettingPanel::GeneralNetwork, 2)], [], None, None;
    CinnabarBadges => "show_cinnabar_badges", toggle("Show Cinnabar badges", true),
        [(SettingPanel::GeneralNetwork, 3)], [], None, None;
    AutoUpdateEnabled => "auto_update_enabled", toggle("Check for updates automatically", super::super::updates::Preferences::DEFAULT.checks),
        [(SettingPanel::Updates, 1)], [], None, None;
    InstallUpdatesOnQuit => "install_updates_on_quit", toggle("Install updates when I quit", super::super::updates::Preferences::DEFAULT.install_on_quit),
        [(SettingPanel::Updates, 2)], [], None, None;
    UpdateChannel => "update_channel", dropdown("Update channel", UPDATE_CHANNEL_CHOICES, 0),
        [(SettingPanel::Updates, 0)], [], None, None;
    ModUpdateChecks => "mod_update_checks", toggle("Check for mod updates", true),
        [(SettingPanel::ModUpdates, 0)], [], None, None;
    ModAutoUpdate => "mod_auto_update", toggle("Install mod updates automatically", true),
        [(SettingPanel::ModUpdates, 1)], [], None, None;
    OnlyTrustedSkinsAllowed => "only_trusted_skins_allowed", toggle("options.onlyTrustedSkinsAllowed", false),
        [(SettingPanel::General, 0)], [], None, Some("options.onlyTrustedSkinsAllowed.name");
    FilterProfanity => "filter_profanity", toggle("options.filterProfanity", false),
        [(SettingPanel::General, 1)], [], None, Some("options.filterProfanity.name");
    PauseOptionToggle => "pause_option_toggle", toggle("options.pauseHint", false),
        [(SettingPanel::GeneralPause, 0)], [], None, None;
    PauseMenuOnFocusLost => "pause_menu_on_focus_lost", toggle("options.pauseMenuOnFocusLost", false),
        [(SettingPanel::GeneralPause, 1)], [], None, None;
    EcomodeToggle => "ecomode_toggle", toggle("options.enableEcoMode", false),
        [(SettingPanel::GeneralMisc, 0)], [], None, Some("options.enableEcoMode.name");
    CopyCoordinateUi => "copy_coordinate_ui", toggle("options.copyCoordinateUI", false),
        [(SettingPanel::CreatorCoordinates, 0)], [], None, Some("options.copyCoordinateUI.name");
    ScriptDebuggerPasscodeRequired => "script_debugger_passcode_required", toggle("options.creator.debuggerPasscodeRequired", false),
        [(SettingPanel::CreatorDebugger, 0)], [], None, None;
    ScriptDebuggerAutoAttach => "script_debugger_auto_attach", toggle("options.creator.debuggerAutoAttach", false),
        [(SettingPanel::CreatorDebugger, 1)], [], None, None;
    EditorCollectNetworkMetrics => "editor_collect_network_metrics", toggle("options.creator.editor.collectNetworkMetrics", false),
        [(SettingPanel::CreatorEditor, 0)], [], None, None;
    ServerboundClientDiagnosticsEnabled => "serverbound_client_diagnostics_enabled", toggle("options.creator.serverboundClientDiagnosticsEnabled", false),
        [(SettingPanel::CreatorDiagnostic, 0)], [], None, None;
    ScriptWatchdogSpikeWarning => "script_watchdog_spike_warning", toggle("options.creator.watchdogSpikeWarning", false),
        [(SettingPanel::CreatorWatchdog, 0)], [], None, None;
    ScriptWatchdogSlowWarning => "script_watchdog_slow_warning", toggle("options.creator.watchdogSlowWarning", false),
        [(SettingPanel::CreatorWatchdog, 1)], [], None, None;
    DeviceInfoUseMemoryTierOverride => "device_info_use_memory_tier_override", toggle("options.creator.deviceInfoUseMemoryTierOverride", false),
        [(SettingPanel::CreatorDevice, 0)], [], None, None;
    DebugTextFilteringUseDelaySecOverride => "debug_text_filtering_use_delay_sec_override", toggle("options.creator.debugTextFilteringUseDelaySecOverride", false),
        [(SettingPanel::CreatorFiltering, 0)], [], None, None;
    ContentLogFile => "content_log_file", toggle("options.content_log_file", false),
        [(SettingPanel::CreatorContentLog, 0)], [], None, None;
    ContentLogGui => "content_log_gui", toggle("options.content_log_gui", false),
        [(SettingPanel::CreatorContentLog, 1)], [], None, None;
    ContentLogGuiShowOnErrors => "content_log_gui_show_on_errors", toggle("options.content_log_gui_show_on_errors", false),
        [(SettingPanel::CreatorContentLog, 2)], [], None, Some("options.content_log_gui_show_on_errors.name");
    EnableGameplaySubtitles => "enable_gameplay_subtitles", toggle("options.enableGameplaySubtitles", false),
        [(SettingPanel::AccessibilityGameplay, 0)], [SettingsGroup::Accessibility], None, None;
    HideOwnGameplaySubtitles => "hide_own_gameplay_subtitles", toggle("options.hideOwnGameplaySubtitles", false),
        [], [SettingsGroup::Accessibility], None, None;
    HideAmbientGameplaySubtitles => "hide_ambient_gameplay_subtitles", toggle("options.hideAmbientGameplaySubtitles", false),
        [], [SettingsGroup::Accessibility], None, None;
    EnableUiTextToSpeech => "enable_ui_text_to_speech", toggle("options.enableUITextToSpeech", false),
        [(SettingPanel::AccessibilityTts, 0)], [SettingsGroup::Accessibility], None, None;
    EnableChatTextToSpeech => "enable_chat_text_to_speech", toggle("options.enableChatTextToSpeech", false),
        [(SettingPanel::AccessibilityTts, 1)], [SettingsGroup::Accessibility], None, None;
    EnableOpenChatMessage => "enable_open_chat_message", toggle("options.enableOpenChatMessage", false),
        [(SettingPanel::AccessibilityTts, 3)], [SettingsGroup::Accessibility], None, None;
    HideEndflash => "hide_endflash", toggle("options.hideEndFlash", false),
        [(SettingPanel::AccessibilityGameplay, 2)], [SettingsGroup::Accessibility], None, None;
    EnableDitheringBlocks => "enable_dithering_blocks", toggle("options.enableDitheringBlocks", false),
        [(SettingPanel::AccessibilityGameplay, 3)], [SettingsGroup::Accessibility], None, None;
    EnableDitheringMobs => "enable_dithering_mobs", toggle("options.enableDitheringMobs", false),
        [(SettingPanel::AccessibilityGameplay, 4)], [SettingsGroup::Accessibility], None, None;
    GuiAccessibilityScaling => "gui_accessibility_scaling", toggle("options.gui.accessibility.scaling", false),
        [(SettingPanel::AccessibilityUi, 4), (SettingPanel::VideoAccessibility, 1)], [SettingsGroup::Video, SettingsGroup::Accessibility], None, Some("options.guiAccessibilityScaling.name");
    HideChat => "hide_chat", toggle("chat.settings.muteAll", false),
        [(SettingPanel::Chat, 0)], [SettingsGroup::Chat], None, None;
    ToggleEmoteChat => "toggle_emote_chat", toggle("chat.settings.muteEmotes", false),
        [(SettingPanel::Chat, 9)], [SettingsGroup::Chat], None, None;
    ToggleTts => "toggle_tts", toggle("chat.settings.tts", false),
        [(SettingPanel::Chat, 10)], [SettingsGroup::Chat], None, None;
    ChatColor => "chat_color", dropdown("chat.settings.chatColor", super::chat::CHAT_COLORS, 0),
        [(SettingPanel::Chat, 6)], [SettingsGroup::Chat], None, None;
    MentionsColor => "mentions_color", dropdown("chat.settings.mentionsColor", super::chat::MENTIONS_COLORS, 5),
        [(SettingPanel::Chat, 7)], [SettingsGroup::Chat], None, None;
    MentionsTarget => "mentions_target", dropdown("chat.settings.mentionsTarget", super::chat::MENTIONS_TARGETS, 0),
        [(SettingPanel::Chat, 8)], [SettingsGroup::Chat], None, None;
    ChatFontSize => "chat_font_size", slider("chat.settings.fontSize", 5, 20, 10),
        [(SettingPanel::Chat, 2)], [SettingsGroup::Chat], None, None;
    ChatLineSpacing => "chat_line_spacing", slider("chat.settings.lineSpacing", 0, 100, 0),
        [(SettingPanel::Chat, 3)], [SettingsGroup::Chat], None, None;
    AlwaysSprint => "always_sprint", toggle("Always Sprint", false),
        [(SettingPanel::Keyboard, 4)], [], None, None;
    ShowExactServerPing => "show_exact_server_ping", toggle("options.showExactServerPing", false),
        [(SettingPanel::VideoCustomization, 10)], [SettingsGroup::Video], None, None;
    OreuiDarkMode => "oreui_dark_mode", toggle("options.oreuiDarkMode", false),
        [(SettingPanel::VideoCustomization, 11)], [SettingsGroup::Video], None, None;
    DiscordPresence => "discord_presence", toggle("Discord Rich Presence", true),
        [(SettingPanel::Discord, 0)], [SettingsGroup::Discord], None, Some("discord.settings.enabled");
    DiscordServerDetails => "discord_server_details", toggle("discord.settings.serverDetails", true),
        [(SettingPanel::DiscordDetails, 0)], [SettingsGroup::Discord], None, None;
    DiscordPartySize => "discord_party_size", toggle("discord.settings.partySize", true),
        [(SettingPanel::DiscordDetails, 1)], [SettingsGroup::Discord], None, None;
    DiscordAllowJoins => "discord_allow_joins", toggle("discord.settings.allowJoins", true),
        [(SettingPanel::DiscordDetails, 2)], [SettingsGroup::Discord], None, None;
    DiscordTimestamp => "discord_timestamp", toggle("discord.settings.timestamp", true),
        [(SettingPanel::DiscordDetails, 3)], [SettingsGroup::Discord], None, None;
    NativeSkyboxOffset => "native_skybox_offset", toggle("Bedrock Skybox Vertical Offset", assets::DEFAULT_NATIVE_SKYBOX_OFFSET),
        [(SettingPanel::VideoGraphics, 9)], [SettingsGroup::Video], None, None;
    FullScreen => "full_screen", host("options.fullscreen"),
        [(SettingPanel::VideoCustomization, 0)], [SettingsGroup::Video], None, None;
    GuiScale => "gui_scale", host("options.guiScale.optionName"),
        [(SettingPanel::AccessibilityUi, 3), (SettingPanel::VideoAccessibility, 0)], [SettingsGroup::Video, SettingsGroup::Accessibility], None, None;
    GraphicsApi => "graphics_api", dropdown("Graphics API", super::graphics_api::CHOICES, 0),
        [(SettingPanel::VideoBackend, 0)], [SettingsGroup::Video], None, None;
    VerboseLogging => "verbose_logging", toggle("Verbose logging", false),
        [(SettingPanel::GeneralAdvanced, 0)], [], None, None;
    RecordNetworkSession => "record_network_session", toggle("Record network session", false),
        [(SettingPanel::GeneralAdvanced, 1)], [], None, None;
    TouchSensitivity => "touch_sensitivity", slider("Touch sensitivity", 0, 100, 50),
        [(SettingPanel::Touch, 0)], [], None, None;

}
