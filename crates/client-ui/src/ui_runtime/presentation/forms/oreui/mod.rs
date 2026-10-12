//! The OreUI design system, drawn in our own code, and the screens vanilla shows
//! with OreUI by default (`docs/oreui.md`). Installed icon and control artwork
//! is read at runtime.

mod accounts;
mod add_server;
mod artwork;
pub(super) mod bed_screen;
mod bedtime;
#[cfg(test)]
mod dark_mode_tests;
mod death;
#[cfg(test)]
mod destructive_tests;
mod dressing_room;
mod exit;
mod focus;
mod friends;
mod grid;
mod home;
mod icons;
mod inbox;
mod loading;
pub(super) mod loading_screen;
mod modal;
mod motion;
mod paint;
mod pause;
mod play;
mod play_realms;
mod play_servers;
pub(in crate::ui_runtime::presentation) use play_servers::animated_server_details;
pub(super) mod menu_screen;
mod profile;
mod progress;
mod radio;
mod realm_membership;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod route_tests;
mod scroll_focus;
mod settings;
#[cfg(test)]
mod settings_tests;
mod sidebar;
use ui::oreui_theme as theme;
mod transitions;
mod widgets;
mod world_settings;

use std::sync::Arc;

#[cfg(test)]
use ui::UiNode;
use ui::{UiPoint, UiRect};

pub use bedtime::BedHit;
#[cfg(test)]
use paint::Canvas;
pub use paint::Originals;
pub(super) use transitions::Transitions;

pub(super) struct CharacterPreview {
    pub(super) control: paint::Bounds,
    pub(super) clip: paint::Bounds,
}

#[cfg(test)]
use super::super::{TextMetrics, UiPresentationRuntime};

/// Which look OreUI screens draw with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Look {
    #[default]
    Drawn,
    /// The install's sprites where the drawn look would approximate them.
    Originals,
}

/// OreUI's installed artwork, look and motion, carried across builds.
#[derive(Default)]
pub(super) struct OreUiScreen {
    pub(super) originals: Option<Arc<Originals>>,
    pub(super) look: Look,
    pub(super) dark_mode: bool,
    pub(super) transitions: Transitions,
    /// The last menu paint's leftovers for its adapter.
    paint: Option<menu_screen::MenuPaint>,
}

/// The bed screen's last hit rects (window-logical) and the tracked pointer.
#[derive(Default)]
pub(super) struct BedScreen {
    hits: Vec<(BedHit, UiRect)>,
    pointer: Option<UiPoint>,
}
