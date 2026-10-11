use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};

use assets::{MAX_PARTICLE_CARRIER_BYTES, RuntimeParticleAssets};

pub(crate) const PARTICLE_ASSETS_FILENAME: &str = assets::carriers::PARTICLE.output;

/// The particle carrier path beside the world carrier.
pub(crate) fn particle_asset_path(world_asset_path: &Path) -> PathBuf {
    world_asset_path.with_file_name(PARTICLE_ASSETS_FILENAME)
}

/// Loads the particle carrier if present and valid; particles are simply absent otherwise.
pub(crate) fn load_optional_carrier(world_asset_path: &Path) -> Option<Arc<RuntimeParticleAssets>> {
    let path = particle_asset_path(world_asset_path);
    let file = match File::open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            diagnostics::log_stderr!(
                "particle carrier not found at {}; particles are disabled (build with: make particle-assets)",
                path.display()
            );
            return None;
        }
        Err(error) => {
            diagnostics::log_stderr!(
                "could not open particle carrier {}: {error}; particles are disabled",
                path.display()
            );
            return None;
        }
    };
    let mut bytes = Vec::new();
    if let Err(error) = file
        .take(MAX_PARTICLE_CARRIER_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
    {
        diagnostics::log_stderr!(
            "could not read particle carrier {}: {error}; particles are disabled",
            path.display()
        );
        return None;
    }
    match RuntimeParticleAssets::decode(&bytes) {
        Ok(assets) => {
            diagnostics::log_stderr!(
                "loaded particle carrier from {} ({} effects, {} textures)",
                path.display(),
                assets.effects().len(),
                assets.textures().len()
            );
            Some(Arc::new(assets))
        }
        Err(error) => {
            diagnostics::log_stderr!(
                "invalid particle carrier {}: {error}; particles are disabled (rebuild with: make particle-assets)",
                path.display()
            );
            None
        }
    }
}
