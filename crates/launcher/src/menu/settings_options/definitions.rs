//! Controller names and labels come from the pinned pack's settings_sections JSON.

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
}

#[derive(Clone, Copy, Debug)]
pub struct SettingDefinition {
    pub name: &'static str,
    pub label: &'static str,
    pub kind: SettingKind,
    pub min: i32,
    pub max: i32,
    pub step: i32,
    pub default: i32,
}

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

/// The default choice uses Java animation while Bedrock retains the vanilla paths.
pub const ANIMATIONS_OPTION: SettingDefinition =
    dropdown("animations", "Animations", ANIMATION_CHOICES, 0);

/// Optional crosshair visibility in both third-person camera views.
pub const THIRD_PERSON_CROSSHAIR_OPTION: SettingDefinition =
    toggle("third_person_crosshair", "Third Person Crosshair", false);

/// Keeps the crosshair's background inversion enabled unless the player opts out.
pub const INVERT_CROSSHAIR_OPTION: SettingDefinition =
    toggle("invert_crosshair", "Invert Crosshair Colors", true);

/// Defines one boolean binding with an integral persisted value.
const fn toggle(name: &'static str, label: &'static str, default: bool) -> SettingDefinition {
    SettingDefinition {
        name,
        label,
        kind: SettingKind::Toggle,
        min: 0,
        max: 1,
        step: 1,
        default: default as i32,
    }
}

/// Defines an integral slider; the JSON-UI adapter converts to a normalized position.
const fn slider(
    name: &'static str,
    label: &'static str,
    min: i32,
    max: i32,
    default: i32,
) -> SettingDefinition {
    SettingDefinition {
        name,
        label,
        kind: SettingKind::Slider,
        min,
        max,
        step: 1,
        default,
    }
}

/// Defines a radio-backed dropdown in the pack's displayed choice order.
const fn dropdown(
    name: &'static str,
    label: &'static str,
    choices: &'static [SettingChoice],
    default: i32,
) -> SettingDefinition {
    SettingDefinition {
        name,
        label,
        kind: SettingKind::Dropdown(choices),
        min: 0,
        max: choices.len() as i32 - 1,
        step: 1,
        default,
    }
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
const TOAST_DURATIONS: &[SettingChoice] = &[
    SettingChoice {
        name: "notification_duration_radio_ThreeSec",
        label: "options.notificationDuration.toast.ThreeSec",
    },
    SettingChoice {
        name: "notification_duration_radio_TenSec",
        label: "options.notificationDuration.toast.TenSec",
    },
    SettingChoice {
        name: "notification_duration_radio_ThirtySec",
        label: "options.notificationDuration.toast.ThirtySec",
    },
];
const CHAT_DURATIONS: &[SettingChoice] = &[
    SettingChoice {
        name: "chat_message_duration_radio_ThreeSec",
        label: "options.notificationDuration.chat.ThreeSec",
    },
    SettingChoice {
        name: "chat_message_duration_radio_TenSec",
        label: "options.notificationDuration.chat.TenSec",
    },
    SettingChoice {
        name: "chat_message_duration_radio_ThirtySec",
        label: "options.notificationDuration.chat.ThirtySec",
    },
];

// Defaults retained from the existing desktop host are provisional until the
// current vanilla option defaults are confirmed; see plan.md.
pub const SETTINGS_OPTIONS: &[SettingDefinition] = &[
    dropdown(
        "content_log_gui_level",
        "options.content_log_gui.level",
        CONTENT_LEVELS,
        0,
    ),
    dropdown(
        "toast_notification_duration",
        "options.notificationDuration.Toast",
        TOAST_DURATIONS,
        0,
    ),
    dropdown(
        "chat_message_duration",
        "options.notificationDuration.Chat",
        CHAT_DURATIONS,
        1,
    ),
    slider("controller_sensitivity", "options.sensitivity", 0, 100, 50),
    slider(
        "spyglass_gamepad_dampening",
        "options.spyglassdampen",
        0,
        100,
        50,
    ),
    slider(
        "gamepad_cursor_sensitivity",
        "options.gamepadcursorsensitivity",
        0,
        100,
        50,
    ),
    slider(
        "hud_text_background_opacity",
        "options.hudTextBackgroundOpacity",
        0,
        100,
        50,
    ),
    slider(
        "chat_background_opacity",
        "options.chatBackgroundOpacity",
        0,
        100,
        50,
    ),
    slider(
        "actionbar_text_background_opacity",
        "options.actionBarTextBackgroundOpacity",
        0,
        100,
        50,
    ),
    slider("darkness", "options.darknessEffectModifier", 0, 100, 100),
    slider("screen_distortion", "options.screenDistortion", 0, 100, 100),
    slider("glint_strength", "options.glintStrength", 0, 100, 100),
    slider("glint_speed", "options.glintSpeed", 0, 100, 100),
    slider(
        "render_distance",
        "options.renderDistance",
        4,
        render_api::PHASE0_MAX_VIEW_RADIUS_CHUNKS,
        render_api::PHASE0_MAX_VIEW_RADIUS_CHUNKS,
    ),
    slider("max_framerate", "options.framerateLimit", 0, 240, 0),
    // Vanilla keeps this out of retail menus (persisted `gfx_vsync`, on); see plan.md.
    toggle("vsync", "options.vsync", true),
    slider("field_of_view", "options.fov", 30, 110, 60),
    slider("gamma", "options.gamma", 0, 100, 50),
    slider("interface_opacity", "options.hudOpacity", 0, 100, 100),
    slider("damage_bob", "options.damageBobbing", 0, 100, 100),
    slider("panorama_speed", "options.panoramaSpeed", 0, 100, 100),
    toggle("hide_hand", "options.hidehand", false),
    toggle("hide_paperdoll", "options.hidepaperdoll", false),
    toggle("hide_hud", "options.hidehud", false),
    THIRD_PERSON_CROSSHAIR_OPTION,
    INVERT_CROSSHAIR_OPTION,
    toggle("screen_animations", "options.screenAnimations", true),
    toggle("show_auto_save_icon", "options.showautosaveicon", true),
    toggle(
        "classic_box_selection",
        "options.classic_box_selection",
        ui::DEFAULT_OUTLINE_SELECTION,
    ),
    toggle("ingame_player_names", "options.ingamePlayerNames", true),
    toggle("view_bobbing", "options.viewBobbing", true),
    ANIMATIONS_OPTION,
    toggle("camera_shake", "options.screenShake", true),
    toggle("transparent_leaves", "options.transparentleaves", true),
    toggle("bubble_particles", "options.bubbleparticles", true),
    toggle("render_clouds", "options.renderclouds", true),
    toggle("fancy_skies", "options.fancyskies", true),
    toggle("smooth_lighting", "options.smooth_lighting", true),
    toggle("field_of_view_toggle", "options.fov.toggle", true),
    dropdown("third_person", "options.thirdperson", PERSPECTIVES, 0),
    dropdown("graphics_mode", "options.graphicsMode", GRAPHICS, 1),
    slider(
        "keyboard_mouse_sensitivity",
        "options.sensitivity",
        0,
        100,
        50,
    ),
    slider(
        "spyglass_mouse_dampening",
        "options.spyglassdampen",
        0,
        100,
        50,
    ),
    toggle("keyboard_mouse_invert_y_axis", "options.invertYAxis", false),
    // Vanilla 1.26.50 ships auto-jump off for every input mode.
    toggle("keyboard_mouse_autojump", "options.autojump", false),
    toggle(
        "keyboard_show_full_keyboard_options",
        "options.fullKeyboardGameplay",
        false,
    ),
    slider("main_volume", "soundCategory.main", 0, 100, 100),
    slider("music_volume", "soundCategory.music", 0, 100, 100),
    slider("sound_volume", "soundCategory.sound", 0, 100, 100),
    slider("ambient_volume", "soundCategory.ambient", 0, 100, 100),
    slider("block_volume", "soundCategory.block", 0, 100, 100),
    slider("hostile_volume", "soundCategory.hostile", 0, 100, 100),
    slider("neutral_volume", "soundCategory.neutral", 0, 100, 100),
    slider("player_volume", "soundCategory.player", 0, 100, 100),
    slider("record_volume", "soundCategory.record", 0, 100, 100),
    slider("weather_volume", "soundCategory.weather", 0, 100, 100),
    slider(
        "texttospeech_volume",
        "soundCategory.texttospeech",
        0,
        100,
        100,
    ),
    // P:ui/settings_sections/controls_section.json:732; default is not yet recovered.
    toggle("controller_invert_y_axis", "options.invertYAxis", false),
    // P:ui/settings_sections/controls_section.json:741; vanilla 1.26.50 default is off.
    toggle("controller_autojump", "options.autojump", false),
    // P:ui/settings_sections/controls_section.json:750; default is not yet recovered.
    toggle("hide_tooltips", "options.hidetooltips", false),
    // P:ui/settings_sections/controls_section.json:759; default is not yet recovered.
    toggle("hide_gamepad_cursor", "options.hidegamepadcursor", false),
    // P:ui/settings_sections/controls_section.json:768; default is not yet recovered.
    toggle("controller_clear_hotbar", "options.clearhotbar", false),
    // P:ui/settings_sections/controls_section.json:777; default is not yet recovered.
    toggle("swap_gamepad_ab_buttons", "options.swapGamepadAB", false),
    // P:ui/settings_sections/controls_section.json:796; default is not yet recovered.
    toggle("swap_gamepad_xy_buttons", "options.swapGamepadXY", false),
    // P:ui/settings_sections/general_section.json:110; default is not yet recovered.
    toggle("websockets_enabled", "options.websocketsEnabled", false),
    // P:ui/settings_sections/general_section.json:119; default is not yet recovered.
    toggle("websocket_encryption", "options.websocketEncryption", false),
    // P:ui/settings_sections/general_section.json:165; default is not yet recovered.
    toggle("auto_update_enabled", "options.autoUpdateEnabled", false),
    // P:ui/settings_sections/general_section.json:246; default is not yet recovered.
    toggle(
        "only_trusted_skins_allowed",
        "options.onlyTrustedSkinsAllowed",
        false,
    ),
    // P:ui/settings_sections/general_section.json:269; default is not yet recovered.
    toggle("filter_profanity", "options.filterProfanity", false),
    // P:ui/settings_sections/general_section.json:318; default is not yet recovered.
    toggle("pause_option_toggle", "options.pauseHint", false),
    // P:ui/settings_sections/general_section.json:332; default is not yet recovered.
    toggle(
        "pause_menu_on_focus_lost",
        "options.pauseMenuOnFocusLost",
        false,
    ),
    // P:ui/settings_sections/general_section.json:388; default is not yet recovered.
    toggle("ecomode_toggle", "options.enableEcoMode", false),
    // P:ui/settings_sections/general_section.json:2046; default is not yet recovered.
    toggle("copy_coordinate_ui", "options.copyCoordinateUI", false),
    // P:ui/settings_sections/general_section.json:2104; default is not yet recovered.
    toggle(
        "script_debugger_passcode_required",
        "options.creator.debuggerPasscodeRequired",
        false,
    ),
    // P:ui/settings_sections/general_section.json:2127; default is not yet recovered.
    toggle(
        "script_debugger_auto_attach",
        "options.creator.debuggerAutoAttach",
        false,
    ),
    // P:ui/settings_sections/general_section.json:2269; default is not yet recovered.
    toggle(
        "editor_collect_network_metrics",
        "options.creator.editor.collectNetworkMetrics",
        false,
    ),
    // P:ui/settings_sections/general_section.json:2328; default is not yet recovered.
    toggle(
        "serverbound_client_diagnostics_enabled",
        "options.creator.serverboundClientDiagnosticsEnabled",
        false,
    ),
    // P:ui/settings_sections/general_section.json:2420; default is not yet recovered.
    toggle(
        "script_watchdog_spike_warning",
        "options.creator.watchdogSpikeWarning",
        false,
    ),
    // P:ui/settings_sections/general_section.json:2449; default is not yet recovered.
    toggle(
        "script_watchdog_slow_warning",
        "options.creator.watchdogSlowWarning",
        false,
    ),
    // P:ui/settings_sections/general_section.json:2529; default is not yet recovered.
    toggle(
        "device_info_use_memory_tier_override",
        "options.creator.deviceInfoUseMemoryTierOverride",
        false,
    ),
    // P:ui/settings_sections/general_section.json:2634; default is not yet recovered.
    toggle(
        "debug_text_filtering_use_delay_sec_override",
        "options.creator.debugTextFilteringUseDelaySecOverride",
        false,
    ),
    // P:ui/settings_sections/general_section.json:2734; default is not yet recovered.
    toggle("content_log_file", "options.content_log_file", false),
    // P:ui/settings_sections/general_section.json:2742; default is not yet recovered.
    toggle("content_log_gui", "options.content_log_gui", false),
    // P:ui/settings_sections/general_section.json:2750; default is not yet recovered.
    toggle(
        "content_log_gui_show_on_errors",
        "options.content_log_gui_show_on_errors",
        false,
    ),
    // P:ui/settings_sections/general_section.json:4753; default is not yet recovered.
    toggle(
        "enable_gameplay_subtitles",
        "options.enableGameplaySubtitles",
        false,
    ),
    // P:ui/settings_sections/general_section.json:4769; default is not yet recovered.
    toggle(
        "hide_own_gameplay_subtitles",
        "options.hideOwnGameplaySubtitles",
        false,
    ),
    // P:ui/settings_sections/general_section.json:4785; default is not yet recovered.
    toggle(
        "hide_ambient_gameplay_subtitles",
        "options.hideAmbientGameplaySubtitles",
        false,
    ),
    // P:ui/settings_sections/general_section.json:4838; default is not yet recovered.
    toggle(
        "enable_ui_text_to_speech",
        "options.enableUITextToSpeech",
        false,
    ),
    // P:ui/settings_sections/general_section.json:4847; default is not yet recovered.
    toggle(
        "enable_chat_text_to_speech",
        "options.enableChatTextToSpeech",
        false,
    ),
    // P:ui/settings_sections/general_section.json:4876; default is not yet recovered.
    toggle(
        "enable_open_chat_message",
        "options.enableOpenChatMessage",
        false,
    ),
    // P:ui/settings_sections/general_section.json:4941; default is not yet recovered.
    toggle("hide_endflash", "options.hideEndFlash", false),
    // P:ui/settings_sections/general_section.json:4949; default is not yet recovered.
    toggle(
        "enable_dithering_blocks",
        "options.enableDitheringBlocks",
        false,
    ),
    // P:ui/settings_sections/general_section.json:4957; default is not yet recovered.
    toggle(
        "enable_dithering_mobs",
        "options.enableDitheringMobs",
        false,
    ),
    // P:ui/settings_sections/general_section.json:5133; default is not yet recovered.
    toggle(
        "gui_accessibility_scaling",
        "options.gui.accessibility.scaling",
        false,
    ),
    // P:ui/chat_settings_menu_screen.json:16; default is not yet recovered.
    toggle("hide_chat", "chat.settings.muteAll", false),
    // P:ui/chat_settings_menu_screen.json:26; default is not yet recovered.
    toggle("toggle_emote_chat", "chat.settings.muteEmotes", false),
    // P:ui/chat_settings_menu_screen.json:35; default is not yet recovered.
    toggle("toggle_tts", "chat.settings.tts", false),
    // P:ui/chat_settings_menu_screen.json:68; defaults and numeric mappings remain provisional.
    dropdown(
        "chat_typeface",
        "chat.settings.typeface",
        super::chat::TYPEFACES,
        0,
    ),
    dropdown(
        "chat_color",
        "chat.settings.chatColor",
        super::chat::CHAT_COLORS,
        0,
    ),
    dropdown(
        "mentions_color",
        "chat.settings.mentionsColor",
        super::chat::MENTIONS_COLORS,
        5,
    ),
    slider("chat_font_size", "chat.settings.fontSize", 5, 20, 10),
    slider("chat_line_spacing", "chat.settings.lineSpacing", 0, 100, 0),
    toggle("always_sprint", "Always Sprint", false),
    toggle(
        super::SHOW_EXACT_SERVER_PING,
        "options.showExactServerPing",
        false,
    ),
    toggle(super::OREUI_DARK_MODE, "options.oreuiDarkMode", false),
];
