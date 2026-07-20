use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::{
    AssetError,
    item::{ItemVisualAlias, ItemVisualDefinition},
};

use super::super::{
    CompiledEntityAssets, ENTITY_BLOB_MAGIC, ENTITY_BLOB_VERSION, EntityAssetSource,
    EntityAssetSymbol, EntityGeometry, HASH_BYTES, HEADER_BYTES, MAX_ENTITY_CATALOG_BYTES,
    RuntimeEntityAssets, invalid, validate_compiled,
};
use super::{
    CompiledMolangExpression, EntityAnimationChannel, EntityAnimationClip,
    EntityAnimationController, EntityAnimationKeyframe, EntityControllerAnimation,
    EntityControllerState, EntityControllerTransition, EntityRigAnimationBinding, EntityRigBinding,
    EntityRigControllerBinding, EntityRigGeometryBinding, EntityRigTexture, MolangCollection,
    MolangCollectionItem, MolangOp, MolangSymbol,
};

#[derive(Serialize)]
#[serde(deny_unknown_fields)]
struct EntityCatalogPayloadRef<'a> {
    block_visual_count: u32,
    sources: &'a [EntityAssetSource],
    symbols: &'a [EntityAssetSymbol],
    geometries: &'a [EntityGeometry],
    animation_clips: &'a [EntityAnimationClip],
    animation_channels: &'a [EntityAnimationChannel],
    animation_keyframes: &'a [EntityAnimationKeyframe],
    molang_symbols: &'a [MolangSymbol],
    molang_expressions: &'a [CompiledMolangExpression],
    molang_ops: &'a [MolangOp],
    molang_collections: &'a [MolangCollection],
    molang_collection_items: &'a [MolangCollectionItem],
    controllers: &'a [EntityAnimationController],
    controller_states: &'a [EntityControllerState],
    controller_animations: &'a [EntityControllerAnimation],
    controller_transitions: &'a [EntityControllerTransition],
    rig_bindings: &'a [EntityRigBinding],
    rig_geometries: &'a [EntityRigGeometryBinding],
    rig_animations: &'a [EntityRigAnimationBinding],
    rig_controllers: &'a [EntityRigControllerBinding],
    rig_textures: &'a [EntityRigTexture],
    item_visuals: &'a [ItemVisualDefinition],
    item_visual_aliases: &'a [ItemVisualAlias],
}

pub(crate) fn encode_compiled(compiled: &CompiledEntityAssets) -> Result<Box<[u8]>, AssetError> {
    validate_compiled(compiled)?;
    encode(
        compiled.source_manifest_sha256,
        EntityCatalogPayloadRef {
            block_visual_count: compiled.block_visual_count,
            sources: &compiled.sources,
            symbols: &compiled.symbols,
            geometries: &compiled.geometries,
            animation_clips: &compiled.animation_clips,
            animation_channels: &compiled.animation_channels,
            animation_keyframes: &compiled.animation_keyframes,
            molang_symbols: &compiled.molang_symbols,
            molang_expressions: &compiled.molang_expressions,
            molang_ops: &compiled.molang_ops,
            molang_collections: &compiled.molang_collections,
            molang_collection_items: &compiled.molang_collection_items,
            controllers: &compiled.controllers,
            controller_states: &compiled.controller_states,
            controller_animations: &compiled.controller_animations,
            controller_transitions: &compiled.controller_transitions,
            rig_bindings: &compiled.rig_bindings,
            rig_geometries: &compiled.rig_geometries,
            rig_animations: &compiled.rig_animations,
            rig_controllers: &compiled.rig_controllers,
            rig_textures: &compiled.rig_textures,
            item_visuals: &compiled.item_visuals,
            item_visual_aliases: &compiled.item_visual_aliases,
        },
    )
}

pub(crate) fn encode_runtime(runtime: &RuntimeEntityAssets) -> Result<Box<[u8]>, AssetError> {
    encode(
        runtime.source_manifest_sha256,
        EntityCatalogPayloadRef {
            block_visual_count: runtime.block_visual_count,
            sources: &runtime.sources,
            symbols: &runtime.symbols,
            geometries: &runtime.geometries,
            animation_clips: &runtime.animation_clips,
            animation_channels: &runtime.animation_channels,
            animation_keyframes: &runtime.animation_keyframes,
            molang_symbols: &runtime.molang_symbols,
            molang_expressions: &runtime.molang_expressions,
            molang_ops: &runtime.molang_ops,
            molang_collections: &runtime.molang_collections,
            molang_collection_items: &runtime.molang_collection_items,
            controllers: &runtime.controllers,
            controller_states: &runtime.controller_states,
            controller_animations: &runtime.controller_animations,
            controller_transitions: &runtime.controller_transitions,
            rig_bindings: &runtime.rig_bindings,
            rig_geometries: &runtime.rig_geometries,
            rig_animations: &runtime.rig_animations,
            rig_controllers: &runtime.rig_controllers,
            rig_textures: &runtime.rig_textures,
            item_visuals: &runtime.item_visuals,
            item_visual_aliases: &runtime.item_visual_aliases,
        },
    )
}

fn encode(
    source_manifest_sha256: [u8; 32],
    payload: EntityCatalogPayloadRef<'_>,
) -> Result<Box<[u8]>, AssetError> {
    let mut bytes = vec![0; HEADER_BYTES];
    serde_json::to_writer(&mut bytes, &payload)
        .map_err(|_| invalid("failed to encode MCBEENT6 catalog payload"))?;
    let payload_bytes = bytes.len() - HEADER_BYTES;
    if payload_bytes > MAX_ENTITY_CATALOG_BYTES {
        return Err(invalid("MCBEENT6 catalog payload exceeds bound"));
    }
    bytes[0..8].copy_from_slice(&ENTITY_BLOB_MAGIC);
    bytes[8..12].copy_from_slice(&ENTITY_BLOB_VERSION.to_le_bytes());
    write_count(&mut bytes, 12, payload.sources.len(), "source")?;
    write_count(&mut bytes, 16, payload.symbols.len(), "symbol")?;
    write_count(&mut bytes, 20, payload.geometries.len(), "geometry")?;
    bytes[24..56].copy_from_slice(&source_manifest_sha256);
    bytes[56..64].copy_from_slice(
        &u64::try_from(payload_bytes)
            .map_err(|_| invalid("MCBEENT6 payload length overflow"))?
            .to_le_bytes(),
    );
    write_count(
        &mut bytes,
        64,
        payload.animation_clips.len(),
        "animation clip",
    )?;
    write_count(&mut bytes, 68, payload.controllers.len(), "controller")?;
    write_count(&mut bytes, 72, payload.rig_bindings.len(), "rig binding")?;
    write_count(&mut bytes, 76, payload.item_visuals.len(), "item visual")?;
    let digest = Sha256::digest(&bytes);
    bytes.reserve(HASH_BYTES);
    bytes.extend_from_slice(&digest);
    Ok(bytes.into_boxed_slice())
}

fn write_count(
    bytes: &mut [u8],
    offset: usize,
    count: usize,
    section: &str,
) -> Result<(), AssetError> {
    bytes[offset..offset + 4].copy_from_slice(
        &u32::try_from(count)
            .map_err(|_| invalid(format!("MCBEENT6 {section} count overflow")))?
            .to_le_bytes(),
    );
    Ok(())
}
