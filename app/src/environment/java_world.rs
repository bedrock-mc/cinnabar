//! Startup and frame adaptation for the optional Java world presentation.

use assets::java_world::{CARRIER_FILENAME, MAX_CARRIER_BYTES, decode_java_world};
use render_scene::JavaWorldVisuals;
use sha2::{Digest, Sha256};
use std::{io::Read, path::Path, sync::Arc};

/// Loads the optional sibling carrier without weakening required Bedrock asset validation.
pub(crate) fn load(
    world_path: &Path,
    bedrock: &assets::RuntimeAtmosphereAssets,
    world: &assets::RuntimeAssets,
) -> JavaWorldVisuals {
    let path = world_path.with_file_name(CARRIER_FILENAME);
    let load = || -> Result<_, Box<dyn std::error::Error + Send + Sync>> {
        let mut bytes = Vec::new();
        std::fs::File::open(&path)?
            .take(MAX_CARRIER_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        let assets = decode_java_world(&bytes)?;
        Ok(JavaWorldVisuals::new(
            Arc::new(assets),
            Sha256::digest(&bytes).into(),
            bedrock,
            world,
        ))
    };
    match load() {
        Ok(assets) => assets,
        Err(error) => {
            bevy::log::warn!(path = %path.display(), %error, "Java world visuals unavailable; run make assets to prepare their local resources");
            JavaWorldVisuals::default()
        }
    }
}
