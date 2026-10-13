//! Moves column summaries from the world stream into the far-terrain cache and keeps the
//! cache on the server, world and dimension being played.

use std::time::Instant;

use bevy::prelude::{Local, Res, ResMut, Resource};
use chunk_pipeline::FarColumnSummary;
use far_terrain::{ColumnPos, FarTerrainCache, FarTerrainConfig};

use crate::{runtime::world::ClientWorld, session::SessionController};

/// The far-terrain cache and whether the feature runs.
#[derive(Resource)]
pub(crate) struct FarTerrain {
    pub(crate) cache: FarTerrainCache,
    pub(crate) enabled: bool,
    /// Columns beyond the render distance kept loaded around the player.
    pub(crate) distance_chunks: i32,
    drained: Vec<FarColumnSummary>,
}

impl FarTerrain {
    /// A cache persisted under `disk_root`, or memory-only without one.
    pub(crate) fn new(disk_root: Option<std::path::PathBuf>) -> Self {
        Self {
            cache: FarTerrainCache::new(FarTerrainConfig::default(), disk_root),
            enabled: false,
            distance_chunks: 0,
            drained: Vec::new(),
        }
    }
}

/// The world the cache was last pointed at, so unchanged frames skip the identity hash.
#[derive(Default)]
pub(crate) struct ActiveWorld(Option<(String, u64, i32)>);

/// Keeps the stream's summary extraction, the cache's namespace and its loaded area current.
pub(crate) fn feed_far_terrain(
    far: Option<ResMut<FarTerrain>>,
    world: Option<ResMut<ClientWorld>>,
    session: Option<Res<SessionController>>,
    mut active: Local<ActiveWorld>,
) {
    let (Some(mut far), Some(mut world)) = (far, world) else {
        return;
    };
    let far = &mut *far;
    let destination = session
        .as_deref()
        .and_then(SessionController::destination_identity);
    let target = world
        .stream
        .as_ref()
        .zip(destination)
        .filter(|_| far.enabled)
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
        active.0 = target
            .map(|(destination, level, dimension)| (destination.to_owned(), level, dimension));
    }
    let Some(stream) = world.stream.as_ref().filter(|_| enabled) else {
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
    let radius = (stream.render_distance_blocks() / 16.0).ceil() as i32 + far.distance_chunks;
    far.cache
        .set_focus(ColumnPos::new(centre.x, centre.z), radius);
    far.cache.poll(now);
}
