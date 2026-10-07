//! The OreUI design system, drawn in our own code, and the screens vanilla shows
//! with OreUI by default (`docs/oreui.md`). Installed icon and control artwork
//! is read at runtime.

mod accounts;
mod add_server;
mod artwork_runtime;
mod bed_runtime;
mod bedtime;
#[cfg(test)]
mod dark_mode_tests;
mod death;
mod dressing_room;
mod exit;
mod focus;
mod friends;
mod grid;
mod home;
mod icons;
mod inbox;
mod loading;
mod loading_runtime;
mod modal;
mod motion;
mod paint;
mod pause;
mod play;
mod play_realms;
mod play_servers;
mod profile;
mod progress;
mod radio;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod route_tests;
mod screen_runtime;
mod scroll_focus;
mod settings;
#[cfg(test)]
mod settings_tests;
mod sidebar;
use crate::oreui_theme as theme;
mod transitions;
mod widgets;
mod world_settings;

#[cfg(test)]
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
#[cfg(test)]
use crate::menu::{MenuAction, MenuScreen, MenuView, auth::AuthState};

/// Which look OreUI screens draw with.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Look {
    #[default]
    Drawn,
    /// The install's sprites where the drawn look would approximate them.
    Originals,
}

/// The bed screen's last hit rects (window-logical) and the tracked pointer.
#[derive(Default)]
pub(super) struct BedScreen {
    hits: Vec<(BedHit, UiRect)>,
    pointer: Option<UiPoint>,
}
