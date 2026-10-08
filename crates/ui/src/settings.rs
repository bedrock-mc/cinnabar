use semantic_input::{ControlSettings, PerspectiveMode};

pub const CURRENT_SETTINGS_SCHEMA: u32 = 2;
pub const DEFAULT_OUTLINE_SELECTION: bool = true;

#[derive(Clone, Debug, PartialEq)]
pub struct UserSettings {
    pub schema_version: u32,
    pub controls: ControlSettings,
    pub video: VideoSettings,
    pub gameplay: GameplaySettings,
}

impl Default for UserSettings {
    fn default() -> Self {
        Self {
            schema_version: CURRENT_SETTINGS_SCHEMA,
            controls: ControlSettings::default(),
            video: VideoSettings::default(),
            gameplay: GameplaySettings::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VideoSettings {
    pub horizontal_fov_degrees: f32,
    pub fullscreen: bool,
    pub frame_cap: Option<u16>,
    pub vsync: bool,
    pub ui_scale: f32,
    pub render_distance_chunks: u8,
    pub brightness: f32,
    pub render_mode: RenderMode,
    pub enhanced_quality: render_api::EnhancedQuality,
    /// Scales speed-driven FOV changes, `0..=1`.
    pub fov_effects_scale: f32,
    /// Scales portal and nausea distortion, `0..=1`.
    pub distortion_scale: f32,
    pub view_bobbing: bool,
    pub cinematic_camera: bool,
    pub camera_shake: bool,
    pub outline_selection: bool,
    pub damage_bob: f32,
    /// Java Edition 1.7 player animations instead of vanilla Bedrock's.
    pub java_animations: bool,
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            horizontal_fov_degrees: 90.0,
            fullscreen: false,
            frame_cap: None,
            vsync: true,
            ui_scale: 1.0,
            render_distance_chunks: 16,
            brightness: 0.5,
            render_mode: RenderMode::Vanilla,
            enhanced_quality: render_api::EnhancedQuality::default(),
            fov_effects_scale: 1.0,
            distortion_scale: 1.0,
            view_bobbing: true,
            cinematic_camera: false,
            camera_shake: true,
            outline_selection: DEFAULT_OUTLINE_SELECTION,
            damage_bob: 1.0,
            java_animations: true,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct GameplaySettings {
    pub default_perspective: PerspectiveMode,
    /// Sprint key toggles a persistent sprint instead of requiring hold.
    pub toggle_sprint: bool,
    /// Automatically requests sprint while keyboard/mouse forward movement is eligible.
    pub always_sprint: bool,
    /// Sneak key toggles a persistent sneak instead of requiring hold.
    pub toggle_sneak: bool,
}

/// World rendering path. `Enhanced` is an opt-in custom look that never counts
/// toward vanilla parity.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
pub enum RenderMode {
    #[default]
    Vanilla,
    Enhanced,
}

impl RenderMode {
    #[must_use]
    /// Stable persisted spelling of this mode.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Vanilla => "vanilla",
            Self::Enhanced => "enhanced",
        }
    }

    /// Case-insensitive inverse of [`Self::as_str`].
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        [Self::Vanilla, Self::Enhanced]
            .into_iter()
            .find(|mode| value.trim().eq_ignore_ascii_case(mode.as_str()))
    }

    #[must_use]
    /// Switch between the two supported modes.
    pub const fn toggled(self) -> Self {
        match self {
            Self::Vanilla => Self::Enhanced,
            Self::Enhanced => Self::Vanilla,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{RenderMode, UserSettings};

    #[test]
    fn default_block_selection_uses_an_outline() {
        assert!(UserSettings::default().video.outline_selection);
    }

    #[test]
    fn render_mode_defaults_to_vanilla_and_round_trips_its_text() {
        assert_eq!(RenderMode::default(), RenderMode::Vanilla);
        for mode in [RenderMode::Vanilla, RenderMode::Enhanced] {
            assert_eq!(RenderMode::parse(mode.as_str()), Some(mode));
            assert_eq!(mode.toggled().toggled(), mode);
        }
        assert_eq!(RenderMode::parse(" ENHANCED "), Some(RenderMode::Enhanced));
        assert_eq!(RenderMode::parse("shaders"), None);
    }
}
