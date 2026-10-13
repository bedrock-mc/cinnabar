//! Moves column summaries from the world stream into the far-terrain cache, keeps the cache
//! on the server, world and dimension being played, and publishes the tiles to draw.

use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bevy::prelude::{AppExit, Local, MessageReader, Res, ResMut, Resource};
use chunk_pipeline::{
    FULL_DETAIL_HYSTERESIS_CHUNKS, FarColumnSummary, FarSummaryInbox, WorldStream,
};
use client_state::RuntimeSettings;
use far_terrain::{
    ColumnPos, EncodedSummary, FarTerrainCache, FarTerrainConfig, FarTiles, NearMask,
    RetiringSession, TileView,
};
use render_api::far_terrain::{DeviceTier, far_reach_chunks, full_detail_chunks};
use render_scene::FarTerrainScene;

use crate::{runtime::world::ClientWorld, session::SessionController};

/// The far-terrain cache, the selected tiles and the view they were chosen for.
#[derive(Resource)]
pub(crate) struct FarTerrain {
    cache: FarTerrainCache,
    tiles: FarTiles,
    drained: Vec<FarColumnSummary>,
    near: Arc<NearMask>,
    /// Centre column and drawn-set revision the mask was built for.
    near_view: Option<([i32; 2], u64)>,
    /// When turning far terrain off began waiting for near terrain to cover its columns.
    drain_since: Option<Instant>,
    published: Option<u64>,
    retiring: Vec<Retiring>,
}

/// Microseconds since the Unix epoch: summary versions this run stamps exceed earlier runs'.
fn version_base() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_micros() as u64)
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
    /// A cache persisted under `disk_root`, or memory-only without one.
    pub(crate) fn new(disk_root: Option<std::path::PathBuf>) -> Self {
        Self {
            cache: FarTerrainCache::new(FarTerrainConfig::default(), disk_root, version_base()),
            tiles: FarTiles::default(),
            drained: Vec::new(),
            near: Arc::new(NearMask::default()),
            near_view: None,
            drain_since: None,
            published: None,
            retiring: Vec::new(),
        }
    }

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
        self.cache.leave_world();
        self.cache.sync();
    }
}

/// Longest far terrain stays after being turned off while near terrain catches up.
const FAR_DRAIN_LIMIT: Duration = Duration::from_secs(10);

/// Finest tiles are kept built this far either side of the drawn columns' edge, so a column
/// changing drawer already has its far tile.
const EDGE_MARGIN_BLOCKS: f32 = 4.0 * 16.0;

/// The world the cache was last pointed at, so unchanged frames skip the identity hash.
#[derive(Default)]
pub(crate) struct ActiveWorld(Option<(String, u64, i32)>);

/// Keeps extraction, the cache namespace, the loaded area and the drawn tiles current.
pub(crate) fn feed_far_terrain(
    far: Option<ResMut<FarTerrain>>,
    world: Option<ResMut<ClientWorld>>,
    session: Option<Res<SessionController>>,
    settings: Res<RuntimeSettings>,
    menu: Option<Res<client_shell::menu::MenuRuntime>>,
    scene: Option<ResMut<FarTerrainScene>>,
    mut active: Local<ActiveWorld>,
) {
    let (Some(mut far), Some(mut world), Some(mut scene)) = (far, world, scene) else {
        return;
    };
    let far = &mut *far;
    let video = settings.user_settings_update().1.video;
    let tier = menu.as_ref().map_or(DeviceTier::Mid, |menu| {
        DeviceTier::from_recommended_render_distance(
            menu.settings_snapshot()
                .0
                .render_distance_defaults()
                .recommended,
        )
    });
    let destination = session
        .as_deref()
        .and_then(SessionController::destination_identity);
    let now = Instant::now();
    // Turned off, far terrain stays until near terrain has drawn every column it covered.
    let draining = !video.far_terrain
        && far.published.is_some()
        && world
            .stream
            .as_ref()
            .is_some_and(|stream| !stream.all_loaded_near_drawn())
        && now.saturating_duration_since(*far.drain_since.get_or_insert(now)) < FAR_DRAIN_LIMIT;
    if !draining {
        far.drain_since = None;
    }
    let target = world
        .stream
        .as_ref()
        .zip(destination)
        .filter(|_| video.far_terrain || draining)
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
        active.0 = target
            .map(|(destination, level, dimension)| (destination.to_owned(), level, dimension));
    }
    far.drain_retiring(now);
    if world
        .stream
        .as_ref()
        .is_some_and(|stream| enabled && stream.has_far_summaries())
        && let Some(stream) = world.stream.as_mut()
    {
        stream.drain_far_summaries(&mut far.drained);
    }
    // Columns past the full-detail radius skip lighting and meshing; far terrain draws them.
    let full_detail = world
        .stream
        .as_ref()
        .filter(|_| video.far_terrain)
        .and_then(|stream| full_detail_radius(stream, &video, tier));
    if world
        .stream
        .as_ref()
        .is_some_and(|stream| stream.full_detail_radius() != full_detail)
        && let Some(stream) = world.stream.as_mut()
    {
        stream.set_full_detail_radius(full_detail);
    }
    let Some(stream) = world.stream.as_ref().filter(|_| enabled) else {
        far.cache.poll(now);
        if far.published.take().is_some() {
            *scene = FarTerrainScene::default();
        }
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
    let loaded = loaded_radius(stream, &video);
    let reach = far_reach_chunks(
        u32::from(video.render_distance_chunks),
        loaded.max(0) as u32,
        tier,
    ) as i32;
    // Far terrain hides under exactly the columns near terrain has drawn.
    let (drawn_revision, _) = stream.near_drawn_columns();
    let near_view = ([centre.x, centre.z], drawn_revision);
    let near_changed = far.near_view != Some(near_view);
    if near_changed {
        let drawn = stream
            .near_drawn_columns()
            .1
            .filter(|column| column.dimension == centre.dimension)
            .map(|column| ColumnPos::new(column.x, column.z));
        far.near = Arc::new(NearMask::from_columns(near_view.0, drawn));
        far.near_view = Some(near_view);
    }
    let horizon_blocks = ((loaded + reach) * 16) as f32;
    far.cache
        .set_memory_budget(summary_budget_bytes(loaded + reach));
    far.cache
        .set_focus(ColumnPos::new(centre.x, centre.z), loaded + reach);
    far.cache.poll(now);
    let changed = far.cache.store().take_changed();
    far.tiles.mark_changed(&changed);
    // Only finest tiles, which hide per column, reach the drawn columns and a margin past
    // them, so a coarser tile is replaced before near terrain takes any of its columns. Tiles
    // wholly inside the drawn disc, short of a margin kept ready for columns leaving it, are
    // left out.
    // Columns leaving full detail need their far tiles built, so the skipped disc stays
    // inside the radius.
    let near_blocks = far.near.outer_blocks() + EDGE_MARGIN_BLOCKS;
    let skip_blocks = full_detail.map_or(f32::INFINITY, |radius| (radius * 16) as f32);
    far.tiles.set_view(TileView {
        centre: [centre.x as f32 * 16.0 + 8.0, centre.z as f32 * 16.0 + 8.0],
        near_blocks,
        skip_blocks: (far.near.inner_blocks().min(skip_blocks) - EDGE_MARGIN_BLOCKS).max(0.0),
        far_blocks: (horizon_blocks - near_blocks).max(0.0),
    });
    far.tiles.release_near(&far.near);
    far.tiles.update(far.cache.store(), now);
    // Columns leaving full detail keep their meshes until their far tiles are built.
    let store = far.cache.store();
    let covered: Vec<_> = stream
        .far_handoffs()
        .filter(|column| {
            let pos = ColumnPos::new(column.x, column.z);
            store.contains(pos) && far.tiles.ready(pos)
        })
        .collect();
    let revision = far.tiles.revision();
    if near_changed || far.published != Some(revision) {
        *scene = FarTerrainScene {
            tiles: far.tiles.drawn().into(),
            near: Arc::clone(&far.near),
        };
        far.published = Some(revision);
    }
    if !covered.is_empty()
        && let Some(stream) = world.stream.as_mut()
    {
        for column in covered {
            stream.finish_far_handoff(column);
        }
    }
}

// Entities are drawn only on full-detail terrain: their render distance stays inside the
// smallest full-detail radius.
const _: () = assert!(
    render_api::far_terrain::MIN_FULL_DETAIL_CHUNKS as f32 * 16.0
        > render_model::MAX_ACTOR_RENDER_DISTANCE_BLOCKS
);

/// The radius the server sends, before it says so the radius the client keeps.
fn loaded_radius(stream: &WorldStream, video: &ui::VideoSettings) -> i32 {
    stream.server_view_radius().unwrap_or_else(|| {
        stream
            .retained_view_radius()
            .unwrap_or(i32::from(video.render_distance_chunks))
    })
}

/// The full-detail radius when it is smaller than what the server sends.
fn full_detail_radius(
    stream: &WorldStream,
    video: &ui::VideoSettings,
    tier: DeviceTier,
) -> Option<i32> {
    let loaded = loaded_radius(stream, video);
    full_detail_chunks(
        video.full_detail_distance_chunks,
        u32::from(video.render_distance_chunks),
        tier,
    )
    .map(|radius| radius as i32)
    .filter(|radius| *radius < loaded)
}

/// Compressed summaries for every column out to the horizon, at an average of 512 bytes.
fn summary_budget_bytes(horizon_chunks: i32) -> usize {
    let columns = std::f64::consts::PI * f64::from(horizon_chunks.max(0)).powi(2);
    far_terrain::config::DEFAULT_MEMORY_BUDGET_BYTES.max((columns * 512.0) as usize)
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
            FarTerrainCache::new(FarTerrainConfig::default(), Some(root.path().into()), 0);
        join(&mut reader, now);
        reader.set_focus(ColumnPos::new(300, 0), 4);
        reader.sync();
        reader.poll(now);
        let stored = reader.store().encoded(ColumnPos::new(300, 0)).cloned();
        assert_eq!(stored.map(|summary| summary.hash), Some(flat(90).hash));
    }
}
