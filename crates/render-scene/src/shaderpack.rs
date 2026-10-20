//! Main-world state of the experimental Java shader-pack render mode: the game state packs
//! read through their standard uniforms. The renderer behind it compiles only with the
//! `shaderpacks` feature; without it these types stay unused.

use bevy::{prelude::Resource, render::extract_resource::ExtractResource};

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
    /// Blocks from the camera along its view direction to the first non-air block, `None`
    /// when nothing is in range; `centerDepthSmooth` smooths its depth-buffer value.
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
