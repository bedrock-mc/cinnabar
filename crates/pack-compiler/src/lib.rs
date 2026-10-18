//! Reusable Bedrock resource-pack compilation for runtime and offline callers.
//!
//! Compilation produces engine-independent `assets` data. Command-line argument
//! parsing and publication of offline output bundles belong to `asset-compiler`.

mod animation;
mod atmosphere;
#[cfg(not(target_arch = "wasm32"))]
mod audio;
mod audio_bank;
mod audio_pcm;
mod biome;
pub mod block_geometry;
mod compiler;
mod fadpcm;
mod hud;
mod hud_extras;
mod lang;
mod oreui_icons;
mod particle;
pub mod pbr;
mod ui;
mod weather_textures;

pub use animation::AnimationInventory;

pub use atmosphere::{
    AtmosphereCompileOptions, compile_atmosphere_assets, compile_atmosphere_assets_with_options,
};
#[cfg(not(target_arch = "wasm32"))]
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
pub use compiler::{
    compile_pack, compile_pack_with_biomes, compile_pack_with_material_keys,
    inspect_animation_inventory,
};
pub use fadpcm::{DecodedFadpcm, FadpcmDecodeError, decode_fsb5_fadpcm};
pub use hud::{CompiledHudCarrier, HudCompileError, HudCompileReport, compile_hud_assets};
pub use hud_extras::compile_hud_extras_to_file;
pub use lang::{
    CompiledLangCarrier, LangCompileError, LangCompileReport, compile_lang_assets,
    compile_language, vanilla_language_codes,
};
pub use oreui_icons::{compile_oreui_icons_to_file, oreui_icon_sources};
pub use particle::{
    CompiledParticleCarrier, ParticleCompileReport, compile_particle_assets,
    decode_particle_carrier,
};
pub use ui::{CompiledUiCarrier, UiCompileReport, compile_ui_assets, decode_ui_carrier};
pub use weather_textures::{compile_weather_textures, compile_weather_textures_to_file};
