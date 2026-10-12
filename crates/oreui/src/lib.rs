//! The OreUI design system, drawn in our own code, and the screens vanilla shows
//! with OreUI by default (`docs/oreui.md`). Installed icon and control artwork
//! and the shipped OreUI fonts are read at runtime.

mod accounts;
mod add_server;
mod artwork;
pub mod bed_screen;
mod bedtime;
#[cfg(test)]
mod dark_mode_tests;
mod death;
#[cfg(test)]
mod destructive_tests;
mod dressing_room;
mod exit;
mod focus;
pub mod fonts;
mod friends;
mod grid;
mod home;
mod icons;
pub mod images;
mod inbox;
mod loading;
pub mod loading_screen;
pub mod menu_screen;
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
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
mod transitions;
mod widgets;
mod world_settings;

#[cfg(test)]
#[global_allocator]
static ALLOCATOR: test_alloc::CountingAllocator = test_alloc::CountingAllocator;

use std::sync::Arc;

#[cfg(test)]
use ui::UiNode;
use ui::oreui_theme as theme;
use ui::{UiPoint, UiRect};

pub use bedtime::{BedHit, SleepStatus};
#[cfg(test)]
use paint::Canvas;
pub use paint::Originals;
pub use play_servers::animated_server_details;
pub use transitions::Transitions;

/// The character area a route leaves for the adapter's player preview, as logical
/// `[left, top, right, bottom]` rects.
pub struct CharacterPreview {
    pub control: [f32; 4],
    pub clip: [f32; 4],
}

#[cfg(test)]
use ui_engine::text_metrics::TextMetrics;

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
pub struct OreUiScreen {
    pub originals: Option<Arc<Originals>>,
    pub look: Look,
    pub dark_mode: bool,
    pub transitions: Transitions,
    /// The last menu paint's leftovers for its adapter.
    paint: Option<menu_screen::MenuPaint>,
}

/// The bed screen's last hit rects (window-logical) and the tracked pointer.
#[derive(Default)]
pub struct BedScreen {
    hits: Vec<(BedHit, UiRect)>,
    pointer: Option<UiPoint>,
}
