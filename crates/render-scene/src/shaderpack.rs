//! Main-world state of the experimental Java shader-pack render mode: which pack the player
//! selected, how loading went, the per-camera opt-in, and the game state packs read through
//! their standard uniforms. The renderer behind it compiles only with the `shaderpacks`
//! feature; without it these types stay unused.

use std::{collections::HashMap, fmt, path::PathBuf, sync::Arc};

use bevy::{
    prelude::{Component, Resource},
    render::{extract_component::ExtractComponent, extract_resource::ExtractResource},
};

/// The pack the settings ask for. The render host loads it on a worker whenever `pack`
/// changes or is explicitly reselected, and reports progress through [`ShaderPackStatus`].
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub struct ShaderPackRequest {
    /// Pack folder or zip; `None` unloads the current pack.
    pub pack: Option<PathBuf>,
    /// Private directory for cached translations; `None` disables the cache.
    pub cache_dir: Option<PathBuf>,
    /// Explicit menu selections reload even when the pack path is unchanged.
    pub reload_generation: u64,
}

/// Load state of the requested pack, written by the render host.
#[derive(Resource, Clone, Debug, Default, PartialEq, Eq)]
pub enum ShaderPackStatus {
    /// No pack requested.
    #[default]
    Idle,
    /// Translation is running for this pack.
    Loading(PathBuf),
    /// The pack is translated and its programs are ready to bind.
    Ready(PathBuf),
    /// The pack could not be used; cameras stay on Vanilla.
    Failed { pack: PathBuf, reason: String },
}

impl ShaderPackStatus {
    /// Whether cameras may opt into the loaded pack.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }
}

/// Per-camera opt-in: the camera's world passes run through the loaded pack.
#[derive(Component, ExtractComponent, Clone, Copy, Debug, Default, PartialEq, Eq)]
#[extract_app(bevy::render::RenderApp)]
#[require(bevy::render::view::Msaa::Off)]
pub struct ShaderPackRendering;

/// The most biomes [`ShaderPackFrameInputs::sky_biomes`] carries; the Gaussian blend's
/// lighter biomes beyond these are dropped.
pub const SKY_BIOME_SLOTS: usize = 4;

/// Game state behind the standard Iris uniforms that the render world cannot derive from
/// the view. Published each frame by the app; camera matrices, positions, screen size and
/// frame timing come from the extracted view and render-world clock instead.
#[derive(Resource, ExtractResource, Clone, Copy, Debug, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
pub struct ShaderPackFrameInputs {
    /// Time of day in ticks, `0..24000` (`worldTime`).
    pub world_time: i32,
    /// Whole days elapsed (`worldDay`).
    pub world_day: i32,
    /// Moon phase `0..8` (`moonPhase`).
    pub moon_phase: i32,
    /// Celestial angle as a fraction of a full turn, `0..1`, where 0 is noon (Java's
    /// `getTimeOfDay`); drives `sunAngle`, `shadowAngle` and the celestial positions.
    pub celestial_angle: f32,
    /// Rain level `0..1` (`rainStrength`).
    pub rain_strength: f32,
    /// Thunder level `0..1` (`thunderStrength`).
    pub thunder_strength: f32,
    /// Block and sky light at the eye, each `0..240` in steps of 16 (`eyeBrightness`).
    pub eye_brightness: [i32; 2],
    /// 0 air, 1 water, 2 lava, 3 powder snow (`isEyeInWater`).
    pub eye_medium: i32,
    /// Bedrock biomes around the camera with Java's Gaussian sky and fog blend weights, the
    /// heaviest first; the weights sum to 1 over the used slots and unused slots weigh 0.
    /// The render host derives `skyColor` and `fogColor` from their Java biomes.
    pub sky_biomes: [(u32, f32); SKY_BIOME_SLOTS],
    /// Frozen `frameTimeCounter` in seconds for captures (developer `--frame-time-counter`);
    /// `None` counts real time.
    pub frame_time_counter: Option<f32>,
    /// Blocks along the camera ray to terrain geometry; models use their surfaces, while
    /// cubes and liquids use cell boundaries. `None` when nothing is in range.
    /// `centerDepthSmooth` smooths the corresponding depth-buffer value.
    pub center_distance: Option<f32>,
    /// Fog start and end distances in blocks (`fogStart`, `fogEnd`).
    pub fog_range: [f32; 2],
    /// Render distance in blocks (`far`).
    pub render_distance: f32,
    /// Night vision strength `0..1` (`nightVision`).
    pub night_vision: f32,
    /// Blindness strength `0..1` (`blindness`).
    pub blindness: f32,
    /// Darkness effect strength `0..1` (`darknessFactor`).
    pub darkness: f32,
    /// `block.properties`/`item.properties` id of the main-hand item, -1 when unmapped
    /// (`heldItemId`).
    pub held_item_id: i32,
    /// Light emitted by the main-hand item's block, `0..15` (`heldBlockLightValue`).
    pub held_block_light: i32,
    /// Off-hand counterparts (`heldItemId2`, `heldBlockLightValue2`).
    pub held_item_id2: i32,
    pub held_block_light2: i32,
    /// Player health, hunger and air as fractions `0..1`, -1 when unknown
    /// (`currentPlayerHealth`, `currentPlayerHunger`, `currentPlayerAir`).
    pub player_health: f32,
    pub player_hunger: f32,
    pub player_air: f32,
    /// Bedrock biome id at the camera entity's block, `None` before the world streams; the
    /// biome uniforms (`biome`, `biome_category`, `temperature`, `rainfall`,
    /// `biome_precipitation`) read it through its Java biome.
    pub biome: Option<u32>,
    /// Block Y the biome was sampled at, for the snow line of `biome_precipitation`.
    pub biome_block_y: i32,
    /// Sky dimension: 0 Overworld, 1 Nether, 2 End; selects the pack's `world*` folder.
    pub dimension: i32,
    /// Lowest block Y of the current dimension (`bedrockLevel`, Iris's dimension `minY`).
    pub bedrock_level: i32,
    /// Positive range over which void fog fades; normal worlds use 32, flat worlds use 1.
    pub void_darkness_range: f32,
    /// Local player is sneaking, sprinting, hurt, burning (`is_sneaking` …).
    pub sneaking: bool,
    pub sprinting: bool,
    pub hurt: bool,
    pub on_fire: bool,
    /// Camera is on the ground (`is_on_ground`).
    pub on_ground: bool,
}

impl Default for ShaderPackFrameInputs {
    fn default() -> Self {
        Self {
            world_time: 6000,
            world_day: 0,
            moon_phase: 0,
            celestial_angle: 0.0,
            rain_strength: 0.0,
            thunder_strength: 0.0,
            eye_brightness: [0, 240],
            eye_medium: 0,
            sky_biomes: [(0, 0.0); SKY_BIOME_SLOTS],
            frame_time_counter: None,
            center_distance: None,
            fog_range: [0.0, 256.0],
            render_distance: 256.0,
            night_vision: 0.0,
            blindness: 0.0,
            darkness: 0.0,
            held_item_id: -1,
            held_block_light: 0,
            held_item_id2: -1,
            held_block_light2: 0,
            player_health: -1.0,
            player_hunger: -1.0,
            player_air: -1.0,
            biome: None,
            biome_block_y: 64,
            dimension: 0,
            bedrock_level: 0,
            void_darkness_range: 32.0,
            sneaking: false,
            sprinting: false,
            hurt: false,
            on_fire: false,
            on_ground: true,
        }
    }
}

/// Resolves a Bedrock identifier (`minecraft:ender_crystal`) to the loaded pack's id.
pub type ShaderPackIdLookup = Arc<dyn Fn(&str) -> Option<i32> + Send + Sync>;

/// The loaded pack's `entity.properties` and `item.properties` ids by Bedrock identifier,
/// through the pinned Java mapping. Published by the render host when a pack loads (empty
/// without one); the app reads it to fill [`ShaderPackActorIds`] and the held-item ids.
#[derive(Resource, Clone, Default)]
pub struct ShaderPackIdMaps {
    entity: Option<ShaderPackIdLookup>,
    item: Option<ShaderPackIdLookup>,
    /// Changes whenever the lookups do.
    pub revision: u64,
}

impl ShaderPackIdMaps {
    #[must_use]
    pub fn new(entity: ShaderPackIdLookup, item: ShaderPackIdLookup, revision: u64) -> Self {
        Self {
            entity: Some(entity),
            item: Some(item),
            revision,
        }
    }

    /// The pack's `entityId` of a Bedrock entity identifier.
    #[must_use]
    pub fn entity_id(&self, bedrock_identifier: &str) -> Option<i32> {
        self.entity.as_ref()?(bedrock_identifier)
    }

    /// The pack's item id (`heldItemId`, `currentRenderedItemId`) of a Bedrock item identifier.
    #[must_use]
    pub fn item_id(&self, bedrock_identifier: &str) -> Option<i32> {
        self.item.as_ref()?(bedrock_identifier)
    }

    /// Whether a pack's maps are loaded.
    #[must_use]
    pub fn is_loaded(&self) -> bool {
        self.entity.is_some()
    }
}

impl fmt::Debug for ShaderPackIdMaps {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ShaderPackIdMaps")
            .field("loaded", &self.is_loaded())
            .field("revision", &self.revision)
            .finish()
    }
}

/// The pack's `entityId` of every actor the rig renders, by runtime id; actors absent read
/// Iris's -1. Published each frame by the app while a pack is loaded.
#[derive(Resource, ExtractResource, Clone, Debug, Default, PartialEq, Eq)]
#[extract_app(bevy::render::RenderApp)]
pub struct ShaderPackActorIds {
    pub by_runtime_id: HashMap<u64, i32>,
}

impl ShaderPackActorIds {
    /// The `entityId` of the actor with `runtime_id`.
    #[must_use]
    pub fn entity_id(&self, runtime_id: u64) -> i32 {
        self.by_runtime_id.get(&runtime_id).copied().unwrap_or(-1)
    }
}
