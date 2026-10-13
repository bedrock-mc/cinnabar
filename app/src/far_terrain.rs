//! Moves column summaries from the world stream into the far-terrain cache and keeps the
//! cache on the server, world and dimension being played.

use std::time::{Duration, Instant};

use bevy::prelude::{AppExit, Local, MessageReader, Res, ResMut, Resource};
use chunk_pipeline::{FarColumnSummary, FarSummaryInbox, WorldStream};
use far_terrain::{ColumnPos, EncodedSummary, FarTerrainCache, FarTerrainConfig, RetiringSession};

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

/// Longest an exit waits for retiring sessions' final summaries.
const EXIT_SUMMARY_WAIT: Duration = Duration::from_secs(1);

/// A retired session's final summaries, written to its namespace once all have arrived.
struct Retiring {
    session: RetiringSession,
    inbox: FarSummaryInbox,
    columns: Vec<(ColumnPos, EncodedSummary)>,
}

impl FarTerrain {
    /// Summarises the stream's resident columns before it is dropped, so this session's
    /// edits reach the cache even though no column retires through eviction.
    pub(crate) fn retire_stream(&mut self, stream: &mut WorldStream) {
        if !stream.far_terrain_enabled() {
            return;
        }
        let Some(session) = self.cache.retire_session() else {
            return;
        };
        self.retiring.push(Retiring {
            session,
            inbox: stream.retire_far_terrain(),
            columns: Vec::new(),
        });
    }

    /// Collects retiring summaries and persists each session's once its inbox is empty.
    fn drain_retiring(&mut self, now: Instant) {
        self.drain_retiring_until(now, None);
    }

    /// Like [`Self::drain_retiring`], blocking until `deadline` for summaries in flight.
    fn drain_retiring_until(&mut self, now: Instant, deadline: Option<Instant>) {
        let mut index = 0;
        while index < self.retiring.len() {
            let retiring = &mut self.retiring[index];
            match deadline {
                Some(deadline) => retiring.inbox.drain_waiting(&mut self.drained, deadline),
                None => retiring.inbox.drain(&mut self.drained),
            }
            let dimension = retiring.session.dimension();
            retiring.columns.extend(
                self.drained
                    .drain(..)
                    .filter(|finished| finished.column.dimension == dimension)
                    .map(|finished| {
                        let column = finished.column;
                        (ColumnPos::new(column.x, column.z), finished.summary)
                    }),
            );
            let expired = deadline.is_some_and(|deadline| Instant::now() >= deadline);
            if retiring.inbox.is_idle() || expired {
                let done = self.retiring.remove(index);
                self.cache.persist(done.session, done.columns, now);
            } else if deadline.is_none() {
                index += 1;
            }
        }
    }

    /// Summarises the live session and writes every retiring session's summaries, waiting
    /// at most [`EXIT_SUMMARY_WAIT`], since the app ends before another frame drains them.
    fn finish(&mut self, stream: Option<&mut WorldStream>) {
        if let Some(stream) = stream {
            self.retire_stream(stream);
        }
        let now = Instant::now();
        self.drain_retiring_until(now, Some(now + EXIT_SUMMARY_WAIT));
        self.cache.flush_all();
        self.cache.sync();
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

/// Writes far-terrain summaries still in flight when the app exits.
pub(crate) fn finish_far_terrain_on_exit(
    mut exits: MessageReader<AppExit>,
    far: Option<ResMut<FarTerrain>>,
    world: Option<ResMut<ClientWorld>>,
) {
    if exits.read().next().is_none() {
        return;
    }
    let Some(mut far) = far else {
        return;
    };
    let mut world = world;
    let stream = world.as_mut().and_then(|world| world.stream.as_mut());
    far.finish(stream);
}

#[cfg(test)]
mod tests {
    use bevy::prelude::{App, Last};
    use far_terrain::{ColumnSummary, SurfaceCell, config::WORLD_PROBE_COLUMNS};

    use super::*;

    const SERVER: &str = "remote:play.example.net:19132";

    fn flat(height: i16) -> EncodedSummary {
        let mut summary = ColumnSummary::default();
        summary.cells_mut().fill(SurfaceCell {
            height,
            rgb: [90, 140, 60],
            water_depth: 0,
        });
        summary.encode()
    }

    fn join(cache: &mut FarTerrainCache, now: Instant) {
        cache.enter_world(SERVER, 42, 0);
        for x in 0..WORLD_PROBE_COLUMNS as i32 {
            cache.admit(0, ColumnPos::new(x, 0), flat(64), now);
        }
        cache.sync();
        cache.poll(now);
        assert!(cache.namespace().is_some());
    }

    /// Summaries a retiring session collected reach disk when the app exits that frame.
    #[test]
    fn exit_writes_retiring_summaries() {
        let root = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut far = FarTerrain::new(Some(root.path().to_owned()));
        join(&mut far.cache, now);
        let session = far.cache.retire_session().unwrap();
        far.retiring.push(Retiring {
            session,
            inbox: FarSummaryInbox::default(),
            columns: vec![(ColumnPos::new(300, 0), flat(90))],
        });
        far.cache.leave_world();
        let mut app = App::new();
        app.add_message::<AppExit>();
        app.insert_resource(far)
            .add_systems(Last, finish_far_terrain_on_exit);
        app.world_mut().write_message(AppExit::Success);
        app.update();

        let mut reader =
            FarTerrainCache::new(FarTerrainConfig::default(), Some(root.path().into()));
        join(&mut reader, now);
        reader.set_focus(ColumnPos::new(300, 0), 4);
        reader.sync();
        reader.poll(now);
        let stored = reader.store().encoded(ColumnPos::new(300, 0)).cloned();
        assert_eq!(stored.map(|summary| summary.hash), Some(flat(90).hash));
    }
}
