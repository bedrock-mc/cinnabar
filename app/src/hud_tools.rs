//! Function-key client tools: screenshot capture (F2) and the F3 debug overlay.

mod debug_overlay;
mod screenshot;

use std::path::PathBuf;

use bevy::prelude::{App, Plugin};

pub(crate) struct HudToolsPlugin {
    pub screenshots_dir: PathBuf,
    pub debug_overlay: bool,
}

impl Plugin for HudToolsPlugin {
    fn build(&self, app: &mut App) {
        screenshot::configure(app, self.screenshots_dir.clone());
        if self.debug_overlay {
            debug_overlay::configure(app);
        }
    }
}
