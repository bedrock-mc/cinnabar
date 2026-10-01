//! Bounded Bedrock resource-pack source readers.

mod actor;
mod atmosphere;
mod audio;
mod audio_pcm;
mod biome;
mod blob;
mod block_entity;
mod block_visibility;
mod compiled;
mod entity;
mod environment_settings;
mod equipment;
mod error;
mod font;
mod fsb;
mod glyph_sheet;
mod hud;
mod hud_extras;
mod icon;
mod item;
mod lang;
mod light_registry;
mod material_keys;
mod model;
mod ogg;
mod particle;
mod physics_registry;
mod provenance;
mod registry;
mod runtime;
mod server_lang;
mod skin_geometry;
mod sound_bank;
mod sound_events;
mod texture;
mod ui;
mod vanilla_refs;
mod weather_textures;

pub use hud_extras::{
    HUD_EXTRA_SIDE, HUD_EXTRAS_MAGIC, HUD_EXTRAS_VERSION, HudExtraRole, HudExtras, HudExtrasError,
    MAX_HUD_EXTRAS_BYTES, decode_hud_extras, encode_hud_extras,
};
pub use skin_geometry::{
    MAX_SKIN_GEOMETRY_BONES, MAX_SKIN_GEOMETRY_CUBES, MAX_SKIN_GEOMETRY_VERTICES, SkinGeometry,
    SkinGeometryBounds, SkinGeometryError, SkinPolyMesh, SkinPolyVertex, parse_skin_geometry,
    parse_skin_geometry_layer, skin_geometry_name,
};

pub use actor::{
    ACTOR_CARRIER_MAGIC, ACTOR_CARRIER_VERSION, ActorArtworkBinding, ActorPoseMode, ActorTexture,
    MAX_ACTOR_BINDINGS, MAX_ACTOR_CARRIER_BYTES, MAX_ACTOR_PIXEL_BYTES, MAX_ACTOR_TEXTURE_SIDE,
    MAX_ACTOR_TEXTURES, RuntimeActorCatalog, encode_actor_catalog,
    neutral_actor_geometry_uvs_are_supported, neutral_actor_material_is_supported,
    neutral_actor_pose_mode,
};
pub use atmosphere::{
    ATMOSPHERE_BLOB_MAGIC, ATMOSPHERE_BLOB_VERSION, AtmosphereRole, AtmosphereTexture,
    BiomeVisualProfile, CelestialBorderTexel, CelestialTile, CompiledAtmosphereAssets, FogDistance,
    FogDistanceMode, FogMedium, FogProfile, MAX_ENVIRONMENT_IDENTIFIER_BYTES,
    MAX_ENVIRONMENT_PROFILES, MAX_FOG_DISTANCES, ResolvedFog, RuntimeAtmosphereAssets,
    composite_celestial, encode_atmosphere_blob,
};
pub use audio::{
    AUDIO_CARRIER_MAGIC, AudioAlternative, AudioCatalogError, AudioDefinition,
    MAX_AUDIO_ALTERNATIVES, MAX_AUDIO_ALTERNATIVES_PER_DEFINITION, MAX_AUDIO_CARRIER_BYTES,
    MAX_AUDIO_CATEGORY_BYTES, MAX_AUDIO_DEFINITIONS, MAX_AUDIO_IDENTIFIER_BYTES,
    MAX_AUDIO_PATH_BYTES, MAX_AUDIO_SUBTITLE_BYTES, RuntimeAudioCatalog, encode_audio_catalog,
};
pub use audio_pcm::{
    AudioPcmError, AudioPcmExpectedIdentity, AudioPcmMode, MAX_AUDIO_PCM_BYTES,
    MAX_AUDIO_PCM_CARRIER_BYTES, MAX_AUDIO_PCM_SOURCE_BYTES, RuntimeAudioPcm, encode_audio_pcm,
    reviewed_audio_pcm_identity, validate_audio_pcm_catalog,
};
pub use biome::{
    BIOME_REGISTRY_MAGIC, BIOME_RULE_FLAG_GRASS_SHADED, BIOME_TINT_FLAG_SWAMP_GRASS,
    BiomeRegistryRecord, BiomeRule, CompiledBiomeAssets, LinearBiomeTints, LiveBiomeDefinition,
    MAX_BIOME_NAME_BYTES, MAX_BIOME_NAMES_BYTES, MAX_BIOME_RULES, MISSING_BIOME_DENSE_INDEX,
    RAW_BIOME_ID_COUNT, ResolvedBiomeTints, TINT_MAP_BYTES, TINT_MAP_COUNT, TINT_MAP_SIZE,
    TintMapId, TintSource, colormap_coordinate, read_biome_registry,
};
pub use blob::{BLOB_MAGIC, BLOB_VERSION, encode_blob, write_blob_atomic};
pub use block_entity::{
    BLOCK_ENTITY_CARRIER_MAGIC, BLOCK_ENTITY_CARRIER_VERSION, BLOCK_ENTITY_ROUTES,
    BlockEntityPlacement, BlockEntityRouteKind, MAX_BLOCK_ENTITY_ATLAS_SIDE,
    MAX_BLOCK_ENTITY_CARRIER_BYTES, MAX_BLOCK_ENTITY_KEY_BYTES, MAX_BLOCK_ENTITY_PLACEMENTS,
    RuntimeBlockEntityAssets, block_entity_route, encode_block_entity_catalog,
};
pub use block_visibility::is_default_invisible_block;
pub use compiled::{
    BlockFace, BlockVisual, CompiledAssets, DIAGNOSTIC_MATERIAL, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_BIRCH_FOLIAGE, MATERIAL_FLAG_DRY_FOLIAGE,
    MATERIAL_FLAG_EVERGREEN_FOLIAGE, MATERIAL_FLAG_FOLIAGE_CLASS_MASK, MATERIAL_FLAG_FOLIAGE_TINT,
    MATERIAL_FLAG_GRASS_TINT, MATERIAL_FLAG_LIQUID_DEPTH_WRITE, MATERIAL_FLAG_OVERLAY_MASK,
    MATERIAL_FLAG_ROTATE_UV, MATERIAL_FLAG_TINT_MASK, MATERIAL_FLAG_UV_MASK,
    MATERIAL_FLAG_WATER_TINT, MATERIAL_FLAGS_MASK, MAX_MATERIALS, MAX_TEXTURE_LAYERS, Material,
};
pub use entity::{
    CompiledEntityAssets, CompiledMolangExpression, ENTITY_BLOB_MAGIC, ENTITY_BLOB_VERSION,
    EntityAnimationChannel, EntityAnimationClip, EntityAnimationController,
    EntityAnimationInterpolation, EntityAnimationKeyframe, EntityAnimationLoop,
    EntityAnimationProperty, EntityAssetKind, EntityAssetSource, EntityAssetSummary,
    EntityAssetSymbol, EntityControllerAnimation, EntityControllerAnimationTarget,
    EntityControllerState, EntityControllerTransition, EntityDependency, EntityDependencyKind,
    EntityDependencyResolution, EntityGeometry, EntityGeometryBone, EntityGeometryCube,
    EntityGeometryFaceUv, EntityGeometryFaceUvs, EntityGeometryInheritance, EntityGeometryScalar,
    EntityGeometryUv, EntityRenderCandidate, EntityRenderData, EntityRenderGeometry,
    EntityRenderLayer, EntityRenderSlot, EntityRenderVisibility, EntityRigAnimationBinding,
    EntityRigBinding, EntityRigControllerBinding, EntityRigFallback, EntityRigGeometryBinding,
    MAX_ENTITY_ANIMATION_CHANNELS, MAX_ENTITY_ANIMATION_CLIPS, MAX_ENTITY_ANIMATION_KEYFRAMES,
    MAX_ENTITY_ASSET_PATH_BYTES, MAX_ENTITY_ASSET_SOURCES, MAX_ENTITY_ASSET_SYMBOLS,
    MAX_ENTITY_CATALOG_BYTES, MAX_ENTITY_CONTROLLER_ANIMATIONS, MAX_ENTITY_CONTROLLER_NESTING,
    MAX_ENTITY_CONTROLLER_STATES, MAX_ENTITY_CONTROLLER_TRANSITIONS, MAX_ENTITY_CONTROLLERS,
    MAX_ENTITY_DEPENDENCIES, MAX_ENTITY_GEOMETRIES, MAX_ENTITY_GEOMETRY_BONES,
    MAX_ENTITY_GEOMETRY_CUBES, MAX_ENTITY_GEOMETRY_NAME_BYTES, MAX_ENTITY_GEOMETRY_SCALAR,
    MAX_ENTITY_IDENTIFIER_BYTES, MAX_ENTITY_RENDER_CANDIDATES, MAX_ENTITY_RENDER_LAYERS,
    MAX_ENTITY_RENDER_PATTERN_BYTES, MAX_ENTITY_RENDER_SLOTS, MAX_ENTITY_RENDER_VISIBILITY,
    MAX_ENTITY_RIG_ANIMATIONS, MAX_ENTITY_RIG_BINDINGS, MAX_ENTITY_RIG_CONTROLLERS,
    MAX_ENTITY_RIG_GEOMETRIES, MAX_ENTITY_SOURCE_BYTES, MAX_ENTITY_TEXTURE_DIMENSION,
    MAX_ENTITY_TOTAL_SOURCE_BYTES, MAX_MOLANG_COLLECTION_ITEMS, MAX_MOLANG_COLLECTION_ITEMS_TOTAL,
    MAX_MOLANG_COLLECTIONS, MAX_MOLANG_EXPRESSIONS, MAX_MOLANG_LOOP_DEPTH,
    MAX_MOLANG_LOOP_ITERATIONS, MAX_MOLANG_OPS, MAX_MOLANG_OPS_PER_EXPRESSION,
    MAX_MOLANG_QUERY_ARGUMENTS, MAX_MOLANG_STACK_DEPTH, MAX_MOLANG_STRING_BYTES, MOLANG_QUERIES,
    MolangBranch, MolangCall, MolangCollection, MolangCollectionItem, MolangEaseCurve,
    MolangEaseMode, MolangFunction, MolangOp, MolangSymbol, MolangSymbolKind, RuntimeEntityAssets,
    encode_entity_blob, molang_call, molang_program_stack, validate_entity_geometry_inheritance,
};
pub use entity::{PACK_EQUIPMENT_INDEX_BASE, PACK_RIG_ID_BASE};
pub use environment_settings::{CloudQuality, EnvironmentQualitySettings, PrecipitationQuality};
pub use equipment::{
    ArmorSlot, AttachablePose, AttachablePoseBone, EQUIPMENT_CARRIER_MAGIC,
    EQUIPMENT_CARRIER_VERSION, EquipmentBinding, EquipmentCategory, EquipmentReference,
    EquipmentTexture, EquipmentTransform, ItemUseDuration, MAX_EQUIPMENT_BINDINGS,
    MAX_EQUIPMENT_CARRIER_BYTES, MAX_EQUIPMENT_IDENTIFIER_BYTES, MAX_EQUIPMENT_TEXTURE_SIDE,
    MAX_EQUIPMENT_TEXTURES, RuntimeEquipmentCatalog, encode_equipment_catalog,
    encode_equipment_catalog_full, encode_equipment_catalog_with_textures,
};
pub use error::AssetError;
pub use font::{
    CompiledFontCatalog, FONT_CARRIER_MAGIC, FONT_CARRIER_SCHEMA, FontCatalogError,
    FontCatalogIdentity, FontTexturePage, GlyphMetrics, MAX_FONT_GLYPHS, MAX_FONT_PAGE_SIDE,
    MAX_FONT_PAGES, MAX_FONT_PATH_BYTES, MAX_FONT_SOURCE_BYTES, RuntimeFontCatalog,
    encode_font_catalog,
};
pub use fsb::{DecodedSound, FsbError, MAX_FSB_INPUT_BYTES, MAX_FSB_PCM_BYTES, decode_fsb5};
pub use glyph_sheet::{
    CellGlyph, GlyphAtlas, GlyphSheet, SHEET_GRID, SheetGlyph, extract_cells, pack_cells,
    texel_size_64,
};
pub use hud::{
    HUD_CARRIER_MAGIC, HUD_CARRIER_VERSION, HUD_SOURCE_MANIFEST_SHA256, HudCatalogError,
    HudTexture, HudTextureRole, MAX_HUD_TEXTURE_BYTES, RuntimeHudCatalog, encode_hud_catalog,
};
pub use icon::{
    ICON_CARRIER_MAGIC, ICON_CARRIER_VERSION, IconEntry, IconSprite, MAX_ICON_CARRIER_BYTES,
    MAX_ICON_ENTRIES, MAX_ICON_KEY_BYTES, MAX_ICON_SIDE, MAX_ICON_SPRITES, RuntimeIconCatalog,
    encode_icon_catalog,
};
pub use item::{
    BlockVisualId, ItemActionPhase, ItemDisplayScalar, ItemDisplayTransform, ItemIconRef,
    ItemStackIdentity, ItemStackIdentityError, ItemTextureReference, ItemVisualAlias,
    ItemVisualDefinition, ItemVisualDefinitionRoute, ItemVisualId, ItemVisualKey, ItemVisualRoute,
    MAX_BLOCK_VISUALS, MAX_ITEM_IDENTIFIER_BYTES, MAX_ITEM_VISUAL_ALIASES, MAX_ITEM_VISUALS,
};
pub use lang::{
    LANG_CARRIER_MAGIC, LANG_CARRIER_VERSION, LangCatalogError, LangEntry, MAX_LANG_CARRIER_BYTES,
    MAX_LANG_ENTRIES, MAX_LANG_KEY_BYTES, MAX_LANG_VALUE_BYTES, RuntimeLangCatalog,
    VANILLA_EN_US_LANG_SHA256, encode_lang_catalog, is_language_code,
};
pub use light_registry::{LightProperties, read_light_registry, read_light_registry_for_protocol};
pub use material_keys::{MAX_MATERIAL_KEYS_BYTES, MaterialKeys};
pub use model::{
    ANIMATION_FLAG_BLEND, Animation, MAX_ANIMATION_FRAMES, MAX_ANIMATIONS, MAX_MODEL_QUADS,
    MAX_MODEL_TEMPLATES, MAX_TEXTURE_PAGES, MODEL_QUAD_FLAG_CULL_FACE_MASK,
    MODEL_QUAD_FLAG_FACE_MASK, MODEL_QUAD_FLAG_TWO_SIDED, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
    MODEL_TEMPLATE_FLAG_FENCE_NETHER, MODEL_TEMPLATE_FLAG_FENCE_WOOD,
    MODEL_TEMPLATE_FLAG_GATE_AXIS_X, MODEL_TEMPLATE_FLAG_GATE_AXIS_Z, MODEL_TEMPLATE_FLAG_KELP,
    MODEL_TEMPLATE_FLAG_PANE, MODEL_TEMPLATE_FLAG_STAIR, MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE,
    MODEL_TEMPLATE_FLAG_WALL, ModelQuad, ModelTemplate, NO_ANIMATION, NO_MODEL_TEMPLATE,
    TexturePage, TextureRef, VisualKind, VisualSupport,
};
pub use ogg::{decode_ogg, decode_sound};
pub use particle::{
    MAX_PARTICLE_CARRIER_BYTES, MAX_PARTICLE_EFFECT_BYTES, MAX_PARTICLE_EFFECTS,
    MAX_PARTICLE_KEY_BYTES, MAX_PARTICLE_TEXTURE_SIDE, MAX_PARTICLE_TEXTURES,
    PARTICLE_CARRIER_MAGIC, PARTICLE_CARRIER_VERSION, ParticleEffectFile, ParticleTexture,
    RuntimeParticleAssets, encode_particle_catalog, strip_json_comments,
};
pub use physics_registry::{
    BlockPhysicsFlags, BlockPhysicsRecord, PhysicsRegistry, SurfaceResponse,
    physics_registry_header_protocol, read_physics_registry, read_physics_registry_for_protocol,
};
pub use provenance::{
    BlobProvenance, VANILLA_SOURCE_MANIFEST, VanillaSource, canonical_source_manifest_sha256,
    vanilla_source, vanilla_source_manifest_sha256,
};
pub use registry::{
    BlockFlags, CollisionBox, CollisionConfidence, CollisionSeed, ContributorRole, ModelFamily,
    ModelState, ModelStateField, RegistryProvenance, RegistryRecord, read_registry,
    read_registry_for_protocol, registry_header_protocol,
};
pub use runtime::{
    BlockOverlay, MaterialOverride, NetworkIdMode, ResolvedBlock, ResolvedFace, RuntimeAssets,
    SequentialIdRemap,
};
pub use server_lang::{MAX_SERVER_LANG_INPUT_BYTES, ServerLangOverlay};
pub use sound_bank::{
    MAX_SOUND_BANK_FILES, MAX_SOUND_BANK_PATH_BYTES, MAX_SOUND_BANK_PREFIX_BYTES, SOUND_BANK_MAGIC,
    SoundBankEntry, SoundBankError, SoundBankIndex, encode_sound_bank, sound_bank_prefix_len,
};
pub use sound_events::{FloatRange, RouteLookup, SoundEventTables, SoundRoute};
pub use texture::{
    MAX_TILE_SIZE, MIP_COUNT, TILE_SIZE, TextureArray, TextureMip, build_texture_mip_chain,
    downsample_linear_premultiplied,
};
pub use ui::{
    MAX_UI_ATLAS_PAGES, MAX_UI_ATLAS_SIDE, MAX_UI_CARRIER_BYTES, MAX_UI_FILE_BYTES, MAX_UI_FILES,
    MAX_UI_KEY_BYTES, MAX_UI_SIDECARS, MAX_UI_TEXTURES, RuntimeUiAssets, UI_CARRIER_MAGIC,
    UI_CARRIER_VERSION, UiAtlasPage, UiFile, UiNineSlice, UiSidecar, UiSidecarEntry,
    UiTexturePlacement, UiTextureUv, encode_ui_catalog,
};
pub use vanilla_refs::{MAX_VANILLA_REFS_BYTES, VanillaEntityRefs, VanillaGeometryFile};
pub use weather_textures::{
    END_SKY_SIDE, MAX_WEATHER_TEXTURES_BYTES, WEATHER_SHEET_SIDE, WEATHER_TEXTURES_MAGIC,
    WEATHER_TEXTURES_VERSION, WeatherImage, WeatherTextures, WeatherTexturesError,
    decode_weather_textures, encode_weather_textures,
};

mod biome_noise;
pub use biome_noise::{ClientRandom, grass_noise_permutation};
