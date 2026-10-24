//! Engine effects used by client views; gameplay exposure and overlays have their own owners.

use bevy::prelude::{App, Plugin};

/// Installs MSAA composition and optional bloom without unused engine camera effects.
pub struct ClientPostProcessPlugin;

impl Plugin for ClientPostProcessPlugin {
    /// Keeps supported engine effects in one bundle shared by application and render tests.
    fn build(&self, app: &mut App) {
        app.add_plugins((
            bevy::post_process::msaa_writeback::MsaaWritebackPlugin,
            bevy::post_process::bloom::BloomPlugin,
        ));
    }
}
