//! Reusable Bedrock resource-pack compilation for runtime and offline callers.
//!
//! Compilation produces engine-independent `assets` data. Command-line argument
//! parsing and publication of offline output bundles belong to `asset-compiler`.

mod actor;
mod animation;
mod atmosphere;
mod audio;
mod audio_bank;
mod audio_pcm;
mod biome;
mod block_entity;
mod compiler;
mod entity;
mod fadpcm;
mod font;
mod hud;
mod hud_extras;
mod icon;
mod image;
mod lang;
mod pack;
mod particle;
mod ui;
mod weather_textures;

pub use actor::{
    ActorCompileReport, ActorFallback, ActorPackCompilation, ActorTextureEvidence,
    CompiledActorCarrier, compile_actor_assets, compile_actor_pack,
};
pub use animation::AnimationInventory;
pub use assets::BlockFace;
pub use atmosphere::{
    AtmosphereCompileOptions, compile_atmosphere_assets, compile_atmosphere_assets_with_options,
};
pub use audio::{
    AUDIO_SOUND_DEFINITIONS_RELATIVE_PATH, AudioCompileError, AudioCompileReport,
    CompiledAudioCarrier, PINNED_SOUND_DEFINITIONS_SHA256, compile_audio_assets,
};
pub use audio_bank::{
    AudioBankCompileError, AudioBankCompileReport, CompiledAudioBank, compile_audio_bank,
};
pub use audio_pcm::{
    AudioPcmCompileError, AudioPcmCompileReport, CompiledAudioPcmCarrier, compile_audio_pcm_assets,
};
pub use biome::compile_biome_assets;
pub use block_entity::{
    BlockEntityCompileReport, CompiledBlockEntityCarrier, compile_block_entity_assets,
};
pub use compiler::{
    compile_pack, compile_pack_with_biomes, compile_pack_with_material_keys,
    inspect_animation_inventory,
};
pub use entity::{
    CompileReferenceOutcome, EntityAssetCompilation, EntityPackCompilation, EntityPackSkips,
    FallbackReason, MAX_PACK_ENTITY_BYTES, MAX_PACK_ENTITY_SOURCES, RejectReason,
    compile_entity_assets, compile_entity_assets_with_report, compile_entity_pack,
    compile_equipment_textures, compile_equipment_textures_for_assets,
    compile_equipment_textures_for_assets_with, compile_equipment_textures_with,
    compile_item_use_durations, compile_vanilla_entity_refs,
};
pub use fadpcm::{DecodedFadpcm, FadpcmDecodeError, decode_fsb5_fadpcm};
pub use font::{
    CompiledFontCarrier, FontCompileError, FontCompileReport, GlyphAdvances, OutlineFontConfig,
    compile_fonts, compile_outline_font, compile_outline_font_with_fallback,
};
pub use hud::{CompiledHudCarrier, HudCompileError, HudCompileReport, compile_hud_assets};
pub use hud_extras::compile_hud_extras_to_file;
pub use icon::{
    CompiledIconCarrier, IconCompileReport, compile_icon_assets, compile_icon_assets_with_blocks,
    overlay_block_icon,
};
pub use lang::{
    CompiledLangCarrier, LangCompileError, LangCompileReport, compile_lang_assets,
    compile_language, vanilla_language_codes,
};
pub use pack::{
    BlockTextureMap, FlipbookSource, MAX_FLIPBOOK_FRAMES, MAX_FLIPBOOKS, PackSources,
    TerrainTextureMap, TextureKey, read_pack, resolve_texture_key,
};
pub use particle::{
    CompiledParticleCarrier, ParticleCompileReport, compile_particle_assets,
    decode_particle_carrier,
};
pub use ui::{CompiledUiCarrier, UiCompileReport, compile_ui_assets, decode_ui_carrier};
pub use weather_textures::{compile_weather_textures, compile_weather_textures_to_file};
