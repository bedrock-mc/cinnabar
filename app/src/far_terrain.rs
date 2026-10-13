//! Moves column summaries from the world stream into the far-terrain cache, keeps the cache
//! on the server, world and dimension being played, and publishes the tiles to draw.

use std::{sync::Arc, time::Instant};

use bevy::prelude::{Local, Res, ResMut, Resource, Time};
use chunk_pipeline::FarColumnSummary;
use client_state::RuntimeSettings;
use far_terrain::{
    ColumnPos, FarTerrainCache, FarTerrainConfig, FarTiles, NearMask, TileView,
    config::COVERAGE_RING_CHUNKS, ring_coverage,
};
use render_scene::FarTerrainScene;

use crate::{runtime::world::ClientWorld, session::SessionController};

/// Seconds for the fog reach to close most of the gap to a new coverage.
const FOG_SETTLE_SECONDS: f32 = 1.0;

/// Coverage is resampled at most this often while columns keep arriving.
const COVERAGE_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// The far-terrain cache, the selected tiles and the view they were chosen for.
#[derive(Resource)]
pub(crate) struct FarTerrain {
    cache: FarTerrainCache,
    tiles: FarTiles,
    drained: Vec<FarColumnSummary>,
    near: Arc<NearMask>,
    near_view: Option<([i32; 2], i32)>,
    published: Option<u64>,
    /// Share of the ring just past near terrain that has cached columns.
    coverage: f32,
    coverage_at: Option<Instant>,
    /// Columns changed since coverage was last sampled.
    coverage_stale: bool,
}

impl FarTerrain {
    /// A cache persisted under `disk_root`, or memory-only without one.
    pub(crate) fn new(disk_root: Option<std::path::PathBuf>) -> Self {
        Self {
            cache: FarTerrainCache::new(FarTerrainConfig::default(), disk_root),
            tiles: FarTiles::default(),
            drained: Vec::new(),
            near: Arc::new(NearMask::default()),
            near_view: None,
            published: None,
            coverage: 0.0,
            coverage_at: None,
            coverage_stale: false,
        }
    }
}

/// Blocks far terrain adds to the air-fog distance, eased toward its cached coverage so fog
/// never jumps and an uncached world keeps today's fog.
#[derive(Resource, Default)]
pub(crate) struct FarTerrainFog {
    blocks: f32,
}

impl FarTerrainFog {
    pub(crate) const fn blocks(&self) -> f32 {
        self.blocks
    }
}

/// The world the cache was last pointed at, so unchanged frames skip the identity hash.
#[derive(Default)]
pub(crate) struct ActiveWorld(Option<(String, u64, i32)>);

/// Keeps extraction, the cache namespace, the loaded area, the tiles and the fog reach current.
#[allow(clippy::too_many_arguments)]
pub(crate) fn feed_far_terrain(
    far: Option<ResMut<FarTerrain>>,
    world: Option<ResMut<ClientWorld>>,
    session: Option<Res<SessionController>>,
    settings: Res<RuntimeSettings>,
    time: Res<Time>,
    scene: Option<ResMut<FarTerrainScene>>,
    fog: Option<ResMut<FarTerrainFog>>,
    mut active: Local<ActiveWorld>,
) {
    let (Some(mut far), Some(mut world), Some(mut scene), Some(mut fog)) = (far, world, scene, fog)
    else {
        return;
    };
    let far = &mut *far;
    let video = settings.user_settings_update().1.video;
    let destination = session
        .as_deref()
        .and_then(SessionController::destination_identity);
    let target = world
        .stream
        .as_ref()
        .zip(destination)
        .filter(|_| video.far_terrain)
        .map(|(stream, destination)| {
            (
                destination,
                world.level_identity,
                stream.current_dimension(),
            )
        });
    let enabled = target.is_some();
    // Borrowed mutably only on a change, so the world is not marked changed every frame.
    if world
        .stream
        .as_ref()
        .is_some_and(|stream| stream.far_terrain_enabled() != enabled)
        && let Some(stream) = world.stream.as_mut()
    {
        stream.set_far_terrain_enabled(enabled);
    }
    let current = active
        .0
        .as_ref()
        .map(|(destination, level, dimension)| (destination.as_str(), *level, *dimension));
    if current != target {
        match target {
            Some((destination, level, dimension)) => {
                far.cache.enter_world(destination, level, dimension);
            }
            None => far.cache.leave_world(),
        }
        far.tiles.clear();
        far.coverage = 0.0;
        active.0 = target
            .map(|(destination, level, dimension)| (destination.to_owned(), level, dimension));
    }
    let Some(stream) = world.stream.as_ref().filter(|_| enabled) else {
        ease_fog(&mut fog, 0.0, time.delta_secs());
        if far.published.take().is_some() {
            *scene = FarTerrainScene::default();
        }
        return;
    };
    let now = Instant::now();
    stream.drain_far_summaries(&mut far.drained);
    for FarColumnSummary { column, summary } in far.drained.drain(..) {
        far.cache.admit(
            column.dimension,
            ColumnPos::new(column.x, column.z),
            summary,
            now,
        );
    }
    let centre = stream.player_column();
    let radius = stream
        .retained_view_radius()
        .unwrap_or_else(|| (stream.render_distance_blocks() / 16.0).ceil() as i32);
    let near_chunks = radius + ::world::CHUNK_VIEW_SLACK + 1;
    let far_chunks = video.far_terrain_distance_chunks as i32;
    far.cache
        .set_focus(ColumnPos::new(centre.x, centre.z), near_chunks + far_chunks);
    far.cache.poll(now);
    let changed = far.cache.store().take_changed();
    far.tiles.mark_changed(&changed);
    far.tiles.set_view(TileView {
        centre: [centre.x as f32 * 16.0 + 8.0, centre.z as f32 * 16.0 + 8.0],
        near_blocks: (near_chunks * 16) as f32,
        far_blocks: (far_chunks * 16) as f32,
    });
    far.tiles.update(far.cache.store(), now);
    let near_view = ([centre.x, centre.z], radius);
    let near_changed = far.near_view != Some(near_view);
    if near_changed {
        far.near = Arc::new(NearMask::retained(near_view.0, radius));
        far.near_view = Some(near_view);
    }
    let coverage_due = far
        .coverage_at
        .is_none_or(|at| now.saturating_duration_since(at) >= COVERAGE_INTERVAL);
    far.coverage_stale |= !changed.is_empty();
    if near_changed || (far.coverage_stale && coverage_due) {
        far.coverage_at = Some(now);
        far.coverage_stale = false;
        far.coverage = ring_coverage(
            far.cache.store(),
            ColumnPos::new(centre.x, centre.z),
            near_chunks,
            COVERAGE_RING_CHUNKS,
        );
    }
    ease_fog(
        &mut fog,
        far.coverage * (far_chunks * 16) as f32,
        time.delta_secs(),
    );
    let revision = far.tiles.revision();
    if near_changed || far.published != Some(revision) {
        *scene = FarTerrainScene {
            tiles: far.tiles.drawn().into(),
            near: Arc::clone(&far.near),
        };
        far.published = Some(revision);
    }
}

/// Moves the fog reach toward `target` with a one-second exponential ease.
fn ease_fog(fog: &mut FarTerrainFog, target: f32, seconds: f32) {
    if fog.blocks == target {
        return;
    }
    let blend = 1.0 - (-seconds / FOG_SETTLE_SECONDS).exp();
    fog.blocks += (target - fog.blocks) * blend;
    if (fog.blocks - target).abs() < 0.5 {
        fog.blocks = target;
    }
}
