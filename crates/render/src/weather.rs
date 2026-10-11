//! Precipitation model after vanilla: biome lattice, per-kind intensity,
//! ten wrapped particle layers per kind, a 64x64 column occlusion grid and the splash hook.

use std::sync::Arc;

use bevy::{prelude::Resource, render::extract_resource::ExtractResource};

use crate::celestial::unit;

/// Rain level change per second while the server target moves.
/// Vanilla weather approaches its targets by this amount per tick.
pub const PRECIPITATION_LEVEL_PER_TICK: f32 = 0.01;
pub const PRECIPITATION_LEVEL_PER_SECOND: f32 =
    PRECIPITATION_LEVEL_PER_TICK * world::TICKS_PER_SECOND as f32;
/// Side of the cube the particle mesh wraps in, centred ahead of the camera.
pub const PARTICLE_BOX: f32 = 30.0;
/// Quads in the shared particle mesh.
pub const PARTICLE_MESH_QUADS: usize = 2500;
/// Particles a layer cycles through; its draw window wraps inside this pool.
pub const PARTICLE_POOL: u32 = 925;
/// Independently drifting copies of the mesh drawn per precipitation kind.
pub const LAYERS_PER_KIND: usize = 10;
/// Most layer records one frame can carry: rain and snow.
pub const MAX_PRECIPITATION_LAYERS: usize = 2 * LAYERS_PER_KIND;
/// Columns per side of the occlusion grid centred on the camera column.
pub const OCCLUSION_SIDE: i32 = 64;
/// Occlusion height of a column that never shows this kind.
pub const OCCLUSION_BLOCKED: i32 = i32::MAX;
/// Occlusion height of an unloaded column; precipitation shows at any height.
pub const OCCLUSION_OPEN: i32 = i32::MIN;
/// Simulation ticks per second.
pub const PRECIPITATION_TICKS_PER_SECOND: f64 = world::TICKS_PER_SECOND as f64;

const SNOW_TEMPERATURE: f32 = 0.15;
const TEMPERATURE_LOSS_PER_BLOCK_ABOVE_SEA: f32 = 0.05 / 30.0;
const SEA_LEVEL: f32 = 64.0;
const LATTICE_WEIGHT: f32 = 0.5;
const INTENSITY_BLEND: f32 = 0.5;
const INTENSITY_DIVISOR: f32 = 10.0;
/// Particle density halving the vanilla renderer applies unless a view flag is set.
const DENSITY_SCALE: f32 = 0.5;
const WIND_NOISE_TIME_SCALE: f32 = 0.1;
const WIND_VERTICAL_SCALE: f32 = 0.1;
const MAX_SPLASHES_PER_TICK: f32 = 4.0;
const SPLASH_RADIUS: i32 = 10;

/// What falls in one column.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Precipitation {
    None,
    Rain,
    Snow,
}

/// Temperature after the altitude lapse that turns rain into snow on high ground.
#[must_use]
pub fn altitude_adjusted_temperature(temperature: f32, surface_y: i32) -> f32 {
    let above = (surface_y as f32 - SEA_LEVEL).max(0.0);
    temperature - above * TEMPERATURE_LOSS_PER_BLOCK_ABOVE_SEA
}

/// Rain, snow or nothing for a biome: zero downfall never precipitates, cold falls as snow.
#[must_use]
pub fn classify_precipitation(temperature: f32, downfall: f32, surface_y: i32) -> Precipitation {
    if !temperature.is_finite() || !downfall.is_finite() || downfall <= 0.0 {
        return Precipitation::None;
    }
    if altitude_adjusted_temperature(temperature, surface_y) <= SNOW_TEMPERATURE {
        Precipitation::Snow
    } else {
        Precipitation::Rain
    }
}

/// Moves `current` toward `target` by at most `max_step`; invalid input reads as clear.
#[must_use]
pub fn approach_level(current: f32, target: f32, max_step: f32) -> f32 {
    let (current, target) = (unit(current), unit(target));
    let step = if max_step.is_finite() {
        max_step.max(0.0)
    } else {
        0.0
    };
    current + (target - current).clamp(-step, step)
}

/// Vanilla's per-kind rain and snow constants.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PrecipitationParams {
    /// Blocks per tick at unit speed.
    pub fall_speed: f32,
    /// Streak length in blocks at unit speed.
    pub length: f32,
    /// Quad width in clip units at the reference projection.
    pub width: f32,
    pub wind: f32,
    pub gravity: f32,
    /// Sheet cell: u, v offset then u, v size; sprites step along u.
    pub uv_rect: [f32; 4],
}

pub const RAIN_PARAMS: PrecipitationParams = PrecipitationParams {
    fall_speed: 0.6,
    length: 0.5,
    width: 0.1,
    wind: 0.1,
    gravity: 1.0,
    uv_rect: [0.0, 0.125, 0.125, 0.5],
};

pub const SNOW_PARAMS: PrecipitationParams = PrecipitationParams {
    fall_speed: 0.05,
    length: 0.2,
    width: 0.2,
    wind: 0.05,
    gravity: 1.0,
    uv_rect: [0.0, 0.0, 0.125, 0.125],
};

impl PrecipitationParams {
    /// Shader `Dimensions`: clip width, then streak length per unit of velocity.
    #[must_use]
    pub fn dimensions(&self) -> [f32; 2] {
        [self.width, self.length / self.fall_speed]
    }
}

/// Particles each layer draws for a smoothed kind intensity.
#[must_use]
pub fn particles_per_layer(intensity: f32) -> u32 {
    if !intensity.is_finite() || intensity <= 0.0 {
        return 0;
    }
    let count = (intensity * DENSITY_SCALE / INTENSITY_DIVISOR * PARTICLE_POOL as f32).floor();
    count.min(PARTICLE_MESH_QUADS as f32) as u32
}

/// Positive remainder in `0..=modulus`, as the vanilla offset wrap computes it.
fn wrap(value: f32, modulus: f32) -> f32 {
    let remainder = value % modulus;
    if remainder < 0.0 {
        remainder + modulus
    } else {
        remainder
    }
}

/// Static particle mesh: positions in `[box, 2 box)` and a sprite index in `0..8` per quad.
#[must_use]
pub fn particle_mesh(seed: u64) -> Vec<[f32; 4]> {
    let mut random = SplitMix(seed);
    (0..PARTICLE_MESH_QUADS)
        .map(|_| {
            let sprite = (random.next_u64() & 7) as f32;
            let [x, y, z] = [0; 3].map(|_| random.next_f32() * PARTICLE_BOX + PARTICLE_BOX);
            [x, y, z, sprite]
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Layer {
    speed: f32,
    gust: f32,
    base: [f32; 3],
    previous_base: [f32; 3],
    offset: [f32; 3],
    velocity: [f32; 3],
    previous_velocity: [f32; 3],
}

/// Ten drifting layers for rain and for snow plus the smoothed intensities that size them.
#[derive(Clone, Debug)]
pub struct PrecipitationSim {
    layers: [[Layer; LAYERS_PER_KIND]; 2],
    intensity: [f32; 2],
    wind: Simplex,
}

impl PrecipitationSim {
    #[must_use]
    pub fn new(seed: u64) -> Self {
        let mut random = SplitMix(seed);
        let mut layers = [[Layer::default(); LAYERS_PER_KIND]; 2];
        for layer in layers.iter_mut().flatten() {
            layer.speed = random.next_f32() * 0.5 + 0.75;
            layer.base = [0; 3].map(|_| random.next_f32() * PARTICLE_BOX);
            layer.previous_base = layer.base;
            layer.gust = random.next_f32() * 0.5 + 0.5;
            layer.offset = [0; 3].map(|_| random.next_f32() * PARTICLE_BOX);
        }
        Self {
            layers,
            intensity: [0.0; 2],
            wind: Simplex::new(random.next_u64()),
        }
    }

    /// Smoothed rain and snow intensities.
    #[must_use]
    pub fn intensity(&self) -> [f32; 2] {
        self.intensity
    }

    /// Advances one tick; `lattice` is the summed rain and snow sample weight, `seconds` world time.
    pub fn tick(&mut self, lattice: [f32; 2], seconds: f32) {
        for (intensity, target) in self.intensity.iter_mut().zip(lattice) {
            let target = if target.is_finite() {
                target.max(0.0)
            } else {
                0.0
            };
            *intensity = *intensity * (1.0 - INTENSITY_BLEND) + target * INTENSITY_BLEND;
        }
        let time = if seconds.is_finite() {
            seconds * WIND_NOISE_TIME_SCALE
        } else {
            0.0
        };
        let wind = [
            self.wind.sample(time, 0.1),
            self.wind.sample(time, 0.5) * WIND_VERTICAL_SCALE,
            self.wind.sample(time, 1.0),
        ];
        for (layers, params) in self.layers.iter_mut().zip([RAIN_PARAMS, SNOW_PARAMS]) {
            for layer in layers.iter_mut() {
                layer.previous_base = layer.base;
                layer.previous_velocity = layer.velocity;
                let gust = layer.speed * params.wind * layer.gust;
                layer.velocity = wind.map(|component| component * gust);
                layer.velocity[1] -= params.fall_speed * layer.speed * params.gravity;
                for axis in 0..3 {
                    layer.base[axis] = wrap(layer.base[axis] + layer.velocity[axis], PARTICLE_BOX);
                    layer.previous_base[axis] = layer.base[axis] - layer.velocity[axis];
                }
            }
        }
    }

    /// Layer records for a frame `alpha` of the way into the current tick.
    pub fn frame(
        &self,
        camera: [f64; 3],
        forward_offset: [f32; 3],
        alpha: f32,
        out: &mut Vec<PrecipitationLayerRecord>,
    ) {
        out.clear();
        let alpha = unit(alpha);
        let box_size = f64::from(PARTICLE_BOX);
        let camera = camera.map(|value| {
            if value.is_finite() {
                value.rem_euclid(box_size) as f32
            } else {
                0.0
            }
        });
        let half = PARTICLE_BOX * 0.5;
        for (kind, (layers, params)) in self
            .layers
            .iter()
            .zip([RAIN_PARAMS, SNOW_PARAMS])
            .enumerate()
        {
            let count = particles_per_layer(self.intensity[kind]);
            if count == 0 {
                continue;
            }
            for layer in layers {
                let mut base_offset = [0.0; 4];
                let mut velocity = [0.0; 4];
                for axis in 0..3 {
                    let base = layer.previous_base[axis]
                        + (layer.base[axis] - layer.previous_base[axis]) * alpha;
                    base_offset[axis] = wrap(
                        wrap(base, PARTICLE_BOX) + layer.offset[axis]
                            - camera[axis]
                            - (forward_offset[axis] - half),
                        PARTICLE_BOX,
                    );
                    velocity[axis] = layer.previous_velocity[axis]
                        + (layer.velocity[axis] - layer.previous_velocity[axis]) * alpha;
                }
                out.push(PrecipitationLayerRecord {
                    base_offset,
                    velocity,
                    uv_rect: params.uv_rect,
                    dimensions: params.dimensions(),
                    first_particle: 0,
                    particle_count: count,
                    kind: kind as u32,
                    _pad: [0; 3],
                });
            }
        }
    }
}

/// Box centre ahead of the camera: half the box along the view direction.
#[must_use]
pub fn precipitation_forward_offset(forward: [f32; 3]) -> [f32; 3] {
    forward.map(|axis| {
        if axis.is_finite() {
            axis * PARTICLE_BOX * 0.5
        } else {
            0.0
        }
    })
}

/// One drawn layer in the layout `weather.wesl` reads.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub struct PrecipitationLayerRecord {
    pub base_offset: [f32; 4],
    pub velocity: [f32; 4],
    pub uv_rect: [f32; 4],
    pub dimensions: [f32; 2],
    pub first_particle: u32,
    pub particle_count: u32,
    /// 0 rain, 1 snow; selects the occlusion plane.
    pub kind: u32,
    pub _pad: [u32; 3],
}

/// Per-column heights below which each kind is hidden, rain plane then snow plane.
#[derive(Clone, Debug, PartialEq)]
pub struct OcclusionGrid {
    /// World x and z of the grid's minimum corner.
    pub origin: [i32; 2],
    pub heights: Vec<i32>,
    filled: bool,
}

impl Default for OcclusionGrid {
    fn default() -> Self {
        Self {
            origin: [0; 2],
            heights: vec![OCCLUSION_OPEN; 2 * (OCCLUSION_SIDE * OCCLUSION_SIDE) as usize],
            filled: false,
        }
    }
}

impl OcclusionGrid {
    /// Grid origin for a camera column.
    #[must_use]
    pub fn origin_for(camera: [f32; 3]) -> [i32; 2] {
        let half = OCCLUSION_SIDE / 2;
        [
            (camera[0].floor() as i32).saturating_sub(half),
            (camera[2].floor() as i32).saturating_sub(half),
        ]
    }

    fn index(&self, x: i32, z: i32) -> Option<usize> {
        let (dx, dz) = (
            x.wrapping_sub(self.origin[0]),
            z.wrapping_sub(self.origin[1]),
        );
        ((0..OCCLUSION_SIDE).contains(&dx) && (0..OCCLUSION_SIDE).contains(&dz))
            .then(|| (dz * OCCLUSION_SIDE + dx) as usize)
    }

    /// Rain and snow occlusion heights of a world column; `None` outside the grid.
    #[must_use]
    pub fn column(&self, x: i32, z: i32) -> Option<[i32; 2]> {
        let plane = (OCCLUSION_SIDE * OCCLUSION_SIDE) as usize;
        self.index(x, z)
            .map(|index| [self.heights[index], self.heights[plane + index]])
    }

    /// Recentres on `origin`, keeping overlapping columns, then refreshes `refresh` columns
    /// round-robin from `cursor` plus every column the move exposed.
    pub fn update(
        &mut self,
        origin: [i32; 2],
        sampler: &mut impl ColumnSampler,
        cursor: &mut usize,
        refresh: usize,
    ) {
        let plane = (OCCLUSION_SIDE * OCCLUSION_SIDE) as usize;
        let mut stale = vec![!self.filled; plane];
        if self.filled && origin != self.origin {
            let previous = std::mem::take(self);
            self.origin = origin;
            for (index, stale) in stale.iter_mut().enumerate() {
                let (x, z) = self.world(index);
                match previous.column(x, z) {
                    Some([rain, snow]) => {
                        self.heights[index] = rain;
                        self.heights[plane + index] = snow;
                    }
                    None => *stale = true,
                }
            }
        }
        for step in 0..refresh.min(plane) {
            stale[(*cursor + step) % plane] = true;
        }
        *cursor = (*cursor + refresh) % plane;
        self.origin = origin;
        self.filled = true;
        for index in (0..plane).filter(|index| stale[*index]) {
            let (x, z) = self.world(index);
            let [rain, snow] = column_heights(sampler.sample(x, z));
            self.heights[index] = rain;
            self.heights[plane + index] = snow;
        }
    }

    fn world(&self, index: usize) -> (i32, i32) {
        let index = index as i32;
        (
            self.origin[0].saturating_add(index % OCCLUSION_SIDE),
            self.origin[1].saturating_add(index / OCCLUSION_SIDE),
        )
    }
}

/// Rain and snow occlusion heights for one sampled column.
#[must_use]
pub fn column_heights(sample: Option<ColumnSample>) -> [i32; 2] {
    let Some(sample) = sample else {
        return [OCCLUSION_OPEN; 2];
    };
    match classify_precipitation(sample.temperature, sample.downfall, sample.surface_y) {
        Precipitation::Rain => [sample.surface_y, OCCLUSION_BLOCKED],
        Precipitation::Snow => [OCCLUSION_BLOCKED, sample.surface_y],
        Precipitation::None => [OCCLUSION_BLOCKED; 2],
    }
}

/// Surface facts of one world column, supplied by the world-stream owner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColumnSample {
    /// Y of the first block above the top non-air block.
    pub surface_y: i32,
    pub temperature: f32,
    pub downfall: f32,
}

/// Lookup of loaded world columns; `None` for unloaded ones.
pub trait ColumnSampler {
    fn sample(&mut self, x: i32, z: i32) -> Option<ColumnSample>;
}

/// Decoded weather sheet and End sky, when the optional carrier is present.
#[derive(Resource, ExtractResource, Clone, Default)]
#[extract_app(bevy::render::RenderApp)]
pub struct WeatherTextureAssets {
    textures: Option<Arc<assets::WeatherTextures>>,
    identity: [u8; 32],
}

impl WeatherTextureAssets {
    #[must_use]
    pub fn new(textures: Arc<assets::WeatherTextures>, identity: [u8; 32]) -> Self {
        Self {
            textures: Some(textures),
            identity,
        }
    }

    #[must_use]
    pub fn textures(&self) -> Option<&Arc<assets::WeatherTextures>> {
        self.textures.as_ref()
    }

    #[must_use]
    pub const fn identity(&self) -> [u8; 32] {
        self.identity
    }
}

/// Precipitation state consumed by the render world.
#[derive(Resource, ExtractResource, Clone, Debug, Default, PartialEq)]
#[extract_app(bevy::render::RenderApp)]
pub struct PrecipitationScene {
    pub layers: Vec<PrecipitationLayerRecord>,
    pub forward_offset: [f32; 3],
    pub occlusion: Arc<OcclusionGrid>,
    /// Bumped whenever `occlusion` changes, so the GPU copy uploads only then.
    pub occlusion_generation: u64,
}

/// Vanilla's biome sample lattice around the player for precipitation.
pub const PRECIPITATION_SAMPLE_OFFSETS: [[i32; 3]; 27] = {
    const RING: [[i32; 2]; 9] = [
        [0, 0],
        [-12, 0],
        [-8, -8],
        [0, -12],
        [8, -8],
        [12, 0],
        [8, 8],
        [0, 12],
        [-8, 8],
    ];
    let mut offsets = [[0; 3]; 27];
    let mut index = 0;
    while index < 27 {
        let [x, z] = RING[index % 9];
        offsets[index] = [x, [0, -3, 3][index / 9], z];
        index += 1;
    }
    offsets
};

/// Share of the surrounding biomes that rain and snow, each in `0..=1`.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq)]
pub struct PrecipitationMix {
    pub rain: f32,
    pub snow: f32,
}

impl PrecipitationMix {
    /// Summed lattice weights feeding the rain and snow intensities at `level`.
    #[must_use]
    pub fn lattice_weights(&self, level: f32) -> [f32; 2] {
        let scale = unit(level) * LATTICE_WEIGHT * PRECIPITATION_SAMPLE_OFFSETS.len() as f32;
        [self.rain * scale, self.snow * scale]
    }
}

/// Averages the lattice samples (`temperature, downfall, y`; unloaded ones are `None`) into a mix.
#[must_use]
pub fn average_precipitation(samples: &[Option<(f32, f32, i32)>]) -> PrecipitationMix {
    let (mut rain, mut snow) = (0.0, 0.0);
    for (temperature, downfall, y) in samples.iter().flatten() {
        match classify_precipitation(*temperature, *downfall, *y) {
            Precipitation::Rain => rain += 1.0,
            Precipitation::Snow => snow += 1.0,
            Precipitation::None => {}
        }
    }
    let total = PRECIPITATION_SAMPLE_OFFSETS.len() as f32;
    PrecipitationMix {
        rain: rain / total,
        snow: snow / total,
    }
}

/// Picks where rain lands this tick near the grid centre, for the particle system to spawn splashes.
pub fn pick_rain_splashes(grid: &OcclusionGrid, level: f32, tick: u64, out: &mut Vec<[f32; 3]>) {
    out.clear();
    let count = (unit(level) * MAX_SPLASHES_PER_TICK).round() as u64;
    let centre = OCCLUSION_SIDE / 2;
    let side = (2 * SPLASH_RADIUS + 1) as u64;
    for index in 0..count {
        let hash = mix64(tick.wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_add(index));
        let dx = (hash % side) as i32 - SPLASH_RADIUS;
        let dz = ((hash >> 8) % side) as i32 - SPLASH_RADIUS;
        let (x, z) = (
            grid.origin[0].saturating_add(centre + dx),
            grid.origin[1].saturating_add(centre + dz),
        );
        let Some([surface, _]) = grid.column(x, z) else {
            continue;
        };
        if surface == OCCLUSION_OPEN || surface == OCCLUSION_BLOCKED {
            continue;
        }
        let fraction = |shift: u32| ((hash >> shift) & 0xffff) as f32 / 65_536.0;
        out.push([
            x as f32 + fraction(16),
            surface as f32,
            z as f32 + fraction(32),
        ]);
    }
}

fn mix64(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

struct SplitMix(u64);

impl SplitMix {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mix64(self.0)
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1_u64 << 24) as f32
    }
}

/// 2D simplex noise in `-1..=1` over a seeded permutation.
#[derive(Clone, Debug)]
struct Simplex {
    permutation: [u8; 512],
}

impl Simplex {
    fn new(seed: u64) -> Self {
        let mut random = SplitMix(seed);
        let mut table: [u8; 256] = std::array::from_fn(|index| index as u8);
        for index in (1..256).rev() {
            table.swap(index, (random.next_u64() % (index as u64 + 1)) as usize);
        }
        Self {
            permutation: std::array::from_fn(|index| table[index & 255]),
        }
    }

    fn sample(&self, x: f32, y: f32) -> f32 {
        const F2: f32 = 0.366_025_42;
        const G2: f32 = 0.211_324_87;
        const GRADIENTS: [[f32; 2]; 8] = [
            [1.0, 1.0],
            [-1.0, 1.0],
            [1.0, -1.0],
            [-1.0, -1.0],
            [1.0, 0.0],
            [-1.0, 0.0],
            [0.0, 1.0],
            [0.0, -1.0],
        ];
        let skew = (x + y) * F2;
        let (i, j) = ((x + skew).floor(), (y + skew).floor());
        let unskew = (i + j) * G2;
        let corner0 = [x - (i - unskew), y - (j - unskew)];
        let step = if corner0[0] > corner0[1] {
            [1, 0]
        } else {
            [0, 1]
        };
        let corner1 = [
            corner0[0] - step[0] as f32 + G2,
            corner0[1] - step[1] as f32 + G2,
        ];
        let corner2 = [corner0[0] - 1.0 + 2.0 * G2, corner0[1] - 1.0 + 2.0 * G2];
        let (ii, jj) = ((i as i32 & 255) as usize, (j as i32 & 255) as usize);
        let hash = |di: usize, dj: usize| {
            usize::from(self.permutation[ii + di + usize::from(self.permutation[jj + dj])]) % 8
        };
        let contribution = |corner: [f32; 2], gradient: usize| {
            let falloff = 0.5 - corner[0] * corner[0] - corner[1] * corner[1];
            if falloff < 0.0 {
                0.0
            } else {
                let [gx, gy] = GRADIENTS[gradient];
                falloff.powi(4) * (gx * corner[0] + gy * corner[1])
            }
        };
        70.0 * (contribution(corner0, hash(0, 0))
            + contribution(corner1, hash(step[0], step[1]))
            + contribution(corner2, hash(1, 1)))
    }
}

/// Main-world queue of splash positions for the particle system to drain each frame.
#[derive(Resource, Debug, Default)]
pub struct RainSplashQueue {
    pub positions: Vec<[f32; 3]>,
}

#[cfg(test)]
mod tests;
