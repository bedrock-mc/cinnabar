//! Entity, actor, equipment and item artwork compilation.

mod actor;
mod block_entity;
mod entity;
mod icon;
mod starter_skins;

pub use actor::{
    ActorCompileReport, ActorFallback, ActorPackCompilation, ActorTextureEvidence,
    CompiledActorCarrier, compile_actor_assets, compile_actor_assets_for_assets,
    compile_actor_pack, compile_actor_pack_unless,
};
pub use block_entity::{
    BlockEntityCompileReport, BlockEntityTexture, CompiledBlockEntityCarrier,
    compile_block_entity_assets, overlay_block_entity_assets,
};
pub use entity::{
    BlockMolang, BlockStateValue, CompileReferenceOutcome, EntityAssetCompilation,
    EntityPackCompilation, EntityPackSkips, EquipmentBindingDependencies, FallbackReason,
    ItemTagMolang, MAX_PACK_ENTITY_BYTES, MAX_PACK_ENTITY_SOURCES, RejectReason,
    compile_entity_assets, compile_entity_assets_with_behavior_report,
    compile_entity_assets_with_report, compile_entity_pack, compile_equipment_textures,
    compile_equipment_textures_for_assets, compile_equipment_textures_for_assets_with,
    compile_equipment_textures_with, compile_item_attack_timings, compile_item_foods,
    compile_item_use_durations, compile_item_use_profiles, compile_molang_expression,
    compile_vanilla_entity_refs, decode_equipment_raster, missing_harness_bindings,
    select_entity_geometry,
};
pub use icon::{
    CompiledIconCarrier, IconCompileReport, compile_icon_assets, compile_icon_assets_with_blocks,
    overlay_block_icon, overlay_block_icon_with_tint, overlay_cube_icon,
};
pub use starter_skins::{compile_starter_skins, compile_starter_skins_to_file};
