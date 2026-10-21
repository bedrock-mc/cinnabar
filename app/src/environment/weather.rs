//! Client-side weather presentation: eased levels, lightning flash and the precipitation scene.

use bevy::{
    prelude::{DetectChangesMut, Local, Query, Res, ResMut, Time, Transform, With},
    time::Real,
};
use chunk_pipeline::WorldStream;
use {
    render_model::ColumnSampler,
    render_model::{
        ColumnSample, OcclusionGrid, PRECIPITATION_SAMPLE_OFFSETS, PRECIPITATION_TICKS_PER_SECOND,
        PrecipitationMix, PrecipitationSim, average_precipitation, pick_rain_splashes,
        precipitation_forward_offset,
    },
    render_scene::{
        AtmosphereFrame, LightningScene, PrecipitationMixResource, PrecipitationScene,
        RainSplashQueue, SkyKind, lightning_bolt_segments, push_bolt_records,
    },
};

use client_presentation::environment::weather::LightningFlashState;
use {crate::runtime::world::ClientWorld, client_presentation::camera::FlyCamera};

const MAX_QUEUED_SPLASHES: usize = 256;
/// Ticks a bolt stays drawn after it spawns; needs native measurement.
const BOLT_VISIBLE_TICKS: u32 = 8;

const WEATHER_TEXTURES_FILENAME: &str = assets::carriers::WEATHER.output;
const WEATHER_TEXTURES_COMPILE_COMMAND: &str = "make weather-assets";

/// Loads the optional precipitation and End sky carrier next to the world carrier; an absent or
/// invalid carrier logs a notice and leaves the procedural fallbacks in place.
#[must_use]
pub(crate) fn load_optional_weather_textures(
    world_asset_path: &std::path::Path,
) -> render_scene::WeatherTextureAssets {
    let path = world_asset_path.with_file_name(WEATHER_TEXTURES_FILENAME);
    let decoded = std::fs::read(&path)
        .map_err(|error| error.to_string())
        .and_then(|bytes| {
            assets::weather_textures::decode_weather_textures(&bytes)
                .map_err(|error| error.to_string())
        });
    match decoded {
        Ok((textures, identity)) => {
            bevy::log::info!("loaded weather textures from {}", path.display());
            render_scene::WeatherTextureAssets::new(std::sync::Arc::new(textures), identity)
        }
        Err(error) => {
            bevy::log::warn!(
                "weather textures unavailable at {} ({error}); using procedural precipitation and End sky; build with {WEATHER_TEXTURES_COMPILE_COMMAND}",
                path.display()
            );
            render_scene::WeatherTextureAssets::default()
        }
    }
}

struct StreamColumns<'a> {
    stream: &'a WorldStream,
}

impl ColumnSampler for StreamColumns<'_> {
    fn sample(&mut self, x: i32, z: i32) -> Option<ColumnSample> {
        let top = self.stream.top_non_air_block_y(x, z)?;
        let biome =
            self.stream
                .camera_biome_id([x as f32 + 0.5, top as f32 + 0.5, z as f32 + 0.5])?;
        let registry = self.stream.authority().resolved_biome_tints();
        let rule = registry.biome(biome)?;
        Some(ColumnSample {
            surface_y: top.saturating_add(1),
            temperature: rule.temperature,
            downfall: rule.downfall,
        })
    }
}

/// Occlusion columns re-sampled per tick; the whole grid refreshes about every 0.8 s.
const OCCLUSION_REFRESH_PER_TICK: usize = 256;
/// Most simulation ticks replayed after a stall.
const MAX_CATCH_UP_TICKS: u64 = 10;
const PRECIPITATION_SEED: u64 = 0x5241_494e;

pub(crate) struct PrecipitationCadence {
    sim: PrecipitationSim,
    grid: OcclusionGrid,
    cursor: usize,
    last_tick: Option<u64>,
}

impl Default for PrecipitationCadence {
    fn default() -> Self {
        Self {
            sim: PrecipitationSim::new(PRECIPITATION_SEED),
            grid: OcclusionGrid::default(),
            cursor: 0,
            last_tick: None,
        }
    }
}

/// Biome (temperature, downfall) at a block position, when its chunk and biome are known.
fn biome_climate(stream: &WorldStream, position: [f32; 3]) -> Option<(f32, f32)> {
    let id = stream.camera_biome_id(position)?;
    let rule = stream.authority().resolved_biome_tints().biome(id)?;
    Some((rule.temperature, rule.downfall))
}

/// Ticks the vanilla precipitation layers, refreshes the occlusion grid and queues rain splashes.
///
/// The cadence exists only while precipitation shows: dropping it when the rain stops and
/// building a fresh one when it starts again equals resetting it every dry frame, without
/// rebuilding its occlusion grid and noise table each of those frames. Dry frames touch the
/// scene and mix only when they still hold rain, so they are not re-extracted.
#[allow(clippy::too_many_arguments)]
pub(crate) fn update_precipitation_scene(
    frame: Res<AtmosphereFrame>,
    client_world: Res<ClientWorld>,
    camera: Query<&Transform, With<FlyCamera>>,
    time: Res<Time<Real>>,
    mut scene: ResMut<PrecipitationScene>,
    mut splashes: ResMut<RainSplashQueue>,
    mut mix: ResMut<PrecipitationMixResource>,
    mut cadence: Local<Option<PrecipitationCadence>>,
    mut picked: Local<Vec<[f32; 3]>>,
) {
    let level = frame.rain_level();
    let stream = client_world.stream.as_ref();
    let camera = camera.single().ok();
    let (Some(stream), Some(camera), true) = (
        stream,
        camera,
        level > 0.0 && frame.sky_kind() == SkyKind::Overworld,
    ) else {
        if !scene.layers.is_empty() {
            scene.layers.clear();
        }
        mix.set_if_neq(PrecipitationMixResource(PrecipitationMix::default()));
        *cadence = None;
        return;
    };
    let cadence = cadence.get_or_insert_with(PrecipitationCadence::default);
    let elapsed = time.elapsed_secs_f64();
    let ticks = elapsed * PRECIPITATION_TICKS_PER_SECOND;
    let tick = ticks as u64;
    let origin = camera.translation.to_array();
    let pending = cadence
        .last_tick
        .map_or(1, |last| tick.saturating_sub(last).min(MAX_CATCH_UP_TICKS));
    if pending > 0 {
        cadence.last_tick = Some(tick);
        let feet = origin.map(f32::floor);
        let samples = PRECIPITATION_SAMPLE_OFFSETS.map(|offset| {
            let position = [
                feet[0] + offset[0] as f32,
                feet[1] + offset[1] as f32,
                feet[2] + offset[2] as f32,
            ];
            biome_climate(stream, position)
                .map(|(temperature, downfall)| (temperature, downfall, position[1] as i32))
        });
        let averaged = average_precipitation(&samples);
        mix.0 = PrecipitationMix {
            rain: averaged.rain * level,
            snow: averaged.snow * level,
        };
        let weights = averaged.lattice_weights(level);
        for step in 0..pending {
            let seconds = (tick - (pending - 1 - step)) as f64 / PRECIPITATION_TICKS_PER_SECOND;
            cadence.sim.tick(weights, seconds as f32);
        }
        let PrecipitationCadence { grid, cursor, .. } = &mut *cadence;
        let mut columns = StreamColumns { stream };
        let refresh = OCCLUSION_REFRESH_PER_TICK * pending as usize;
        grid.update(
            OcclusionGrid::origin_for(origin),
            &mut columns,
            cursor,
            refresh,
        );
        // Refreshed columns usually resample the same heights; only a changed grid is copied,
        // and the unchanged generation spares the render world an identical upload.
        if *scene.occlusion != *grid {
            scene.occlusion = std::sync::Arc::new(grid.clone());
            scene.occlusion_generation = scene.occlusion_generation.wrapping_add(1);
        }
        if splashes.positions.len() > MAX_QUEUED_SPLASHES {
            splashes.positions.clear();
        }
        pick_rain_splashes(grid, level, tick, &mut picked);
        splashes.positions.extend_from_slice(&picked);
    }
    scene.forward_offset = precipitation_forward_offset(camera.forward().as_vec3().to_array());
    let PrecipitationScene {
        layers,
        forward_offset,
        ..
    } = &mut *scene;
    cadence.sim.frame(
        camera.translation.as_dvec3().to_array(),
        *forward_offset,
        ticks.fract() as f32,
        layers,
    );
}

/// Flashes the sky when a lightning-bolt actor first appears and draws live bolts.
///
/// The scene is marked changed only when it held or now holds bolt records, so frames
/// without lightning are not re-extracted.
pub(crate) fn update_lightning(
    client_world: Res<ClientWorld>,
    time: Res<Time<Real>>,
    mut flash: ResMut<LightningFlashState>,
    mut scene: ResMut<LightningScene>,
    mut seen: Local<std::collections::HashSet<i64>>,
) {
    let had_records = !scene.records.is_empty();
    let records = &mut scene.bypass_change_detection().records;
    records.clear();
    if let Some(stream) = client_world.stream.as_ref() {
        let bolts = stream.authority().lightning_bolts();
        seen.retain(|id| bolts.iter().any(|bolt| bolt.unique_id == *id));
        for bolt in bolts {
            if seen.insert(bolt.unique_id) {
                flash.trigger(time.elapsed_secs_f64());
            }
            if bolt.age_ticks >= BOLT_VISIBLE_TICKS {
                continue;
            }
            let intensity = if bolt.age_ticks % 2 == 0 { 1.0 } else { 0.6 };
            push_bolt_records(
                &lightning_bolt_segments(bolt.unique_id as u64, bolt.position),
                intensity,
                records,
            );
        }
    } else {
        seen.clear();
    }
    if had_records || !scene.records.is_empty() {
        scene.set_changed();
    }
}
