//! Immutable entity carrier admission and retained runtime assets.

use std::sync::Arc;

use super::*;

#[derive(Clone, Debug)]
pub struct RuntimeEntityAssets {
    pub(super) item_use_profiles: Arc<[item_use::ItemUseDescriptor]>,
    pub(super) seat_layouts: Arc<[seats::EntitySeatDescriptor]>,
    pub(super) interaction_traits: Arc<[interactions::EntityInteractionDescriptor]>,
    pub(super) carrier_identity: Option<[u8; 32]>, // SHA-256 of the decoded carrier file
    pub(super) source_manifest_sha256: [u8; 32],
    pub(super) block_visual_count: u32,
    pub(super) sources: Arc<[EntityAssetSource]>,
    pub(super) symbols: Arc<[EntityAssetSymbol]>,
    pub(super) geometries: Arc<[EntityGeometry]>,
    pub(super) geometry_parents: Arc<[Option<usize>]>,
    pub(super) animation_clips: Arc<[EntityAnimationClip]>,
    pub(super) animation_channels: Arc<[EntityAnimationChannel]>,
    pub(super) animation_keyframes: Arc<[EntityAnimationKeyframe]>,
    pub(super) molang_symbols: Arc<[MolangSymbol]>,
    pub(super) molang_query_bindings: Box<[Option<MolangQuery>]>,
    pub(super) molang_expressions: Arc<[CompiledMolangExpression]>,
    pub(super) molang_ops: Arc<[MolangOp]>,
    pub(super) molang_collections: Arc<[MolangCollection]>,
    pub(super) molang_collection_items: Arc<[MolangCollectionItem]>,
    pub(super) controllers: Arc<[EntityAnimationController]>,
    pub(super) controller_states: Arc<[EntityControllerState]>,
    pub(super) controller_animations: Arc<[EntityControllerAnimation]>,
    pub(super) controller_transitions: Arc<[EntityControllerTransition]>,
    pub(super) rig_bindings: Arc<[EntityRigBinding]>,
    pub(super) rig_geometries: Arc<[EntityRigGeometryBinding]>,
    pub(super) rig_animations: Arc<[EntityRigAnimationBinding]>,
    pub(super) rig_controllers: Arc<[EntityRigControllerBinding]>,
    pub(super) item_visuals: Arc<[ItemVisualDefinition]>,
    pub(super) item_visual_aliases: Arc<[ItemVisualAlias]>,
    pub(super) render: Arc<EntityRenderData>,
}

impl RuntimeEntityAssets {
    pub fn decode(bytes: &[u8]) -> Result<Self, AssetError> {
        if bytes.len() < HEADER_BYTES + HASH_BYTES {
            return Err(invalid("truncated MCBEENT4 blob"));
        }
        if bytes[..8] != ENTITY_BLOB_MAGIC || u32_at(bytes, 8)? != ENTITY_BLOB_VERSION {
            return Err(invalid(
                "unsupported MCBEENT4 header; rebuild with make assets",
            ));
        }
        let source_count = u32_at(bytes, 12)? as usize;
        let symbol_count = u32_at(bytes, 16)? as usize;
        let geometry_count = u32_at(bytes, 20)? as usize;
        let animation_clip_count = u32_at(bytes, 64)? as usize;
        let controller_count = u32_at(bytes, 68)? as usize;
        let rig_binding_count = u32_at(bytes, 72)? as usize;
        let item_visual_count = u32_at(bytes, 76)? as usize;
        if source_count == 0
            || source_count > MAX_ENTITY_ASSET_SOURCES
            || symbol_count == 0
            || symbol_count > MAX_ENTITY_ASSET_SYMBOLS
            || geometry_count > MAX_ENTITY_GEOMETRIES
            || animation_clip_count > MAX_ENTITY_ANIMATION_CLIPS
            || controller_count > MAX_ENTITY_CONTROLLERS
            || rig_binding_count > MAX_ENTITY_RIG_BINDINGS
            || item_visual_count > crate::item::MAX_ITEM_VISUALS
        {
            return Err(invalid("MCBEENT4 header counts exceed bounds"));
        }
        let source_manifest_sha256 = array_at::<32>(bytes, 24)?;
        let payload_bytes = usize::try_from(u64::from_le_bytes(array_at(bytes, 56)?))
            .map_err(|_| invalid("MCBEENT4 payload size exceeds platform"))?;
        if payload_bytes > MAX_ENTITY_CATALOG_BYTES
            || bytes.len()
                != HEADER_BYTES
                    .checked_add(payload_bytes)
                    .and_then(|length| length.checked_add(HASH_BYTES))
                    .ok_or_else(|| invalid("MCBEENT4 length overflow"))?
        {
            return Err(invalid("noncanonical MCBEENT4 section layout"));
        }
        let payload_end = HEADER_BYTES + payload_bytes;
        let identity = crate::encoding::sealed_identity(bytes, payload_end)
            .ok_or_else(|| invalid("MCBEENT4 envelope hash mismatch"))?;
        let payload_counts = v4::payload_counts(&bytes[HEADER_BYTES..payload_end])
            .map_err(|_| invalid("invalid MCBEENT4 catalog count preflight"))?;
        if payload_counts
            != [
                source_count,
                symbol_count,
                geometry_count,
                animation_clip_count,
                controller_count,
                rig_binding_count,
                item_visual_count,
            ]
        {
            return Err(invalid("MCBEENT4 catalog counts do not match header"));
        }
        let payload: EntityCatalogPayload =
            serde_json::from_slice(&bytes[HEADER_BYTES..payload_end])
                .map_err(|_| invalid("invalid MCBEENT4 catalog payload"))?;
        let canonical = serde_json::to_vec(&payload)
            .map_err(|_| invalid("failed to canonicalize MCBEENT4 catalog payload"))?;
        if canonical.as_slice() != &bytes[HEADER_BYTES..payload_end] {
            return Err(invalid("noncanonical MCBEENT4 catalog encoding"));
        }
        let compiled = CompiledEntityAssets {
            item_use_profiles: payload.item_use_profiles,
            seat_layouts: payload.seat_layouts,
            interaction_traits: payload.interaction_traits,
            source_manifest_sha256,
            block_visual_count: payload.block_visual_count,
            sources: payload.sources,
            symbols: payload.symbols,
            geometries: payload.geometries,
            animation_clips: payload.animation_clips,
            animation_channels: payload.animation_channels,
            animation_keyframes: payload.animation_keyframes,
            molang_symbols: payload.molang_symbols,
            molang_expressions: payload.molang_expressions,
            molang_ops: payload.molang_ops,
            molang_collections: payload.molang_collections,
            molang_collection_items: payload.molang_collection_items,
            controllers: payload.controllers,
            controller_states: payload.controller_states,
            controller_animations: payload.controller_animations,
            controller_transitions: payload.controller_transitions,
            rig_bindings: payload.rig_bindings,
            rig_geometries: payload.rig_geometries,
            rig_animations: payload.rig_animations,
            rig_controllers: payload.rig_controllers,
            item_visuals: payload.item_visuals,
            item_visual_aliases: payload.item_visual_aliases,
            render: payload.render,
        };
        Ok(Self {
            carrier_identity: Some(identity),
            ..Self::from_compiled(compiled)?
        })
    }

    /// [`Self::from_compiled`] plus the catalog's carrier encoding, if it has one. The identity then
    /// matches what a decode of that encoding reports.
    pub fn from_compiled_encoded(
        compiled: CompiledEntityAssets,
    ) -> Result<(Self, Option<EntityCarrierBlob>), AssetError> {
        use sha2::{Digest, Sha256};
        let blob = encode_entity_blob(&compiled).ok();
        let assets = Self {
            carrier_identity: blob.as_deref().map(|blob| Sha256::digest(blob).into()),
            ..Self::from_compiled(compiled)?
        };
        Ok((assets, blob))
    }

    /// Validates a compiled catalog and wraps it without a blob round trip.
    pub fn from_compiled(compiled: CompiledEntityAssets) -> Result<Self, AssetError> {
        let geometry_parents = validate_compiled(&compiled)?;
        Ok(Self {
            carrier_identity: None,
            item_use_profiles: Arc::from(compiled.item_use_profiles),
            seat_layouts: Arc::from(compiled.seat_layouts),
            interaction_traits: Arc::from(compiled.interaction_traits),
            source_manifest_sha256: compiled.source_manifest_sha256,
            block_visual_count: compiled.block_visual_count,
            sources: Arc::from(compiled.sources),
            symbols: Arc::from(compiled.symbols),
            geometries: Arc::from(compiled.geometries),
            geometry_parents: Arc::from(geometry_parents),
            animation_clips: Arc::from(compiled.animation_clips),
            animation_channels: Arc::from(compiled.animation_channels),
            animation_keyframes: Arc::from(compiled.animation_keyframes),
            molang_query_bindings: bind_molang_queries(&compiled.molang_symbols),
            molang_symbols: Arc::from(compiled.molang_symbols),
            molang_expressions: Arc::from(compiled.molang_expressions),
            molang_ops: Arc::from(compiled.molang_ops),
            molang_collections: Arc::from(compiled.molang_collections),
            molang_collection_items: Arc::from(compiled.molang_collection_items),
            controllers: Arc::from(compiled.controllers),
            controller_states: Arc::from(compiled.controller_states),
            controller_animations: Arc::from(compiled.controller_animations),
            controller_transitions: Arc::from(compiled.controller_transitions),
            rig_bindings: Arc::from(compiled.rig_bindings),
            rig_geometries: Arc::from(compiled.rig_geometries),
            rig_animations: Arc::from(compiled.rig_animations),
            rig_controllers: Arc::from(compiled.rig_controllers),
            item_visuals: Arc::from(compiled.item_visuals),
            item_visual_aliases: Arc::from(compiled.item_visual_aliases),
            render: Arc::new(compiled.render),
        })
    }

    /// The SHA-256 of the carrier this was decoded from or encoded as; `None` for a bare compiled catalog.
    #[must_use]
    pub const fn carrier_identity(&self) -> Option<[u8; 32]> {
        self.carrier_identity
    }

    #[must_use]
    pub const fn source_manifest_sha256(&self) -> [u8; 32] {
        self.source_manifest_sha256
    }

    #[must_use]
    pub const fn block_visual_count(&self) -> u32 {
        self.block_visual_count
    }

    #[must_use]
    pub fn sources(&self) -> &[EntityAssetSource] {
        &self.sources
    }

    #[must_use]
    pub fn symbols(&self) -> &[EntityAssetSymbol] {
        &self.symbols
    }

    #[must_use]
    pub fn geometries(&self) -> &[EntityGeometry] {
        &self.geometries
    }

    /// Selected inheritance parents validated when this immutable catalog was admitted.
    #[must_use]
    pub fn geometry_parents(&self) -> &[Option<usize>] {
        &self.geometry_parents
    }

    #[must_use]
    pub fn geometry_candidates(&self, identifier: &str) -> &[EntityGeometry] {
        let start = self
            .geometries
            .partition_point(|geometry| geometry.identifier.as_ref() < identifier);
        let matching = &self.geometries[start..];
        let length =
            matching.partition_point(|geometry| geometry.identifier.as_ref() == identifier);
        &matching[..length]
    }

    #[must_use]
    pub fn symbol_candidates(
        &self,
        kind: EntityAssetKind,
        identifier: &str,
    ) -> &[EntityAssetSymbol] {
        symbol_candidates(&self.symbols, kind, identifier)
    }
}
