use super::{AssetStartupError, LoadedEntityAssets, shell_quote_path};
use crate::RuntimeActorCatalog;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

pub const ACTOR_ASSETS_FILENAME: &str = crate::carriers::ACTOR.output;
/// Selects the actor carrier beside the world carrier.
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

/// Reads the bounded actor carrier and verifies its entity catalog identity.
fn read_coherent_actor_assets(
    world: &Path,
    entities: &crate::RuntimeEntityAssets,
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
    let limit = crate::MAX_ACTOR_CARRIER_BYTES;
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
