use std::sync::Arc;

use assets::{ResolvedFog, RuntimeAtmosphereAssets};
use bevy::{
    prelude::{Resource, Vec4},
    render::{extract_resource::ExtractResource, render_resource::ShaderType},
};
use meshing::CameraMedium;
use meshing::cloud_viewport::CLOUD_FADE_START;

use crate::{AtmosphereViewInputs, celestial, native_sunlight};

#[path = "atmosphere/liquid_distance.rs"]
mod liquid_distance;

pub const BEDROCK_DAY_TICKS: f64 = celestial::DAY_TICKS;
pub const CLOUD_TEXTURE_WORLD_PERIOD: f64 = meshing::CLOUD_WORLD_PERIOD as f64;
/// Vanilla samples the cloud texture 0.03 blocks ahead per 1.5 ticks.
pub const CLOUD_SCROLL_BLOCKS_PER_TICK: f64 = 0.03 / 1.5;
pub const CLOUD_ALPHA: f32 = 0.7;
const CLOUD_SUNRISE_WEIGHT: f32 = 0.35;
const RAIN_CLOUD_CHANNEL: f32 = 0.6;
const WEATHER_COLOUR_CONTRIBUTION: f32 = 0.95;

// Fallbacks mirroring the vanilla default fog profile, used only without a compiled carrier.
const WATER_FOG_RGB8: u32 = 0x0044_AFF5;
const WATER_FOG_END: f32 = 60.0;
const LAVA_FOG_RGB8: u32 = 0x0099_1A00;
const LAVA_FOG_END: f32 = 0.64;
const AIR_FOG_RGB8: u32 = 0x00AB_D2FF;
const WEATHER_FOG_RGB8: u32 = 0x0066_6666;
const FALLBACK_RENDER_DISTANCE: f32 = 256.0;
const AIR_FOG_FRACTIONS: [f32; 2] = [0.92, 1.0];
const WEATHER_FOG_FRACTIONS: [f32; 2] = [0.23, 0.7];
/// Temperature of the plains biome, used until the camera biome is known.
const DEFAULT_SKY_TEMPERATURE: f32 = 0.8;
/// Provisional lightning-flash sky pull; needs native calibration.
const PROVISIONAL_FLASH_SKY_PULL: f32 = 0.6;
const PROVISIONAL_FLASH_COLOUR: [f32; 3] = [0.85, 0.87, 1.0];
/// Provisional vision-effect responses; need native calibration.
const BLINDNESS_FOG_END: f32 = 5.0;
const DARKNESS_FOG_END_SCALE: f32 = 0.4;
const NETHER_FOG_RGB8: u32 = 0x0033_0808;

/// Which sky model the shader draws.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkyKind {
    Overworld = 0,
    Nether = 1,
    End = 2,
}

impl SkyKind {
    pub(crate) const fn has_clouds(self) -> bool {
        matches!(self, Self::Overworld)
    }

    #[must_use]
    pub const fn from_dimension(dimension: i32) -> Self {
        match dimension {
            1 => Self::Nether,
            2 => Self::End,
            _ => Self::Overworld,
        }
    }
}

/// Explicitly provisional boss-environment response constants. No
/// version-matched native Bedrock capture of the boss-driven sky darkening
/// or world fog exists yet; they must be calibrated against native evidence
/// before any visual acceptance gate closes.
pub const PROVISIONAL_BOSS_DARKEN_SKY_STRENGTH: f32 = 0.5;
pub const PROVISIONAL_BOSS_WORLD_FOG_START_BLOCKS: f32 = 96.0;
pub const PROVISIONAL_BOSS_WORLD_FOG_END_BLOCKS: f32 = 160.0;
const BOSS_DARKEN_ZENITH_TARGET: [f32; 3] = [0.12, 0.14, 0.16];
const BOSS_DARKEN_HORIZON_TARGET: [f32; 3] = [0.22, 0.24, 0.26];

#[derive(Resource, ExtractResource, Clone, Default)]
pub struct AtmosphereTextureAssets {
    runtime: Option<Arc<RuntimeAtmosphereAssets>>,
    identity: [u8; 32],
}

impl AtmosphereTextureAssets {
    #[must_use]
    pub fn new(runtime: Arc<RuntimeAtmosphereAssets>, identity: [u8; 32]) -> Self {
        Self {
            runtime: Some(runtime),
            identity,
        }
    }

    #[must_use]
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }

    #[must_use]
    pub fn runtime(&self) -> Option<&Arc<RuntimeAtmosphereAssets>> {
        self.runtime.as_ref()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MoonPhaseTile {
    pub pixel_origin: [u32; 2],
    pub uv_origin: [f32; 2],
    pub uv_extent: [f32; 2],
}

#[must_use]
pub fn moon_phase_tile(phase: u8) -> MoonPhaseTile {
    let phase = u32::from(phase % 8);
    let column = phase % 4;
    let row = phase / 4;
    MoonPhaseTile {
        pixel_origin: [column * 32, row * 32],
        uv_origin: [column as f32 * 0.25, row as f32 * 0.5],
        uv_extent: [0.25, 0.5],
    }
}

/// Cloud texture offset as a fraction of the period; clouds drift toward -X.
#[must_use]
pub fn cloud_texture_offset(absolute_ticks: f64) -> [f32; 2] {
    let ticks = if absolute_ticks.is_finite() {
        absolute_ticks
    } else {
        0.0
    };
    [
        (-(ticks * CLOUD_SCROLL_BLOCKS_PER_TICK) / CLOUD_TEXTURE_WORLD_PERIOD).rem_euclid(1.0)
            as f32,
        0.0,
    ]
}

/// Matching legacy weather tint applied to otherwise-white clouds.
///
/// The native rain and thunder channels each contribute at most 0.95, in
/// that order. Invalid server-authored levels are treated as clear weather.
#[must_use]
pub fn cloud_weather_colour(rain_level: f32, thunder_level: f32) -> [f32; 3] {
    cloud_base_colour(0.0, rain_level, thunder_level)
}

/// Shade the vanilla cloud tessellator bakes per face: top 1, bottom 0.75, x sides 0.925.
#[must_use]
pub fn cloud_face_shade(normal: [f32; 3]) -> f32 {
    meshing::cloud_face_shade(normal)
}

/// Vanilla cloud RGBA: weather tint, day brightness,
/// the sunrise blend and the fixed alpha.
#[must_use]
pub fn cloud_colour(celestial_angle: f32, rain: f32, thunder: f32, sunrise: [f32; 4]) -> [f32; 4] {
    let base = cloud_base_colour(celestial_angle, rain, thunder);
    let weight = bounded_level(sunrise[3]) * CLOUD_SUNRISE_WEIGHT;
    let [r, g, b] = std::array::from_fn(|channel| {
        let band = if sunrise[channel].is_finite() {
            sunrise[channel]
        } else {
            0.0
        };
        (band * weight + base[channel] * (1.0 - weight)).max(0.0)
    });
    [r, g, b, CLOUD_ALPHA]
}

fn cloud_base_colour(angle: f32, rain: f32, thunder: f32) -> [f32; 3] {
    // Vanilla classic, non-custom cloud colour:
    // rain first, then day RGB multipliers, then thunder's luminance pull.
    let angle = if angle.is_finite() { angle } else { 0.0 };
    let rain = bounded_level(rain) * WEATHER_COLOUR_CONTRIBUTION;
    let weather = celestial::lerp(1.0, RAIN_CLOUD_CHANNEL, rain);
    let brightness = celestial::day_plateau(angle);
    let base = [
        weather * (0.9 * brightness + 0.1),
        weather * (0.9 * brightness + 0.1),
        weather * (0.85 * brightness + 0.15),
    ];
    let grey = (base[0] * 0.3 + base[1] * 0.59 + base[2] * 0.11) * 0.2;
    let weight = bounded_level(thunder) * WEATHER_COLOUR_CONTRIBUTION;
    base.map(|channel| celestial::lerp(channel, grey, weight))
}

/// Alpha scale at `distance`: 1 to 0.9 `fade_distance`, 0 by 1.9; a non-positive distance never fades.
#[must_use]
pub fn cloud_distance_fade(distance: f32, fade_distance: f32) -> f32 {
    if !(fade_distance > 0.0 && fade_distance.is_finite()) {
        return 1.0;
    }
    if !distance.is_finite() {
        return 0.0;
    }
    (1.0 - (distance.max(0.0) / fade_distance - CLOUD_FADE_START).max(0.0)).clamp(0.0, 1.0)
}

/// One deterministic, renderer-ready snapshot of the active Bedrock sky.
///
/// The nine `vec4`-shaped records are also the complete GPU uniform. Keeping the
/// CPU and GPU contracts identical avoids per-frame allocation or conversion.
#[repr(C)]
#[derive(
    Resource,
    ExtractResource,
    Clone,
    Copy,
    Debug,
    PartialEq,
    bytemuck::Pod,
    bytemuck::Zeroable,
    ShaderType,
)]
pub struct AtmosphereFrame {
    sun_direction_daylight: Vec4,
    moon_direction_phase: Vec4,
    sky_zenith_rain: Vec4,
    sky_horizon_thunder: Vec4,
    fog_color_start: Vec4,
    fog_end_time: Vec4,
    /// Sunrise/sunset glow: linear rgb and alpha (zero outside the band).
    sunrise_band: Vec4,
    /// Star alpha, celestial angle in turns, lightning flash, then `sky kind + 4 * medium`.
    sky_extra: Vec4,
    /// Ordinary liquid alpha's distance control in x; remaining channels are reserved.
    liquid_distance: Vec4,
}

const _: () = assert!(std::mem::size_of::<AtmosphereFrame>() == 144);

impl Default for AtmosphereFrame {
    fn default() -> Self {
        Self::from_bedrock_time(0.0, 0.0, 0.0)
    }
}

impl AtmosphereFrame {
    #[must_use]
    pub fn from_bedrock_time(absolute_ticks: f64, rain_level: f32, thunder_level: f32) -> Self {
        let absolute_ticks = if absolute_ticks.is_finite() {
            absolute_ticks
        } else {
            0.0
        };
        let rain = bounded_level(rain_level);
        let thunder = bounded_level(thunder_level);
        let day_fraction =
            (absolute_ticks.rem_euclid(BEDROCK_DAY_TICKS) / BEDROCK_DAY_TICKS) as f32;
        let angle = celestial::celestial_angle(absolute_ticks);
        let sun_direction = celestial::sun_direction(angle);
        let moon_direction = sun_direction.map(|component| -component);
        let moon_phase = ((absolute_ticks / BEDROCK_DAY_TICKS).floor().rem_euclid(8.0)) as u8;

        let zenith = overworld_zenith(angle, DEFAULT_SKY_TEMPERATURE, thunder);
        let fog_color = overworld_fog_colour(
            angle,
            rgb8_to_gamma(AIR_FOG_RGB8),
            rgb8_to_gamma(WEATHER_FOG_RGB8),
            0.0,
        );
        let fog_start = celestial::lerp(
            AIR_FOG_FRACTIONS[0] * FALLBACK_RENDER_DISTANCE,
            WEATHER_FOG_FRACTIONS[0] * FALLBACK_RENDER_DISTANCE,
            0.0,
        );
        let fog_end = celestial::lerp(
            AIR_FOG_FRACTIONS[1] * FALLBACK_RENDER_DISTANCE,
            WEATHER_FOG_FRACTIONS[1] * FALLBACK_RENDER_DISTANCE,
            0.0,
        );
        let band = celestial::sunrise_band(angle, rain);
        let band_rgb = celestial::rgb_to_linear([band[0], band[1], band[2]]);

        Self {
            sun_direction_daylight: Vec4::new(
                sun_direction[0],
                sun_direction[1],
                sun_direction[2],
                celestial::daylight(angle, rain, thunder),
            ),
            moon_direction_phase: Vec4::new(
                moon_direction[0],
                moon_direction[1],
                moon_direction[2],
                f32::from(moon_phase),
            ),
            sky_zenith_rain: Vec4::new(zenith[0], zenith[1], zenith[2], rain),
            sky_horizon_thunder: Vec4::new(fog_color[0], fog_color[1], fog_color[2], thunder),
            fog_color_start: Vec4::new(fog_color[0], fog_color[1], fog_color[2], fog_start),
            // Cloud motion is a local renderer clock, not named daylight time.
            fog_end_time: Vec4::new(fog_end, day_fraction, 0.0, 0.0),
            sunrise_band: Vec4::new(band_rgb[0], band_rgb[1], band_rgb[2], band[3]),
            sky_extra: Vec4::new(celestial::star_brightness(angle, rain), angle, 0.0, 0.0),
            liquid_distance: Vec4::new(
                liquid_distance::alpha_distance_blocks(FALLBACK_RENDER_DISTANCE)
                    .unwrap_or_default(),
                0.0,
                0.0,
                0.0,
            ),
        }
    }

    /// Re-derives the clear-sky colour from the camera biome temperature.
    #[must_use]
    pub fn with_biome_temperature(mut self, temperature: f32) -> Self {
        if self.sky_kind() == SkyKind::Overworld {
            let zenith =
                overworld_zenith(self.celestial_angle(), temperature, self.thunder_level());
            self.sky_zenith_rain =
                Vec4::new(zenith[0], zenith[1], zenith[2], self.sky_zenith_rain.w);
        }
        self
    }

    /// Selects the dimension sky model. Nether and End have no day cycle, sun, moon or stars.
    #[must_use]
    pub fn with_sky_kind(mut self, kind: SkyKind) -> Self {
        let medium_code = self.sky_extra.w - self.sky_extra.w % 4.0;
        self.sky_extra.w = medium_code + kind as u32 as f32;
        if kind == SkyKind::Overworld {
            return self;
        }
        self.sun_direction_daylight.w = 1.0;
        self.sunrise_band.w = 0.0;
        self.sky_extra.x = 0.0;
        let colour = match kind {
            SkyKind::Nether => rgb8_to_linear(NETHER_FOG_RGB8),
            SkyKind::End | SkyKind::Overworld => [0.0; 3],
        };
        self.sky_zenith_rain = Vec4::new(colour[0], colour[1], colour[2], self.sky_zenith_rain.w);
        self.sky_horizon_thunder =
            Vec4::new(colour[0], colour[1], colour[2], self.sky_horizon_thunder.w);
        self
    }

    /// Adds a lightning flash in `0..=1`: brightens the lightmap and pulls sky and fog toward white.
    #[must_use]
    pub fn with_lightning_flash(mut self, flash: f32) -> Self {
        let flash = bounded_level(flash);
        if flash <= 0.0 || self.sky_kind() != SkyKind::Overworld {
            return self;
        }
        self.sky_extra.z = flash;
        self.sun_direction_daylight.w = celestial::lerp(self.daylight(), 1.0, flash);
        let pull = flash * PROVISIONAL_FLASH_SKY_PULL;
        for record in [
            &mut self.sky_zenith_rain,
            &mut self.sky_horizon_thunder,
            &mut self.fog_color_start,
        ] {
            *record = Vec4::new(
                celestial::lerp(record.x, PROVISIONAL_FLASH_COLOUR[0], pull),
                celestial::lerp(record.y, PROVISIONAL_FLASH_COLOUR[1], pull),
                celestial::lerp(record.z, PROVISIONAL_FLASH_COLOUR[2], pull),
                record.w,
            );
        }
        self
    }

    /// Applies blindness (fog closes to black), darkness (dimmer lightmap and fog) and night
    /// vision (lightmap toward full bright), each in `0..=1`.
    #[must_use]
    pub fn with_vision_effects(
        mut self,
        blindness: f32,
        darkness: f32,
        _night_vision: f32,
    ) -> Self {
        let (blindness, darkness) = (bounded_level(blindness), bounded_level(darkness));
        let dim = (1.0 - blindness) * (1.0 - darkness * 0.5);
        for record in [
            &mut self.sky_zenith_rain,
            &mut self.sky_horizon_thunder,
            &mut self.fog_color_start,
        ] {
            *record = Vec4::new(record.x * dim, record.y * dim, record.z * dim, record.w);
        }
        let end_scale = celestial::lerp(1.0, DARKNESS_FOG_END_SCALE, darkness);
        self.fog_end_time.x = celestial::lerp(
            self.fog_end_time.x * end_scale,
            BLINDNESS_FOG_END,
            blindness,
        );
        self.fog_color_start.w =
            celestial::lerp(self.fog_color_start.w * end_scale, 0.0, blindness)
                .min(self.fog_end_time.x);
        self
    }

    /// Applies camera-medium distance fog while retaining the exact celestial
    /// and weather snapshot. The fallbacks mirror the default vanilla fog profile.
    #[must_use]
    pub fn with_camera_medium(mut self, medium: CameraMedium) -> Self {
        let (rgb8, end, code) = match medium {
            CameraMedium::Air => return self,
            CameraMedium::Water => (WATER_FOG_RGB8, WATER_FOG_END, 1.0),
            CameraMedium::Lava => (LAVA_FOG_RGB8, LAVA_FOG_END, 2.0),
        };
        let color = rgb8_to_linear(rgb8);
        self.fog_color_start = Vec4::new(color[0], color[1], color[2], 0.0);
        self.fog_end_time.x = end;
        self.sky_extra.w = self.sky_extra.w % 4.0 + code * 4.0;
        self
    }

    /// Applies only exact client-profile values that survived bounded asset
    /// compilation. Time, weather channels, celestial state, and cloud motion
    /// remain unchanged.
    #[must_use]
    pub fn with_environment_profile(
        mut self,
        sky_rgb8: Option<u32>,
        fog: Option<ResolvedFog>,
    ) -> Self {
        if let Some(rgb) = sky_rgb8.filter(|rgb| *rgb <= 0x00ff_ffff) {
            let lit = if self.sky_kind() == SkyKind::Overworld {
                celestial::day_plateau(self.celestial_angle())
            } else {
                1.0
            };
            let gamma = rgb8_to_gamma(rgb).map(|channel| channel * lit);
            let colour = native_sky_colour(gamma, self.thunder_level());
            self.sky_zenith_rain.x = colour[0];
            self.sky_zenith_rain.y = colour[1];
            self.sky_zenith_rain.z = colour[2];
            self.sky_horizon_thunder.x = colour[0];
            self.sky_horizon_thunder.y = colour[1];
            self.sky_horizon_thunder.z = colour[2];
        }
        if let Some(fog) = fog.filter(valid_fog) {
            let colour = celestial::rgb_to_linear(fog.rgb);
            self.fog_color_start = Vec4::new(colour[0], colour[1], colour[2], fog.start);
            self.fog_end_time.x = fog.end;
        }
        self
    }

    /// Applies profile fog using the native smoothed weather-fog accumulator, and
    /// tints the horizon to match. Overworld fog follows the day cycle; other dimensions are fixed.
    #[must_use]
    pub fn with_blended_fog(
        mut self,
        air: Option<ResolvedFog>,
        weather: Option<ResolvedFog>,
        fog_weather_level: f32,
    ) -> Self {
        let Some(air) = air.filter(valid_fog) else {
            return self;
        };
        let rain = bounded_level(fog_weather_level);
        let fog = match weather.filter(valid_fog) {
            Some(weather) if rain > 0.0 => ResolvedFog {
                start: celestial::lerp(air.start, weather.start, rain),
                end: celestial::lerp(air.end, weather.end, rain),
                rgb: mix3(air.rgb, weather.rgb, rain),
            },
            _ => air,
        };
        let mut gamma = fog.rgb;
        if self.sky_kind() == SkyKind::Overworld {
            let factors = native_sunlight::fog_multipliers(self.celestial_angle());
            gamma = std::array::from_fn(|channel| gamma[channel] * factors[channel]);
        }
        let colour = celestial::rgb_to_linear(gamma);
        self.fog_color_start = Vec4::new(colour[0], colour[1], colour[2], fog.start);
        self.fog_end_time.x = fog.end;
        self.sky_horizon_thunder =
            Vec4::new(colour[0], colour[1], colour[2], self.sky_horizon_thunder.w);
        if self.sky_kind() == SkyKind::Nether {
            self.sky_zenith_rain =
                Vec4::new(colour[0], colour[1], colour[2], self.sky_zenith_rain.w);
        }
        self
    }

    /// Final classic camera colour terms, after biome/profile resolution and before effects.
    #[must_use]
    pub fn with_camera_environment(mut self, view: AtmosphereViewInputs) -> Self {
        if self.sky_kind() != SkyKind::Overworld {
            return self;
        }
        let angle = self.celestial_angle();
        let before = celestial::rgb_to_gamma(self.sky_zenith());
        let adjusted = view.sky_colour(before, angle, self.thunder_level());
        let subtract = view.colour_subtraction(angle);
        if adjusted != before || subtract > 0.0 {
            let sky = celestial::rgb_to_linear(adjusted.map(|c| (c - subtract).max(0.0)));
            self.sky_zenith_rain = Vec4::new(sky[0], sky[1], sky[2], self.sky_zenith_rain.w);
        }
        if self.camera_medium() == CameraMedium::Air {
            let before = celestial::rgb_to_gamma(self.fog_color());
            let adjusted = view.fog_colour(before, angle);
            if adjusted == before {
                return self;
            }
            let fog = celestial::rgb_to_linear(adjusted);
            self.fog_color_start = Vec4::new(fog[0], fog[1], fog[2], self.fog_color_start.w);
            self.sky_horizon_thunder =
                Vec4::new(fog[0], fog[1], fog[2], self.sky_horizon_thunder.w);
        }
        self
    }

    /// The cloud caller uses the same narrow camera glare, in gamma space.
    #[must_use]
    pub fn cloud_colour_for_view(self, view: AtmosphereViewInputs) -> [f32; 4] {
        let mut colour = cloud_colour(
            self.celestial_angle(),
            self.rain_level(),
            self.thunder_level(),
            celestial::raw_sunrise_band(self.celestial_angle()),
        );
        let subtract = if self.sky_kind() == SkyKind::Overworld {
            view.colour_subtraction(self.celestial_angle())
        } else {
            0.0
        };
        for channel in &mut colour[..3] {
            *channel = (*channel - subtract).max(0.0);
        }
        colour
    }

    /// Responds to explicit boss-bar environment requests.
    ///
    /// No version-matched native Bedrock capture of the boss-driven sky
    /// darkening or world-fog effect exists yet, so these bounded constants
    /// only make the retained flags observable end to end. They must not be
    /// read as a vanilla parity claim; native calibration has to replace
    /// them before any visual acceptance gate can close, and that calibration
    /// must also adjudicate the dim-versus-target-mix semantics (mixing toward
    /// fixed targets can slightly brighten an already-dark night sky).
    #[must_use]
    pub fn with_boss_environment(mut self, darken_sky: bool, world_fog: bool) -> Self {
        let zenith = [
            self.sky_zenith_rain.x,
            self.sky_zenith_rain.y,
            self.sky_zenith_rain.z,
        ];
        let horizon = [
            self.sky_horizon_thunder.x,
            self.sky_horizon_thunder.y,
            self.sky_horizon_thunder.z,
        ];
        if darken_sky {
            let strength = PROVISIONAL_BOSS_DARKEN_SKY_STRENGTH.clamp(0.0, 1.0);
            let darkened_zenith = mix3(zenith, BOSS_DARKEN_ZENITH_TARGET, strength);
            let darkened_horizon = mix3(horizon, BOSS_DARKEN_HORIZON_TARGET, strength);
            self.sky_zenith_rain = Vec4::new(
                darkened_zenith[0],
                darkened_zenith[1],
                darkened_zenith[2],
                self.sky_zenith_rain.w,
            );
            self.sky_horizon_thunder = Vec4::new(
                darkened_horizon[0],
                darkened_horizon[1],
                darkened_horizon[2],
                self.sky_horizon_thunder.w,
            );
        }
        if world_fog {
            // Follow the storm model's convention of deriving the fog tint
            // from the current sky so the two effects compose predictably.
            let final_zenith = [
                self.sky_zenith_rain.x,
                self.sky_zenith_rain.y,
                self.sky_zenith_rain.z,
            ];
            let final_horizon = [
                self.sky_horizon_thunder.x,
                self.sky_horizon_thunder.y,
                self.sky_horizon_thunder.z,
            ];
            let colour = mix3(final_horizon, final_zenith, 0.18);
            self.fog_color_start = Vec4::new(
                colour[0],
                colour[1],
                colour[2],
                PROVISIONAL_BOSS_WORLD_FOG_START_BLOCKS,
            );
            self.fog_end_time.x = PROVISIONAL_BOSS_WORLD_FOG_END_BLOCKS;
        }
        self
    }

    #[must_use]
    pub fn sun_direction(self) -> [f32; 3] {
        [
            self.sun_direction_daylight.x,
            self.sun_direction_daylight.y,
            self.sun_direction_daylight.z,
        ]
    }

    #[must_use]
    pub fn moon_phase(self) -> u8 {
        self.moon_direction_phase.w as u8
    }

    #[must_use]
    pub fn day_fraction(self) -> f32 {
        self.fog_end_time.y
    }

    #[must_use]
    pub fn rain_level(self) -> f32 {
        self.sky_zenith_rain.w
    }

    #[must_use]
    pub fn sky_zenith(self) -> [f32; 3] {
        [
            self.sky_zenith_rain.x,
            self.sky_zenith_rain.y,
            self.sky_zenith_rain.z,
        ]
    }

    #[must_use]
    pub fn sky_horizon(self) -> [f32; 3] {
        [
            self.sky_horizon_thunder.x,
            self.sky_horizon_thunder.y,
            self.sky_horizon_thunder.z,
        ]
    }

    #[must_use]
    pub fn thunder_level(self) -> f32 {
        self.sky_horizon_thunder.w
    }

    #[must_use]
    pub fn fog_start(self) -> f32 {
        self.fog_color_start.w
    }

    #[must_use]
    pub fn fog_end(self) -> f32 {
        self.fog_end_time.x
    }

    #[must_use]
    pub fn fog_color(self) -> [f32; 3] {
        [
            self.fog_color_start.x,
            self.fog_color_start.y,
            self.fog_color_start.z,
        ]
    }

    #[must_use]
    pub fn camera_medium(self) -> CameraMedium {
        match (self.sky_extra.w / 4.0) as u32 {
            1 => CameraMedium::Water,
            2 => CameraMedium::Lava,
            _ => CameraMedium::Air,
        }
    }

    #[must_use]
    pub fn sky_kind(self) -> SkyKind {
        match (self.sky_extra.w % 4.0) as u32 {
            1 => SkyKind::Nether,
            2 => SkyKind::End,
            _ => SkyKind::Overworld,
        }
    }

    /// Eased celestial angle in turns; 0 is noon.
    #[must_use]
    pub fn celestial_angle(self) -> f32 {
        self.sky_extra.y
    }

    /// Atmosphere transfer, including lightning; not the classic terrain lightmap input.
    #[must_use]
    pub fn daylight(self) -> f32 {
        self.sun_direction_daylight.w
    }

    #[must_use]
    pub fn star_brightness(self) -> f32 {
        self.sky_extra.x
    }

    #[must_use]
    pub fn lightning_flash(self) -> f32 {
        self.sky_extra.z
    }

    /// Sunrise/sunset glow as linear rgb plus alpha.
    #[must_use]
    pub fn sunrise_band(self) -> [f32; 4] {
        self.sunrise_band.to_array()
    }

    #[must_use]
    pub fn cloud_texture_offset(self) -> [f32; 2] {
        [self.fog_end_time.z.rem_euclid(1.0), 0.0]
    }

    /// Native sample-space translation, before the cloud mesh's 16-block scale.
    #[must_use]
    pub fn cloud_scroll_blocks(self) -> f32 {
        -self.fog_end_time.z * CLOUD_TEXTURE_WORLD_PERIOD as f32
    }

    /// Vanilla advances clouds even while daylight is paused.
    #[must_use]
    pub fn with_cloud_renderer_ticks(mut self, ticks: f64) -> Self {
        self.fog_end_time.z = if ticks.is_finite() {
            (-(ticks.max(0.0) * CLOUD_SCROLL_BLOCKS_PER_TICK) / CLOUD_TEXTURE_WORLD_PERIOD) as f32
        } else {
            0.0
        };
        self
    }

    /// Distance the cloud alpha fade scales by; zero when unset.
    #[must_use]
    pub fn cloud_fade_distance(self) -> f32 {
        self.fog_end_time.w
    }

    /// Sets the cloud fade distance, render distance times the quality scale in vanilla.
    #[must_use]
    pub fn with_cloud_fade_distance(mut self, blocks: f32) -> Self {
        self.fog_end_time.w = if blocks.is_finite() {
            blocks.max(0.0)
        } else {
            0.0
        };
        self
    }
}

fn bounded_level(level: f32) -> f32 {
    if level.is_finite() {
        level.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn lerp(left: f32, right: f32, amount: f32) -> f32 {
    left + (right - left) * amount
}

fn mix3(left: [f32; 3], right: [f32; 3], amount: f32) -> [f32; 3] {
    [
        lerp(left[0], right[0], amount),
        lerp(left[1], right[1], amount),
        lerp(left[2], right[2], amount),
    ]
}

fn overworld_zenith(angle: f32, temperature: f32, thunder: f32) -> [f32; 3] {
    let plateau = celestial::day_plateau(angle);
    let clear = celestial::sky_colour_for_temperature(temperature).map(|channel| channel * plateau);
    native_sky_colour(clear, thunder)
}

fn native_sky_colour(base: [f32; 3], thunder: f32) -> [f32; 3] {
    // Vanilla sky colour also mixes precipitation fog before thunder.
    // That stage waits for current-rain view inputs after profile resolution.
    // with_camera_environment restores its equivalent ordering before camera glare.
    // Outdoor ambient is 1; native culler-list camera exposure is a separate gate.
    let thunder = celestial::storm_tint(base, 0.0, thunder);
    celestial::rgb_to_linear(thunder)
}

fn overworld_fog_colour(
    angle: f32,
    air: [f32; 3],
    weather: [f32; 3],
    fog_weather_level: f32,
) -> [f32; 3] {
    let factors = native_sunlight::fog_multipliers(angle);
    let gamma = mix3(air, weather, fog_weather_level);
    celestial::rgb_to_linear(std::array::from_fn(|channel| {
        gamma[channel] * factors[channel]
    }))
}

fn valid_fog(fog: &ResolvedFog) -> bool {
    fog.start.is_finite()
        && fog.end.is_finite()
        && fog.end >= fog.start
        && fog
            .rgb
            .into_iter()
            .all(|c| c.is_finite() && (0.0..=1.0).contains(&c))
}

fn rgb8_to_gamma(rgb: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| ((rgb >> shift) & 0xff) as f32 / 255.0)
}

fn rgb8_to_linear(rgb: u32) -> [f32; 3] {
    [16, 8, 0].map(|shift| {
        let value = ((rgb >> shift) & 0xff) as f32 / 255.0;
        if value <= 0.040_45 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    })
}

#[cfg(test)]
mod tests {
    use super::{AtmosphereFrame, CameraMedium};

    #[test]
    fn camera_medium_overrides_only_the_distance_fog_contract() {
        let clear = AtmosphereFrame::from_bedrock_time(6_000.0, 0.25, 0.5);
        let water = clear.with_camera_medium(CameraMedium::Water);
        let lava = clear.with_camera_medium(CameraMedium::Lava);

        assert_eq!(water.camera_medium(), CameraMedium::Water);
        assert_eq!(lava.camera_medium(), CameraMedium::Lava);
        assert_eq!(water.sun_direction(), clear.sun_direction());
        assert_eq!(water.rain_level(), clear.rain_level());
        assert_eq!(water.thunder_level(), clear.thunder_level());
        assert_eq!(water.fog_start(), 0.0);
        assert_eq!(water.fog_end(), 60.0);
        assert_eq!(lava.fog_start(), 0.0);
        assert_eq!(lava.fog_end(), 0.64);
        assert_eq!(water.fog_color(), super::rgb8_to_linear(0x0044_AFF5));
        assert_eq!(lava.fog_color(), super::rgb8_to_linear(0x0099_1A00));
    }

    #[test]
    fn medium_survives_profile_fog() {
        let water = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0)
            .with_camera_medium(CameraMedium::Water);
        let profiled = water.with_environment_profile(
            None,
            Some(assets::ResolvedFog {
                start: 0.0,
                end: 40.0,
                rgb: super::rgb8_to_gamma(0x112233),
            }),
        );
        assert_eq!(profiled.camera_medium(), CameraMedium::Water);
        assert_eq!(profiled.fog_end(), 40.0);
    }

    #[test]
    fn night_darkens_sky_and_fog_and_shows_stars() {
        let noon = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
        let midnight = AtmosphereFrame::from_bedrock_time(18_000.0, 0.0, 0.0);
        assert!(midnight.sky_zenith()[2] < 0.01 && noon.sky_zenith()[2] > 0.9);
        assert!(midnight.fog_color()[2] < noon.fog_color()[2] * 0.2);
        assert_eq!(noon.star_brightness(), 0.0);
        assert!(midnight.star_brightness() > 0.4);
        assert!(midnight.daylight() < noon.daylight());
    }

    #[test]
    fn weather_fog_pulls_inward_and_lightning_flash_lights_the_world() {
        let air = assets::ResolvedFog {
            start: 235.0,
            end: 256.0,
            rgb: [0.5; 3],
        };
        let weather = assets::ResolvedFog {
            start: 59.0,
            end: 179.0,
            rgb: [0.4; 3],
        };
        let base = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
        let clear = base.with_blended_fog(Some(air), Some(weather), 0.0);
        let rain = base.with_blended_fog(Some(air), Some(weather), 1.0);
        assert!(rain.fog_start() < clear.fog_start() && rain.fog_end() < clear.fog_end());
        let night = AtmosphereFrame::from_bedrock_time(18_000.0, 1.0, 1.0);
        let flashed = night.with_lightning_flash(1.0);
        assert!((flashed.daylight() - 1.0).abs() < 1.0e-6);
        assert!(flashed.sky_zenith()[2] > night.sky_zenith()[2]);
        assert_eq!(night.with_lightning_flash(0.0), night);
    }

    #[test]
    fn vision_effects_are_identity_at_zero_and_shape_fog_and_light() {
        let noon = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0);
        assert_eq!(noon.with_vision_effects(0.0, 0.0, 0.0), noon);
        let blind = noon.with_vision_effects(1.0, 0.0, 0.0);
        assert_eq!((blind.fog_start(), blind.fog_end()), (0.0, 5.0));
        assert_eq!(blind.fog_color(), [0.0; 3]);
        let dark = noon.with_vision_effects(0.0, 1.0, 0.0);
        assert_eq!(dark.daylight(), noon.daylight());
        assert!(dark.fog_end() < noon.fog_end());
        let night = AtmosphereFrame::from_bedrock_time(18_000.0, 0.0, 0.0);
        assert_eq!(
            night.with_vision_effects(0.0, 0.0, 1.0).daylight(),
            night.daylight()
        );
    }

    #[test]
    fn nether_and_end_have_no_day_cycle() {
        let frame = AtmosphereFrame::from_bedrock_time(18_000.0, 0.5, 0.0)
            .with_sky_kind(super::SkyKind::Nether);
        assert_eq!(frame.sky_kind(), super::SkyKind::Nether);
        assert_eq!(frame.daylight(), 1.0);
        assert_eq!(frame.star_brightness(), 0.0);
        assert_eq!(frame.sunrise_band()[3], 0.0);
        assert_eq!(super::SkyKind::from_dimension(2), super::SkyKind::End);
        assert_eq!(super::SkyKind::from_dimension(7), super::SkyKind::Overworld);
    }

    #[test]
    fn blended_fog_interpolates_by_weather_fog_independently_of_rain() {
        let air = assets::ResolvedFog {
            start: 235.0,
            end: 256.0,
            rgb: super::rgb8_to_gamma(0xABD2FF),
        };
        let weather = assets::ResolvedFog {
            start: 59.0,
            end: 179.0,
            rgb: super::rgb8_to_gamma(0x666666),
        };
        let clear = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0).with_blended_fog(
            Some(air),
            Some(weather),
            0.0,
        );
        assert_eq!((clear.fog_start(), clear.fog_end()), (235.0, 256.0));
        let half = AtmosphereFrame::from_bedrock_time(6_000.0, 0.0, 0.0).with_blended_fog(
            Some(air),
            Some(weather),
            0.5,
        );
        assert_eq!((half.fog_start(), half.fog_end()), (147.0, 217.5));
        assert_eq!(half.sky_horizon(), half.fog_color());
    }

    #[test]
    fn air_medium_preserves_weather_fog_exactly() {
        let clear = AtmosphereFrame::from_bedrock_time(18_000.0, 0.75, 0.25);
        assert_eq!(clear.with_camera_medium(CameraMedium::Air), clear);
    }
}
