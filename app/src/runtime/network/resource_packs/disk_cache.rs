//! Persistent block and icon subscribers share the entity cache's store and budget.
use super::*;
use pack_runtime::PackInputs;
use pack_runtime::compile_cache::{cache_key, part};
use sha2::Digest;

/// Immutable artwork and texture policy inputs shared by both subscribers.
#[derive(Clone, Debug)]
struct References {
    world: Option<[u8; 32]>,
    readers: Vec<[u8; 32]>,
    keys: Option<[u8; 32]>,
    terrain: std::collections::BTreeMap<String, String>,
    device: assets::texture::policy::TexturePolicy,
    terrain_policy: assets::texture::policy::TexturePolicy,
}

/// Names every immutable startup input alongside the ordered archives and device policy.
/// Synthetic carriers have no content identity, so their compiles remain uncached on disk.
fn visual_key(
    subscriber: &str,
    stack: &resource_pack::ValidatedPackStack,
    inputs: &impl std::fmt::Debug,
    max_layers: u32,
) -> Option<[u8; 32]> {
    let _ = pack_runtime::compile_cache::current()?;
    let base = match base_world_assets() {
        Some(base) => Some(base.carrier_identity()?),
        None => None,
    };
    let references = References {
        world: base,
        readers: stack
            .packs()
            .iter()
            .map(|pack| pack.compile_identity())
            .collect(),
        keys: BASE_MATERIAL_KEYS
            .get()
            .map(|(keys, _)| sha2::Sha256::digest(keys.to_json(0)).into()),
        terrain: pack_runtime::pack_textures::base_terrain_catalog()
            .into_iter()
            .collect(),
        device: assets::texture::policy::device_policy(),
        terrain_policy: assets::texture::policy::terrain_texture_policy(),
    };
    Some(key(
        subscriber,
        stack.fingerprint(),
        inputs,
        max_layers,
        &references,
    ))
}

/// Frames stack identities and derives every field of the input and reference records.
fn key(
    subscriber: &str,
    stack: &StackFingerprint,
    inputs: &impl std::fmt::Debug,
    max_layers: u32,
    references: &References,
) -> [u8; 32] {
    let mut key = cache_key(subscriber);
    for (id, version, subpack, content) in stack {
        for bytes in [
            id.as_bytes(),
            version.as_bytes(),
            subpack.as_bytes(),
            content,
        ] {
            part(&mut key, bytes);
        }
    }
    let References {
        world,
        readers,
        keys,
        terrain,
        device,
        terrain_policy,
    } = references;
    part(
        &mut key,
        format!("{world:?}{readers:?}{keys:?}{terrain:?}{device:?}{terrain_policy:?}{inputs:?}")
            .as_bytes(),
    );
    part(&mut key, &max_layers.to_le_bytes());
    key.finalize().into()
}

/// Includes all custom block facts, hash ordering, base artwork and adapter limits.
pub(super) fn block_key(
    stack: &resource_pack::ValidatedPackStack,
    blocks: &protocol::CustomBlocks,
    hashed: bool,
    max_layers: u32,
) -> Option<[u8; 32]> {
    visual_key("blocks", stack, &(blocks, hashed), max_layers)
}

/// Icons additionally depend on registry mappings and the installed item routes and sprites.
pub(super) fn icon_key(
    stack: &resource_pack::ValidatedPackStack,
    inputs: &PackInputs,
    max_layers: u32,
) -> Option<[u8; 32]> {
    let installed = match super::super::item_icons::vanilla::CACHE_IDENTITY.get() {
        Some(identity) => Some((*identity)?),
        None => None,
    };
    let icons = inputs
        .icons
        .iter()
        .map(|(id, key)| (id, key))
        .collect::<std::collections::BTreeMap<_, _>>();
    visual_key(
        "icons",
        stack,
        &(
            &inputs.blocks,
            inputs.hashed,
            icons,
            &inputs.block_items,
            installed,
        ),
        max_layers,
    )
}

/// Restores recorded reads with a hit and recompiles any missing or invalid payload.
pub(super) fn compile<T: serde::Serialize + serde::de::DeserializeOwned>(
    view: &LayeredPackView,
    key: Option<[u8; 32]>,
    build: impl FnOnce() -> T,
) -> T {
    match pack_runtime::compile_cache::current().zip(key) {
        Some((disk, key)) => disk.get_or_compile(&key, view, build),
        None => build(),
    }
}

#[cfg(test)]
mod tests;
