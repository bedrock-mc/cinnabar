use std::{f32::consts::TAU, sync::Arc};

use assets::RuntimeAtmosphereAssets;
use bevy::{
    prelude::{Resource, Vec4},
    render::{extract_resource::ExtractResource, render_resource::ShaderType},
};

pub const BEDROCK_DAY_TICKS: f64 = 24_000.0;
pub const CLOUD_TEXTURE_WORLD_PERIOD: f64 = 256.0;
pub const CLOUD_SCROLL_BLOCKS_PER_TICK: f64 = 0.03;

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
