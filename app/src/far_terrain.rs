//! Moves column summaries from the world stream into the far-terrain cache and keeps the
//! cache on the server, world and dimension being played.

use std::time::Instant;

use bevy::prelude::{Local, Res, ResMut, Resource};
use chunk_pipeline::{FarColumnSummary, FarSummaryInbox, WorldStream};
use far_terrain::{ColumnPos, EncodedSummary, FarTerrainCache, FarTerrainConfig, NamespaceKey};

use crate::{runtime::world::ClientWorld, session::SessionController};

/// The far-terrain cache and whether the feature runs.
#[derive(Resource)]
pub(crate) struct FarTerrain {
    pub(crate) cache: FarTerrainCache,
    pub(crate) enabled: bool,
    /// Columns beyond the render distance kept loaded around the player.
    pub(crate) distance_chunks: i32,
    drained: Vec<FarColumnSummary>,
    retiring: Vec<Retiring>,
}

/// A retired session's final summaries, written to its namespace once all have arrived.
struct Retiring {
    namespace: NamespaceKey,
    inbox: FarSummaryInbox,
    columns: Vec<(ColumnPos, EncodedSummary)>,
}

impl FarTerrain {
    /// Summarises the stream's resident columns before it is dropped, so this session's
    /// edits reach the cache even though no column retires through eviction.
    pub(crate) fn retire_stream(&mut self, stream: &mut WorldStream) {
        let Some(namespace) = self
            .cache
            .namespace()
            .filter(|_| stream.far_terrain_enabled())
        else {
            return;
        };
        self.retiring.push(Retiring {
            namespace,
            inbox: stream.retire_far_terrain(),
            columns: Vec::new(),
        });
    }

    /// Collects retiring summaries and persists each session's once its inbox is empty.
    fn drain_retiring(&mut self, now: Instant) {
        let mut index = 0;
        while index < self.retiring.len() {
            let retiring = &mut self.retiring[index];
            retiring.inbox.drain(&mut self.drained);
            let dimension = retiring.namespace.dimension;
            retiring.columns.extend(
                self.drained
                    .drain(..)
                    .filter(|finished| finished.column.dimension == dimension)
                    .map(|finished| {
                        let column = finished.column;
                        (ColumnPos::new(column.x, column.z), finished.summary)
                    }),
            );
            if retiring.inbox.is_idle() {
                let done = self.retiring.swap_remove(index);
                self.cache.persist(done.namespace, done.columns, now);
            } else {
                index += 1;
            }
        }
    }

    /// A cache persisted under `disk_root`, or memory-only without one.
    pub(crate) fn new(disk_root: Option<std::path::PathBuf>) -> Self {
        Self {
            cache: FarTerrainCache::new(FarTerrainConfig::default(), disk_root),
            enabled: false,
            distance_chunks: 0,
            drained: Vec::new(),
            retiring: Vec::new(),
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
    let now = Instant::now();
    far.drain_retiring(now);
    if world
        .stream
        .as_ref()
        .is_some_and(|stream| enabled && stream.has_far_summaries())
        && let Some(stream) = world.stream.as_mut()
    {
        stream.drain_far_summaries(&mut far.drained);
    }
    let Some(stream) = world.stream.as_ref().filter(|_| enabled) else {
        far.cache.poll(now);
        return;
    };
    for FarColumnSummary { column, summary } in far.drained.drain(..) {
        far.cache.admit(
            column.dimension,
            ColumnPos::new(column.x, column.z),
            summary,
            now,
        );
    }
    let centre = stream.far_terrain_centre();
    let radius = (stream.render_distance_blocks() / 16.0).ceil() as i32 + far.distance_chunks;
    far.cache
        .set_focus(ColumnPos::new(centre.x, centre.z), radius);
    far.cache.poll(now);
}
