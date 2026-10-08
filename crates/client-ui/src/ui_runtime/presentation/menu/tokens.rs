//! Cinnabar launcher design tokens.
//!
//! The palette stays deliberately matte and rectangular: Minecraft-like
//! surfaces, one cyan interaction accent, and semantic colors used by every
//! launcher screen. Geometry tokens keep the shell responsive without each
//! screen inventing its own spacing rhythm.

pub(super) const CANVAS: [u8; 4] = [12, 16, 23, 252];
pub(super) const TOP_BAR: [u8; 4] = [17, 22, 31, 252];
pub(super) const SIDEBAR: [u8; 4] = [19, 25, 35, 252];
pub(super) const PANEL: [u8; 4] = [26, 33, 45, 250];
pub(super) const PANEL_ALT: [u8; 4] = [33, 42, 56, 252];
pub(super) const PANEL_RAISED: [u8; 4] = [39, 49, 65, 255];
pub(super) const BORDER: [u8; 4] = [55, 67, 85, 255];
pub(super) const BUTTON: [u8; 4] = [43, 54, 70, 255];
pub(super) const BUTTON_FOCUSED: [u8; 4] = [55, 112, 151, 255];
pub(super) const BUTTON_HOVERED: [u8; 4] = [50, 67, 88, 255];
pub(super) const BUTTON_PRESSED: [u8; 4] = [66, 145, 184, 255];
pub(super) const TEXT: [u8; 4] = [239, 243, 247, 255];
pub(super) const MUTED: [u8; 4] = [166, 178, 193, 255];
pub(super) const SUBTLE: [u8; 4] = [116, 130, 149, 255];
pub(super) const ACCENT: [u8; 4] = [91, 211, 232, 255];
pub(super) const SUCCESS: [u8; 4] = [100, 210, 137, 255];
pub(super) const DANGER: [u8; 4] = [230, 101, 105, 255];
pub(super) const SCRIM: [u8; 4] = [4, 6, 10, 214];

pub(super) const SPACE_XS: f32 = 6.0;
pub(super) const SPACE_SM: f32 = 10.0;
pub(super) const SPACE_MD: f32 = 16.0;
pub(super) const SPACE_LG: f32 = 24.0;
pub(super) const SPACE_XL: f32 = 32.0;
pub(super) const CONTROL_HEIGHT: f32 = 44.0;
pub(super) const TOUCH_CONTROL_HEIGHT: f32 = 48.0;
pub(super) const TOP_BAR_HEIGHT: f32 = 82.0;
pub(super) const SIDEBAR_WIDTH: f32 = 212.0;
pub(super) const COMPACT_BREAKPOINT: f32 = 900.0;
