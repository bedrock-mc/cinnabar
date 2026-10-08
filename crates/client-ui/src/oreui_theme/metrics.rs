//! Shared geometry in rem for loading panels and raised menu controls.

/// GUI pixels in one OreUI rem.
pub const GUI_PIXELS_PER_REM: f32 = 5.0;
/// Maximum loading panel width.
pub const LOADING_WIDTH: f32 = 48.0;
/// Loading panel inset.
pub const LOADING_PAD: f32 = 3.2;
/// Progress track height, including its border.
pub const PROGRESS_HEIGHT: f32 = 0.8;
/// Raised control height.
pub const BUTTON_HEIGHT: f32 = 4.0;
/// Loading screen's cancel control width.
pub const CANCEL_WIDTH: f32 = 16.0;
/// Raised controls sink by this distance on press.
pub const BUTTON_DEPTH: f32 = 0.4;

/// Track spacing includes the same inset above and below its bordered face.
pub const LOADING_PROGRESS_AREA: f32 = super::SPACE[3] * 2.0 + PROGRESS_HEIGHT;
/// Loading controls sit below the body with the standard two-rem gap.
pub const LOADING_FOOTER_AREA: f32 = super::SPACE[4] + BUTTON_HEIGHT;
