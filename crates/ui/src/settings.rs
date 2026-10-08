use semantic_input::{ControlSettings, PerspectiveMode};

pub const CURRENT_SETTINGS_SCHEMA: u32 = 2;
pub const DEFAULT_OUTLINE_SELECTION: bool = true;
/// Vanilla's field-of-view option: default and slider range in degrees.
pub const DEFAULT_FOV_DEGREES: i32 = 60;
pub const MIN_FOV_DEGREES: i32 = 30;
pub const MAX_FOV_DEGREES: i32 = 110;

/// Desktop vanilla starts with two coverage samples per pixel.
pub const DEFAULT_ANTI_ALIASING_SAMPLES: u32 = 2;
/// Sample counts represented by the rendering backend's camera settings.
pub const ANTI_ALIASING_SAMPLE_COUNTS: [u32; 4] = [1, 2, 4, 8];

/// Spatial silhouette smoothing is an opt-in addition to vanilla's MSAA setting.
pub const DEFAULT_SMAA_MODE: SmaaMode = SmaaMode::Off;

/// Spatial SMAA can run alone or alongside multisample coverage.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(i32)]
pub enum SmaaMode {
    Off = 0,
    Smaa = 1,
}

impl Default for SmaaMode {
    /// Uses the shared preference also used by saved Video settings.
    fn default() -> Self {
        DEFAULT_SMAA_MODE
    }
}

impl SmaaMode {
    /// Labels shared by every Video settings host.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Smaa => "SMAA",
        }
    }

    /// Decodes the validated integral setting without enabling unknown modes.
    pub const fn from_value(value: i32) -> Self {
        if value == Self::Smaa as i32 {
            Self::Smaa
        } else {
            Self::Off
        }
    }
}

/// The intersection of color and depth sample counts supported by the active device.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct AntiAliasingSupport(u32);

impl Default for AntiAliasingSupport {
    fn default() -> Self {
        Self::from_counts(ANTI_ALIASING_SAMPLE_COUNTS)
    }
}

impl AntiAliasingSupport {
    /// Always retains single sampling and drops counts the camera cannot represent.
    pub fn from_counts(counts: impl IntoIterator<Item = u32>) -> Self {
        Self(counts.into_iter().fold(1, |mask, count| {
            mask | if ANTI_ALIASING_SAMPLE_COUNTS.contains(&count) {
                count
            } else {
                0
            }
        }))
    }

    /// Lists supported slider stops in increasing quality order.
    pub fn counts(self) -> impl DoubleEndedIterator<Item = u32> {
        ANTI_ALIASING_SAMPLE_COUNTS
            .into_iter()
            .filter(move |count| self.0 & count != 0)
    }

    /// Keeps a saved preference within the device's supported sample counts.
    pub fn select(self, requested: u32) -> u32 {
        self.counts()
            .rev()
            .find(|count| *count <= requested)
            .unwrap_or(1)
    }
}

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
    pub anti_aliasing_samples: u32,
    pub smaa_mode: SmaaMode,
    pub ui_scale: f32,
    pub render_distance_chunks: u8,
    pub brightness: f32,
    pub render_mode: RenderMode,
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
            horizontal_fov_degrees: DEFAULT_FOV_DEGREES as f32,
            fullscreen: false,
            frame_cap: None,
            vsync: true,
            anti_aliasing_samples: DEFAULT_ANTI_ALIASING_SAMPLES,
            smaa_mode: DEFAULT_SMAA_MODE,
            ui_scale: 1.0,
            render_distance_chunks: 16,
            brightness: 0.5,
            render_mode: RenderMode::Vanilla,
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

#[cfg(test)]
mod antialiasing_tests {
    use super::*;

    #[test]
    fn spatial_antialiasing_defaults_off_and_round_trips_setting_values() {
        assert_eq!(UserSettings::default().video.smaa_mode, SmaaMode::Off);
        assert_eq!(DEFAULT_SMAA_MODE, SmaaMode::Off);
        for mode in [SmaaMode::Off, SmaaMode::Smaa] {
            assert_eq!(SmaaMode::from_value(mode as i32), mode);
        }
        assert_eq!(SmaaMode::from_value(-1), SmaaMode::Off);
        assert_eq!(SmaaMode::from_value(2), SmaaMode::Off);
    }

    #[test]
    fn antialiasing_support_keeps_only_usable_stops_and_falls_back_downward() {
        let support = AntiAliasingSupport::from_counts([0, 3, 4, 8, 16]);
        assert_eq!(support.counts().collect::<Vec<_>>(), [1, 4, 8]);
        assert_eq!(support.select(DEFAULT_ANTI_ALIASING_SAMPLES), 1);
        assert_eq!(support.select(7), 4);
        assert_eq!(support.select(32), 8);
        assert_eq!(AntiAliasingSupport::from_counts([]).select(0), 1);
    }
}
