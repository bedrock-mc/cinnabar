use super::{AssetStartupError, LoadedEntityAssets, shell_quote_path};
use assets::RuntimeActorCatalog;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

pub const ACTOR_ASSETS_FILENAME: &str = assets::carriers::ACTOR.output;
pub fn actor_asset_path(world: &Path) -> PathBuf {
    world.with_file_name(ACTOR_ASSETS_FILENAME)
}

/// Reads and decodes the actor carrier against the entity catalog startup already decoded.
pub fn require_actor_assets(
    world: &Path,
    entities: &LoadedEntityAssets,
) -> Result<RuntimeActorCatalog, AssetStartupError> {
    read_coherent_actor_assets(world, entities.runtime())
}

/// The neutral actor artwork pages for `catalog`; also installs the default player skin.
pub(crate) fn actor_artwork(
    catalog: &RuntimeActorCatalog,
    entities: &assets::RuntimeEntityAssets,
) -> render::ActorArtworkPages {
    if let Some(skin) = default_player_skin(catalog, entities) {
        render_model::install_default_player_skin(skin);
    }
    let artwork = render::ActorArtworkPages::new(catalog);
    diagnostics::log_stderr!(
        "loaded neutral unlit actor artwork: bindings={}, textures={}, page budget rejections={}, rest pose fallbacks={} (pose_expression_unverified); lighting/tint/overlay parity incomplete",
        catalog.bindings().len(),
        catalog.textures().len(),
        artwork.rejected_bindings(),
        catalog
            .bindings()
            .iter()
            .filter(|binding| binding.pose_mode == assets::ActorPoseMode::RestPose)
            .count()
    );
    artwork
}

/// The player entity's default texture from the carrier, when present.
fn default_player_skin(
    catalog: &RuntimeActorCatalog,
    entities: &assets::RuntimeEntityAssets,
) -> Option<std::sync::Arc<[u8]>> {
    catalog
        .textures()
        .iter()
        .find(|texture| {
            (texture.width, texture.height) == (64, 64)
                && entities
                    .sources()
                    .get(texture.source as usize)
                    .is_some_and(|source| {
                        source.path.as_ref() == render_model::DEFAULT_PLAYER_SKIN_PATH
                    })
        })
        .map(|texture| std::sync::Arc::clone(&texture.rgba8))
}

fn read_coherent_actor_assets(
    world: &Path,
    entities: &assets::RuntimeEntityAssets,
) -> Result<RuntimeActorCatalog, AssetStartupError> {
    let path = actor_asset_path(world);
    let command = format!(
        "make actor-assets ACTOR_ASSET_BLOB={} ACTOR_ASSET_REPORT={}",
        shell_quote_path(&path),
        shell_quote_path(&path.with_file_name("actor-assets.json"))
    );
    let error = |detail: String| AssetStartupError::ActorAssets {
        path: path.clone(),
        detail: detail.into(),
        rebuild_command: command.clone(),
    };
    let limit = assets::MAX_ACTOR_CARRIER_BYTES;
    let file = File::open(&path).map_err(|source| error(source.to_string()))?;
    if file
        .metadata()
        .map_err(|source| error(source.to_string()))?
        .len()
        > limit as u64
    {
        return Err(error("carrier exceeds startup byte bound".into()));
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|source| error(source.to_string()))?;
    if bytes.len() > limit {
        return Err(error("carrier exceeds startup byte bound".into()));
    }
    RuntimeActorCatalog::decode(&bytes, entities).map_err(|source| error(source.to_string()))
}

#[cfg(test)]
mod tests;
