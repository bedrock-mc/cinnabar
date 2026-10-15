//! Moves column summaries from the world stream into the far-terrain cache, keeps the cache
//! on the server, world and dimension being played, and publishes the tiles to draw.

use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bevy::prelude::{AppExit, Local, MessageReader, Res, ResMut, Resource};
use chunk_pipeline::{FarColumnSummary, FarSummaryInbox, WorldStream};
use client_state::RuntimeSettings;
use far_terrain::{
    ColumnPos, EncodedSummary, FarTerrainCache, FarTerrainConfig, FarTiles, NearMask,
    RetiringSession, SummaryBudget, TileView,
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
    unloaded: Vec<::world::ChunkKey>,
    near: Arc<NearMask>,
    /// Centre column, near radius and drawn-set revision the mask was built for.
    near_view: Option<([i32; 2], i32, u64)>,
    /// When turning far terrain off began waiting for near terrain to cover its columns.
    drain_since: Option<Instant>,
    near_radius: NearRadius,
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
    columns: VecDeque<(ColumnPos, EncodedSummary)>,
}

impl FarTerrain {
    /// A cache persisted under `disk_root`, or memory-only without one.
    pub(crate) fn new(disk_root: Option<std::path::PathBuf>) -> Self {
        Self {
            cache: FarTerrainCache::new(FarTerrainConfig::default(), disk_root, version_base()),
            tiles: FarTiles::default(),
            drained: Vec::new(),
            unloaded: Vec::new(),
            near: Arc::new(NearMask::default()),
            near_view: None,
            drain_since: None,
            near_radius: NearRadius::default(),
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
            columns: VecDeque::new(),
        });
    }

    /// Collects retiring summaries and persists each session's once its inbox is empty.
    fn drain_retiring(&mut self, now: Instant, budget: &mut SummaryBudget) {
        self.drain_retiring_until(now, None, budget);
    }

    /// Like [`Self::drain_retiring`], blocking until `deadline` for summaries in flight.
    fn drain_retiring_until(
        &mut self,
        now: Instant,
        deadline: Option<Instant>,
        budget: &mut SummaryBudget,
    ) {
        self.drain_retiring_with(
            now,
            budget,
            deadline.is_some(),
            || deadline.is_some_and(|deadline| Instant::now() >= deadline),
            |retiring| {
                if let Some(deadline) = deadline {
                    FarSummaryInbox::wait_for_any(
                        retiring.iter().map(|retiring| &retiring.inbox),
                        deadline,
                    );
                }
            },
        );
    }

    /// Gives every retiring inbox a bounded turn before waiting for any inbox to change.
    /// Clock and wait callbacks keep the same coordination path deterministic in fixtures.
    fn drain_retiring_with(
        &mut self,
        now: Instant,
        budget: &mut SummaryBudget,
        waiting: bool,
        mut expired: impl FnMut() -> bool,
        mut wait: impl FnMut(&[Retiring]),
    ) {
        loop {
            let expired = expired();
            self.drain_retiring_round(now, budget, waiting, expired);
            if !waiting
                || expired
                || self
                    .retiring
                    .iter()
                    .all(|retiring| retiring.inbox.is_idle())
            {
                break;
            }
            wait(&self.retiring);
        }
    }

    /// Delivers each session's ready output before another session can wait for its permits.
    fn drain_retiring_round(
        &mut self,
        now: Instant,
        budget: &mut SummaryBudget,
        waiting: bool,
        expired: bool,
    ) {
        let mut index = 0;
        while index < self.retiring.len() {
            let retiring = &mut self.retiring[index];
            if expired {
                retiring.inbox.drain(&mut self.drained);
            } else if waiting {
                retiring
                    .inbox
                    .drain_budgeted(&mut self.drained, &mut SummaryBudget::default());
            } else {
                retiring.inbox.drain_budgeted(&mut self.drained, budget);
            }
            let dimension = retiring.session.dimension();
            for finished in self
                .drained
                .drain(..)
                .filter(|finished| finished.column.dimension == dimension)
            {
                let pos = ColumnPos::new(finished.column.x, finished.column.z);
                retiring.session.add_probe(pos, &finished.summary);
                retiring.columns.push_back((pos, finished.summary));
            }
            if retiring.inbox.is_idle() || expired {
                let mut columns = Vec::new();
                while let Some((_, summary)) = retiring.columns.front() {
                    if !budget.take(summary.bytes.len()) {
                        break;
                    }
                    columns.push(retiring.columns.pop_front().expect("checked above"));
                }
                self.cache.persist(retiring.session.clone(), columns, now);
                if retiring.columns.is_empty() {
                    self.retiring.remove(index);
                } else {
                    index += 1;
                }
            } else {
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
        self.drain_retiring_until(
            now,
            Some(now + EXIT_SUMMARY_WAIT),
            &mut SummaryBudget::new(usize::MAX, usize::MAX),
        );
        self.cache.leave_world();
        self.cache.sync();
    }
}

/// Longest far terrain stays after being turned off while near terrain catches up.
const FAR_DRAIN_LIMIT: Duration = Duration::from_secs(10);

/// The radius far terrain stays out of entirely.
#[derive(Default)]
struct NearRadius(i32);

impl NearRadius {
    /// `wanted` normally; while turning off, the radius from before, so columns that were
    /// inside it stay free of far terrain while far-only columns past it keep their cover
    /// until their meshes are drawn.
    fn update(&mut self, draining: bool, wanted: i32) -> i32 {
        if !draining {
            self.0 = wanted;
        }
        self.0
    }
}

/// Destination, level, dimension, session and terrain inputs that identify one cache visit.
type WorldIdentity<D> = (D, Option<u64>, i32, u64, [u8; 32]);

/// The world and session the cache was last pointed at, so unchanged frames skip the
/// identity hash.
#[derive(Default)]
pub(crate) struct ActiveWorld(Option<WorldIdentity<String>>);

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
    // Auto runs far terrain only past what this device meshes in full detail by default;
    // below that nothing of it runs, exactly as with Off.
    let wanted = video
        .far_terrain
        .enabled(u32::from(video.render_distance_chunks), tier);
    // A transfer or rejoin to the same address is a new session and so a fresh visit: the
    // lobby behind it may be another instance under the same identity.
    let destination = session.as_deref().and_then(|session| {
        session
            .destination_identity()
            .map(|destination| (destination, session.generation()))
    });
    let now = Instant::now();
    // Turned off, by the setting or by Auto, far terrain stays until near terrain has drawn
    // every column it covered.
    let draining = !wanted
        && far.published.is_some()
        && world
            .stream
            .as_ref()
            .is_some_and(|stream| !stream.all_loaded_near_drawn())
        && now.saturating_duration_since(*far.drain_since.get_or_insert(now)) < FAR_DRAIN_LIMIT;
    if !draining {
        far.drain_since = None;
    }
    let target: Option<WorldIdentity<&str>> = world
        .stream
        .as_ref()
        .zip(destination)
        .filter(|_| wanted || draining)
        .map(|(stream, (destination, generation))| {
            (
                destination,
                world.level_identity,
                stream.current_dimension(),
                generation,
                world.terrain_resource_identity,
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
    let current: Option<WorldIdentity<&str>> =
        active
            .0
            .as_ref()
            .map(|(destination, level, dimension, generation, resources)| {
                (
                    destination.as_str(),
                    *level,
                    *dimension,
                    *generation,
                    *resources,
                )
            });
    if current != target {
        match target {
            Some((destination, level, dimension, _, resources)) => {
                if let Some(level) = level {
                    far.cache
                        .begin_visit_with_resources(destination, level, dimension, resources);
                } else if destination.starts_with("local:") {
                    // The launcher supplies a stable local-world ID independently of StartGame.
                    far.cache
                        .begin_visit_with_resources(destination, 0, dimension, resources);
                } else {
                    far.cache.begin_uncertain_visit_with_resources(
                        destination,
                        dimension,
                        resources,
                    );
                }
            }
            None => far.cache.leave_world(),
        }
        far.tiles.clear();
        far.near_view = None;
        far.published = None;
        far.drained.clear();
        far.unloaded.clear();
        *scene = FarTerrainScene::default();
        active.0 = target.map(|(destination, level, dimension, generation, resources)| {
            (
                destination.to_owned(),
                level,
                dimension,
                generation,
                resources,
            )
        });
    }
    let mut summary_budget = SummaryBudget::default();
    far.drain_retiring(now, &mut summary_budget);
    if world
        .stream
        .as_ref()
        .is_some_and(|stream| enabled && stream.has_far_summaries())
        && let Some(stream) = world.stream.as_mut()
    {
        stream.drain_far_summaries_budgeted(&mut far.drained, &mut summary_budget);
    }
    if world
        .stream
        .as_ref()
        .is_some_and(|stream| enabled && stream.has_far_unloaded())
        && let Some(stream) = world.stream.as_mut()
    {
        stream.drain_far_unloaded(&mut far.unloaded);
    }
    // Columns past the full-detail radius skip lighting and meshing; far terrain draws them.
    let full_detail = world
        .stream
        .as_ref()
        .filter(|_| wanted)
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
        far.cache.poll_budgeted(now, &mut summary_budget);
        if far.published.take().is_some() {
            *scene = FarTerrainScene::default();
        }
        return;
    };
    let dimension = stream.current_dimension();
    for FarColumnSummary {
        column,
        summary,
        resident,
        ..
    } in far.drained.drain(..)
    {
        let pos = ColumnPos::new(column.x, column.z);
        // Loaded columns may have no other drawer, so their summaries outlive the budget.
        if column.dimension == dimension {
            far.cache.store().set_pinned(pos, resident);
        }
        far.cache.admit(column.dimension, pos, summary, now);
    }
    for column in far.unloaded.drain(..) {
        if column.dimension == dimension {
            far.cache
                .store()
                .set_pinned(ColumnPos::new(column.x, column.z), false);
        }
    }
    let centre = stream.far_terrain_centre();
    let loaded = loaded_radius(stream, &video);
    let reach = far_reach_chunks(
        u32::from(video.render_distance_chunks),
        loaded.max(0) as u32,
        tier,
    ) as i32;
    // Near terrain owns everything within the full-detail radius, or all it is sent without
    // one, even before its meshes load; past that, delivered air or published near geometry
    // hides far terrain, including columns still handing over to it.
    let near_radius = far
        .near_radius
        .update(draining, full_detail.unwrap_or(loaded));
    let (drawn_revision, _) = stream.near_drawn_columns();
    let near_view = ([centre.x, centre.z], near_radius, drawn_revision);
    let near_changed = far.near_view != Some(near_view);
    if near_changed {
        let drawn = stream
            .near_geometry_columns()
            .filter(|column| column.dimension == centre.dimension)
            .map(|column| ColumnPos::new(column.x, column.z));
        let empty = stream
            .near_empty_columns()
            .filter(|column| column.dimension == centre.dimension)
            .map(|column| ColumnPos::new(column.x, column.z));
        far.near = Arc::new(NearMask::around_with_empty(
            near_view.0,
            loaded + reach,
            near_radius,
            drawn,
            empty,
        ));
        far.near_view = Some(near_view);
    }
    far.cache
        .set_memory_budget(summary_budget_bytes(loaded + reach));
    far.cache
        .set_focus(ColumnPos::new(centre.x, centre.z), loaded + reach);
    far.cache.poll_budgeted(now, &mut summary_budget);
    let changed = far.cache.store().take_changed();
    far.tiles.mark_changed(&changed);
    far.tiles.set_view(TileView::around(
        [centre.x, centre.z],
        &far.near,
        near_radius,
        loaded + reach,
    ));
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
#[path = "far_terrain/resource_tests.rs"]
mod resource_tests;

#[cfg(test)]
mod tests {
    use bevy::prelude::{App, Last};
    use far_terrain::{ColumnSummary, SurfaceCell, config::WORLD_PROBE_COLUMNS};

    use super::*;

    const SERVER: &str = "remote:play.example.net:19132";

    /// Encodes a uniformly colored, original surface summary at the requested height.
    pub(super) fn flat(height: i16) -> EncodedSummary {
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

    /// Builds an empty session with far terrain on and the minimum full-detail radius.
    pub(super) fn far_terrain_app(render_distance: u8) -> App {
        let mut settings = RuntimeSettings::default();
        let mut user = settings.user_settings_update().1.clone();
        user.video.far_terrain = render_api::far_terrain::FarTerrainMode::On;
        user.video.render_distance_chunks = render_distance;
        user.video.full_detail_distance_chunks = render_api::far_terrain::MIN_FULL_DETAIL_CHUNKS;
        settings.replace_user_settings(user);
        let world = ClientWorld {
            level_identity: Some(42),
            stream: Some(WorldStream::new(protocol::WorldBootstrap {
                local_player_unique_id: 1,
                local_player_runtime_id: 1,
                dimension: 0,
                player_position: [0.0; 3],
                world_spawn_position: [0; 3],
                air_network_id: 0,
                block_network_ids_are_hashes: false,
            })),
            ..ClientWorld::default()
        };
        let mut app = App::new();
        app.insert_resource(settings)
            .insert_resource(world)
            .insert_resource(SessionController::default().with_address(Some("example.net:19132")))
            .insert_resource(FarTerrain::new(None))
            .init_resource::<FarTerrainScene>()
            .add_systems(bevy::prelude::Update, feed_far_terrain);
        app
    }

    /// A new world rebuilds coverage even when its position, radius and revision coincide.
    #[test]
    fn world_switch_drops_the_previous_visibility_mask() {
        let mut app = far_terrain_app(64);
        app.update();
        let previous = ColumnPos::new(30, 0);
        {
            let mut far = app.world_mut().resource_mut::<FarTerrain>();
            far.near = Arc::new(NearMask::around(
                [0, 0],
                64,
                render_api::far_terrain::MIN_FULL_DETAIL_CHUNKS as i32,
                [previous],
            ));
            assert!(far.near.contains(previous));
        }
        app.world_mut().resource_mut::<ClientWorld>().level_identity = Some(43);
        app.update();
        assert!(
            !app.world()
                .resource::<FarTerrainScene>()
                .near
                .contains(previous)
        );
    }

    /// Distant delivered air hides old terrain without refining unrelated far tiles.
    #[test]
    fn distant_air_does_not_refine_the_far_ring() {
        let horizon = 160;
        let near_radius = render_api::far_terrain::MIN_FULL_DETAIL_CHUNKS as i32;
        let mut app = far_terrain_app(horizon as u8);
        app.world_mut()
            .resource_mut::<ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .submit(1, protocol::WorldEvent::ChunkRadiusUpdated(horizon))
            .unwrap();
        app.update();
        let before = TileView::around(
            [0, 0],
            &app.world().resource::<FarTerrainScene>().near,
            near_radius,
            horizon,
        );
        let empty = ColumnPos::new(128, 0);
        let mut payload = vec![1, 0];
        let sections = protocol::vanilla_dimension_range(0)
            .unwrap()
            .sub_chunk_count;
        payload.extend(std::iter::repeat_n(0xff, sections - 1));
        payload.push(0);
        app.world_mut()
            .resource_mut::<ClientWorld>()
            .stream
            .as_mut()
            .unwrap()
            .submit(
                2,
                protocol::WorldEvent::LevelChunk(protocol::LevelChunkEvent {
                    dimension: 0,
                    x: empty.x,
                    z: empty.z,
                    mode: protocol::LevelChunkMode::Inline { count: 0 },
                    payload,
                }),
            )
            .unwrap();
        test_time::eventually("distant air column committed", || {
            let mut world = app.world_mut().resource_mut::<ClientWorld>();
            let stream = world.stream.as_mut().unwrap();
            stream.poll([0.0; 3], 0);
            stream
                .near_drawn_columns()
                .1
                .any(|column| column.x == empty.x && column.z == empty.z)
        });
        app.update();
        let scene = app.world().resource::<FarTerrainScene>();
        assert!(scene.near.contains(empty));
        let after = TileView::around([0, 0], &scene.near, near_radius, horizon);
        assert_eq!(
            far_terrain::select_tiles(after).len(),
            far_terrain::select_tiles(before).len(),
            "air coverage must not increase the tile count"
        );
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
            columns: VecDeque::from([(ColumnPos::new(300, 0), flat(90))]),
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

    /// Retirement persistence uses the same column limit and carries the remainder forward.
    #[test]
    fn retiring_admission_is_bounded() {
        use far_terrain::config::SUMMARY_COLUMNS_PER_UPDATE;
        let now = Instant::now();
        let mut far = FarTerrain::new(None);
        join(&mut far.cache, now);
        let session = far.cache.retire_session().unwrap();
        let columns = (0..SUMMARY_COLUMNS_PER_UPDATE + 1)
            .map(|x| (ColumnPos::new(1000 + x as i32, 0), flat(90)))
            .collect();
        far.retiring.push(Retiring {
            session,
            inbox: FarSummaryInbox::default(),
            columns,
        });
        let before = far.cache.store().len();
        far.drain_retiring(now, &mut SummaryBudget::default());
        assert_eq!(far.cache.store().len() - before, SUMMARY_COLUMNS_PER_UPDATE);
        assert_eq!(far.retiring[0].columns.len(), 1);
        far.drain_retiring(now, &mut SummaryBudget::default());
        assert!(far.retiring.is_empty());
        assert_eq!(
            far.cache.store().len() - before,
            SUMMARY_COLUMNS_PER_UPDATE + 1
        );
    }

    /// Retiring sessions share capacity without blocking each other's final namespace writes.
    #[test]
    fn shutdown_progresses_all_retiring_inboxes_before_waiting() {
        use chunk_pipeline::FarSummaryFixture;
        use far_terrain::ColumnSnapshot;

        /// Resolves the namespace for one server and dimension using stable fixture probes.
        fn join_world(cache: &mut FarTerrainCache, server: &str, dimension: i32, now: Instant) {
            cache.enter_world(server, 42, dimension);
            for x in 0..WORLD_PROBE_COLUMNS as i32 {
                cache.admit(dimension, ColumnPos::new(x, 0), flat(64), now);
            }
            cache.sync();
            cache.poll(now);
            assert!(cache.namespace().is_some());
        }

        /// Reads a persisted result from the selected namespace after the IO owner settles.
        fn read(
            cache: &mut FarTerrainCache,
            pos: ColumnPos,
            now: Instant,
        ) -> Option<EncodedSummary> {
            cache.set_focus(pos, 4);
            cache.sync();
            cache.poll(now);
            cache.store().encoded(pos).cloned()
        }

        let root = tempfile::tempdir().unwrap();
        let now = Instant::now();
        let mut far = FarTerrain::new(Some(root.path().to_owned()));
        let fixture = FarSummaryFixture::new(2);
        let pos = ColumnPos::new(900, 0);
        let sibling = ColumnPos::new(901, 0);
        join_world(&mut far.cache, SERVER, 0, now);
        let first = far.cache.retire_session().unwrap();
        let mut denied = fixture.inbox();
        fixture.enqueue(
            &mut denied,
            ::world::ChunkKey::new(0, pos.x, pos.z),
            ColumnSnapshot::new(Arc::default(), Vec::new()),
        );

        const OTHER_SERVER: &str = "remote:other.example.net:19132";
        join_world(&mut far.cache, OTHER_SERVER, -1, now);
        let second = far.cache.retire_session().unwrap();
        let mut ready = fixture.inbox();
        fixture.ready(
            &mut ready,
            ::world::ChunkKey::new(-1, pos.x, pos.z),
            flat(70),
        );
        fixture.ready(
            &mut ready,
            ::world::ChunkKey::new(-1, sibling.x, sibling.z),
            flat(71),
        );
        let (usage, limits) = fixture.admission();
        assert_eq!(
            usage.0, limits.0,
            "the later inbox owns every admitted slot"
        );
        far.cache.admit(-1, pos, flat(90), now);
        far.retiring.extend([
            Retiring {
                session: first,
                inbox: denied,
                columns: VecDeque::new(),
            },
            Retiring {
                session: second,
                inbox: ready,
                columns: VecDeque::new(),
            },
        ]);

        let mut waits = 0;
        far.drain_retiring_with(
            now,
            &mut SummaryBudget::new(usize::MAX, usize::MAX),
            true,
            || false,
            |retiring| {
                waits += 1;
                assert!(waits < 16, "finite retirement must keep progressing");
                let (usage, limits) = fixture.admission();
                assert!(usage.0 <= limits.0 && usage.1 <= limits.1);
                fixture.run_one();
                assert!(
                    FarSummaryInbox::wait_for_any(
                        retiring.iter().map(|session| &session.inbox),
                        now
                    ),
                    "delivery releases capacity or a manual job publishes the next event"
                );
            },
        );
        assert!(far.retiring.is_empty());
        assert_eq!(fixture.admission().0, (0, 0));
        assert_eq!(
            far.cache.store().encoded(pos).unwrap().hash,
            flat(90).hash,
            "older retired output cannot replace a newer active-session write"
        );
        far.cache.leave_world();
        far.cache.sync();

        let mut reader =
            FarTerrainCache::new(FarTerrainConfig::default(), Some(root.path().into()), 0);
        join_world(&mut reader, SERVER, 0, now);
        assert_eq!(
            read(&mut reader, pos, now).unwrap().hash,
            ColumnSummary::default().encode().hash
        );
        join_world(&mut reader, OTHER_SERVER, -1, now);
        assert_eq!(read(&mut reader, pos, now).unwrap().hash, flat(90).hash);
        assert_eq!(read(&mut reader, sibling, now).unwrap().hash, flat(71).hash);
    }

    /// Expiration ends the shared wait once, persisting collected output without waiting on held work.
    #[test]
    fn shutdown_deadline_keeps_collected_output_and_stops_waiting() {
        use chunk_pipeline::FarSummaryFixture;
        use far_terrain::ColumnSnapshot;

        let now = Instant::now();
        let mut far = FarTerrain::new(None);
        join(&mut far.cache, now);
        let session = far.cache.retire_session().unwrap();
        let fixture = FarSummaryFixture::new(1);
        let mut inbox = fixture.inbox();
        fixture.enqueue(
            &mut inbox,
            ::world::ChunkKey::new(0, 900, 0),
            ColumnSnapshot::new(Arc::default(), Vec::new()),
        );
        let pos = ColumnPos::new(800, 0);
        far.retiring.push(Retiring {
            session,
            inbox,
            columns: VecDeque::from([(pos, flat(90))]),
        });
        let mut checks = 0;
        let mut waits = 0;
        far.drain_retiring_with(
            now,
            &mut SummaryBudget::new(usize::MAX, usize::MAX),
            true,
            || {
                checks += 1;
                checks > 1
            },
            |_| {
                waits += 1;
            },
        );
        assert_eq!((checks, waits), (2, 1));
        assert!(far.retiring.is_empty());
        assert_eq!(far.cache.store().encoded(pos).unwrap().hash, flat(90).hash);
        assert_eq!(
            fixture.admission().0.0,
            1,
            "unfinished work keeps its permit"
        );
        assert!(fixture.run_one());
        assert_eq!(fixture.admission().0, (0, 0));
    }

    /// Turning far terrain off mid warm-join keeps the old full-detail area free of far
    /// terrain on every frame, while near meshes keep arriving ring by ring.
    #[test]
    fn disabling_mid_join_keeps_far_terrain_out_of_the_old_radius() {
        use far_terrain::SummaryStore;
        let mut store = SummaryStore::new(usize::MAX);
        for x in -40..40 {
            for z in -40..40 {
                let mut summary = ColumnSummary::default();
                summary.cells_mut().fill(SurfaceCell {
                    height: 64,
                    rgb: [90, 140, 60],
                    water_depth: 0,
                });
                store.admit(ColumnPos::new(x, z), &summary);
            }
        }
        let (centre, full_detail, loaded, horizon) = ([0, 0], 8, 16, 32);
        let mut radius = NearRadius::default();
        let mut tiles = FarTiles::default();
        let mut drawn = Vec::new();
        let now = Instant::now();
        for frame in 0..40 {
            // Far terrain is turned off after a few frames, before the near area has meshed.
            let draining = frame >= 6;
            let near_radius = radius.update(draining, if draining { loaded } else { full_detail });
            if frame % 4 == 3 {
                let ring = frame / 4;
                drawn.extend(
                    (-ring..=ring)
                        .flat_map(|x| (-ring..=ring).map(move |z| ColumnPos::new(x, z)))
                        .filter(|column| column.x * column.x + column.z * column.z <= ring * ring),
                );
            }
            let mask = NearMask::around(centre, horizon, near_radius, drawn.iter().copied());
            tiles.set_view(TileView::around(centre, &mask, near_radius, horizon));
            test_time::eventually("this frame's far tiles build", || {
                tiles.update(&mut store, now);
                tiles.ready(ColumnPos::new(20, 0))
            });
            for mesh in tiles.drawn() {
                for column in mesh.key.columns() {
                    if column.x * column.x + column.z * column.z <= full_detail * full_detail {
                        assert!(
                            mask.contains(column),
                            "far terrain over {column:?} at frame {frame}"
                        );
                        assert_eq!(
                            mesh.key.level, 0,
                            "coarse tile near the player at frame {frame}"
                        );
                    }
                }
            }
        }
    }
}
