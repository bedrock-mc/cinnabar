//! Local component hosts and trusted server experience input.

#[cfg(feature = "local-mods")]
pub mod modding;
pub mod server_experiences;

use bevy::prelude::*;
use client_state::{ClientFrameSet, ConsentInput, ExperienceInputSet};

/// Installs extension-owned systems at the shared client frame boundaries.
pub struct ClientExtensionsPlugin;

impl Plugin for ClientExtensionsPlugin {
    /// Consumes consent-owned input before semantic sampling and ordinary UI authority.
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsentInput>().add_systems(
            Update,
            server_experiences::input::consume
                .in_set(ExperienceInputSet)
                .after(ClientFrameSet::RawInput)
                .before(ClientFrameSet::SemanticSample),
        );
    }
}
