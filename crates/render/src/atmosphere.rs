use std::{f32::consts::TAU, sync::Arc};

use assets::{ResolvedFog, RuntimeAtmosphereAssets};
use bevy::{
    prelude::{Resource, Vec4},
    render::{extract_resource::ExtractResource, render_resource::ShaderType},
};
use meshing::CameraMedium;

pub const BEDROCK_DAY_TICKS: f64 = 24_000.0;
pub const CLOUD_TEXTURE_WORLD_PERIOD: f64 = 256.0;
pub const CLOUD_SCROLL_BLOCKS_PER_TICK: f64 = 0.03;
const CLOUD_DIRECTIONAL_AMBIENT: f32 = 0.55;
const RAIN_CLOUD_CHANNEL: f32 = 191.0 / 255.0;
const THUNDER_CLOUD_CHANNEL: f32 = 30.0 / 255.0;
const WEATHER_COLOUR_CONTRIBUTION: f32 = 0.95;

const WATER_FOG_COLOR: [f32; 3] = [0.02, 0.12, 0.2];
const WATER_FOG_END: f32 = 32.0;
const LAVA_FOG_COLOR: [f32; 3] = [0.45, 0.08, 0.0];
const LAVA_FOG_END: f32 = 3.0;

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

/// Vanilla-style cloud motion in normalized texture space. The texture repeats
/// every 256 world blocks and moves east at 0.03 blocks per Bedrock tick.
#[must_use]
pub fn cloud_texture_offset(absolute_ticks: f64) -> [f32; 2] {
    let ticks = if absolute_ticks.is_finite() {
        absolute_ticks
    } else {
        0.0
    };
    [
        ((ticks * CLOUD_SCROLL_BLOCKS_PER_TICK) / CLOUD_TEXTURE_WORLD_PERIOD).rem_euclid(1.0)
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
    let rain = bounded_level(rain_level) * WEATHER_COLOUR_CONTRIBUTION;
    let thunder = bounded_level(thunder_level) * WEATHER_COLOUR_CONTRIBUTION;
    let rain_colour = lerp(1.0, RAIN_CLOUD_CHANNEL, rain);
    let weather_colour = lerp(rain_colour, THUNDER_CLOUD_CHANNEL, thunder);
    [weather_colour; 3]
}

/// Directional diffuse cloud illuminance shared with the legacy cloud shader.
///
/// Cloud faces retain a bounded ambient fill while the real sun direction
/// controls the remaining diffuse response. Invalid vectors or daylight are
/// rejected to darkness rather than producing non-finite GPU reference data.
#[must_use]
pub fn cloud_directional_illuminance(
    normal: [f32; 3],
    sun_direction: [f32; 3],
    daylight: f32,
) -> f32 {
    if normal.into_iter().any(|value| !value.is_finite())
        || sun_direction.into_iter().any(|value| !value.is_finite())
        || !daylight.is_finite()
    {
        return 0.0;
    }
    let normal_length = normal.iter().map(|value| value * value).sum::<f32>().sqrt();
    let sun_length = sun_direction
        .iter()
        .map(|value| value * value)
        .sum::<f32>()
        .sqrt();
    if normal_length <= f32::EPSILON || sun_length <= f32::EPSILON {
        return 0.0;
    }
    let directional = normal
        .into_iter()
        .zip(sun_direction)
        .map(|(normal, sun)| normal / normal_length * (sun / sun_length))
        .sum::<f32>()
        .max(0.0);
    bounded_level(daylight) * lerp(CLOUD_DIRECTIONAL_AMBIENT, 1.0, directional)
}

/// One deterministic, renderer-ready snapshot of the active Bedrock sky.
///
/// The six `vec4`-shaped records are also the complete GPU uniform. Keeping the
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
}

const _: () = assert!(std::mem::size_of::<AtmosphereFrame>() == 96);

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
        let day_ticks = absolute_ticks.rem_euclid(BEDROCK_DAY_TICKS);
        let day_fraction = (day_ticks / BEDROCK_DAY_TICKS) as f32;
        let angle = day_fraction * TAU;
        let (sin, cos) = angle.sin_cos();
        let sun_direction = [clean_unit(cos), clean_unit(sin), 0.0];
        let moon_direction = sun_direction.map(|component| -component);
        let moon_phase = ((absolute_ticks / BEDROCK_DAY_TICKS).floor().rem_euclid(8.0)) as u8;
        let cloud_offset = cloud_texture_offset(absolute_ticks);

        let daylight = (sun_direction[1] * 0.8 + 0.2).clamp(0.0, 1.0);
        let sunrise = (1.0 - sun_direction[1].abs()).powi(3) * (0.25 + daylight * 0.75);
        let storm = (rain * 0.55 + thunder * 0.3).clamp(0.0, 0.8);
        let clear_zenith = mix3([0.004, 0.008, 0.03], [0.18, 0.48, 0.88], daylight);
        let clear_horizon = mix3([0.018, 0.024, 0.065], [0.58, 0.78, 1.0], daylight);
        let warm_horizon = mix3(clear_horizon, [1.0, 0.36, 0.12], sunrise * 0.55);
        let storm_zenith = mix3(clear_zenith, [0.12, 0.14, 0.16], storm);
        let storm_horizon = mix3(warm_horizon, [0.22, 0.24, 0.26], storm);
        let fog_color = mix3(storm_horizon, storm_zenith, 0.18);
        let fog_start = lerp(192.0, 64.0, (rain * 0.8 + thunder * 0.2).clamp(0.0, 1.0));
        let fog_end = lerp(256.0, 112.0, (rain * 0.75 + thunder * 0.25).clamp(0.0, 1.0));

        Self {
            sun_direction_daylight: Vec4::new(
                sun_direction[0],
                sun_direction[1],
                sun_direction[2],
                daylight,
            ),
            moon_direction_phase: Vec4::new(
                moon_direction[0],
                moon_direction[1],
                moon_direction[2],
                f32::from(moon_phase),
            ),
            sky_zenith_rain: Vec4::new(storm_zenith[0], storm_zenith[1], storm_zenith[2], rain),
            sky_horizon_thunder: Vec4::new(
                storm_horizon[0],
                storm_horizon[1],
                storm_horizon[2],
                thunder,
            ),
            fog_color_start: Vec4::new(fog_color[0], fog_color[1], fog_color[2], fog_start),
            fog_end_time: Vec4::new(fog_end, day_fraction, cloud_offset[0], cloud_offset[1]),
        }
    }

    /// Applies camera-medium distance fog while retaining the exact celestial
    /// and weather snapshot. The bounded constants are the Phase 2.7 baseline;
    /// native reference calibration remains part of visual acceptance.
    #[must_use]
    pub fn with_camera_medium(mut self, medium: CameraMedium) -> Self {
        let (color, end) = match medium {
            CameraMedium::Air => return self,
            CameraMedium::Water => (WATER_FOG_COLOR, WATER_FOG_END),
            CameraMedium::Lava => (LAVA_FOG_COLOR, LAVA_FOG_END),
        };
        self.fog_color_start = Vec4::new(color[0], color[1], color[2], 0.0);
        self.fog_end_time.x = end;
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
            let colour = rgb8_to_linear(rgb);
            self.sky_zenith_rain.x = colour[0];
            self.sky_zenith_rain.y = colour[1];
            self.sky_zenith_rain.z = colour[2];
            self.sky_horizon_thunder.x = colour[0];
            self.sky_horizon_thunder.y = colour[1];
            self.sky_horizon_thunder.z = colour[2];
        }
        if let Some(fog) = fog.filter(|fog| {
            fog.start.is_finite()
                && fog.end.is_finite()
                && fog.start >= 0.0
                && fog.end >= fog.start
                && fog.rgb8 <= 0x00ff_ffff
        }) {
            let colour = rgb8_to_linear(fog.rgb8);
            self.fog_color_start = Vec4::new(colour[0], colour[1], colour[2], fog.start);
            self.fog_end_time.x = fog.end;
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
        if self.fog_end_time.x == WATER_FOG_END && self.fog_color() == WATER_FOG_COLOR {
            CameraMedium::Water
        } else if self.fog_end_time.x == LAVA_FOG_END && self.fog_color() == LAVA_FOG_COLOR {
            CameraMedium::Lava
        } else {
            CameraMedium::Air
        }
    }

    #[must_use]
    pub fn cloud_texture_offset(self) -> [f32; 2] {
        [self.fog_end_time.z, self.fog_end_time.w]
    }
}

fn bounded_level(level: f32) -> f32 {
    if level.is_finite() {
        level.clamp(0.0, 1.0)
    } else {
        0.0
    }
}

fn clean_unit(value: f32) -> f32 {
    if value.abs() < 1.0e-6 { 0.0 } else { value }
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
        assert_eq!(water.fog_end(), 32.0);
        assert_eq!(lava.fog_start(), 0.0);
        assert_eq!(lava.fog_end(), 3.0);
        assert_eq!(water.fog_color(), [0.02, 0.12, 0.2]);
        assert_eq!(lava.fog_color(), [0.45, 0.08, 0.0]);
    }

    #[test]
    fn air_medium_preserves_weather_fog_exactly() {
        let clear = AtmosphereFrame::from_bedrock_time(18_000.0, 0.75, 0.25);
        assert_eq!(clear.with_camera_medium(CameraMedium::Air), clear);
    }
}
