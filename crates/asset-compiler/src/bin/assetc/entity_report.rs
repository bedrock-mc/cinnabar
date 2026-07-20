use asset_compiler::CompileReferenceOutcome;
use assets::{EntityAssetSource, EntityAssetSymbol};
use serde::Serialize;

#[derive(Serialize)]
pub(super) struct EntityAssetsReport<'a> {
    pub(super) schema: u32,
    pub(super) source: serde_json::Value,
    pub(super) source_manifest_sha256: Box<str>,
    pub(super) blob_sha256: Box<str>,
    pub(super) counts: EntityAssetCounts,
    pub(super) sources: &'a [EntityAssetSource],
    pub(super) symbols: &'a [EntityAssetSymbol],
    pub(super) reference_outcomes: &'a [CompileReferenceOutcome<u32>],
}

#[derive(Serialize)]
pub(super) struct EntityAssetCounts {
    pub(super) sources: usize,
    pub(super) symbols: usize,
    pub(super) dependencies: usize,
    pub(super) geometries: usize,
    pub(super) bones: usize,
    pub(super) cubes: usize,
    pub(super) animation_clips: usize,
    pub(super) animation_channels: usize,
    pub(super) animation_keyframes: usize,
    pub(super) molang_symbols: usize,
    pub(super) molang_expressions: usize,
    pub(super) molang_ops: usize,
    pub(super) molang_collections: usize,
    pub(super) molang_collection_items: usize,
    pub(super) controllers: usize,
    pub(super) controller_states: usize,
    pub(super) controller_animations: usize,
    pub(super) controller_transitions: usize,
    pub(super) rig_bindings: usize,
    pub(super) rig_geometry_candidates: usize,
    pub(super) rig_animations: usize,
    pub(super) rig_controllers: usize,
    pub(super) rig_textures: usize,
    pub(super) rig_texture_bytes: usize,
    pub(super) rig_geometry_selections: usize,
    pub(super) item_visuals: usize,
    pub(super) item_visual_aliases: usize,
    pub(super) item_sprite_routes: usize,
    pub(super) item_block_routes: usize,
    pub(super) item_empty_hand_routes: usize,
    pub(super) item_missing_routes: usize,
    pub(super) block_visuals: usize,
}
