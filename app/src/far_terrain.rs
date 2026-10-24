//! Moves column summaries from the world stream into the far-terrain cache, keeps the cache
//! on the server, world and dimension being played, and publishes the tiles to draw.

use std::{
    collections::VecDeque,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use bevy::prelude::{AppExit, DetectChangesMut, Local, MessageReader, Res, ResMut, Resource};
use chunk_pipeline::{FarColumnSummary, FarSummaryInbox, TerrainPublicationCursor, WorldStream};
use client_state::RuntimeSettings;
use far_terrain::{
    ColumnPos, EncodedSummary, FarTerrainCache, FarTerrainConfig, FarTiles, NearMask,
    RetiringSession, SummaryBudget, TileView,
};
use render_api::far_terrain::{DeviceTier, far_reach_chunks, full_detail_chunks};
use render_scene::{FarTerrainScene, shaderpack::ShaderPackStatus};

use crate::{runtime::world::ClientWorld, session::SessionController};

#[cfg(test)]
#[path = "far_terrain/publication_tests.rs"]
mod publication_tests;

/// Revisions and view inputs that determine one shared terrain publication target.
type PublicationKey = ((u64, u64, u64, u64, usize, Option<i32>), u64, [i32; 2], i32);

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
    /// Stream requirements, tile revision and viewport used by the render publication target.
    publication: Option<PublicationKey>,
    /// Session/dimension identity and residency indexes used by near publication receipts.
    source_revision: Option<((u64, u64), u64, u64)>,
    /// Where the receipts' section membership was last reconciled with the stream.
    source_cursor: Option<TerrainPublicationCursor>,
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
            publication: None,
            source_revision: None,
            source_cursor: None,
            retiring: Vec::new(),
        }
    }

    /// Rebuilds the CPU selection mask when its radius or acknowledged near columns change.
    fn update_near(
        &mut self,
        stream: &WorldStream,
        centre: ::world::ChunkKey,
        radius: i32,
        horizon: i32,
    ) -> bool {
        let (revision, _) = stream.near_drawn_columns();
        let view = ([centre.x, centre.z], radius, revision);
        if self.near_view == Some(view) {
            return false;
        }
        self.near = Arc::new(NearMask::around_with_empty(
            view.0,
            horizon,
            radius,
            stream
                .near_geometry_columns()
                .filter(|column| column.dimension == centre.dimension)
                .map(|column| ColumnPos::new(column.x, column.z)),
            stream
                .near_empty_columns()
                .filter(|column| column.dimension == centre.dimension)
                .map(|column| ColumnPos::new(column.x, column.z)),
        ));
        self.near_view = Some(view);
        true
    }

    /// Keeps near receipts bounded and scoped to the stream even while far terrain is disabled.
    /// Membership follows the stream's replayed changes, or every section when their history no
    /// longer reaches back. Returns whether `scene` was replaced, which extraction must see.
    fn publish_source(
        &mut self,
        stream: Option<&WorldStream>,
        scene: &mut FarTerrainScene,
    ) -> bool {
        let revision = stream.map(WorldStream::terrain_publication_source);
        if self.source_revision == revision {
            return false;
        }
        let replaced = self.source_revision.map(|stamp| stamp.0) != revision.map(|stamp| stamp.0);
        if replaced {
            scene.publications = stream.map(|_| Default::default());
            self.publication = None;
            self.source_cursor = None;
        }
        if let (Some(stream), Some(publications)) = (stream, &scene.publications) {
            let cursor = stream.terrain_publication_cursor();
            match self
                .source_cursor
                .and_then(|since| stream.terrain_publication_changes(since))
            {
                Some(changes) => publications.apply_membership(changes),
                None => publications.retain(stream.terrain_publication_sections()),
            }
            self.source_cursor = Some(cursor);
        }
        self.source_revision = revision;
        replaced
    }

    /// Shares current section requirements without rebuilding an unchanged publication target.
    /// Returns whether `scene` changed, which extraction must see.
    fn publish_target(
        &mut self,
        stream: &WorldStream,
        horizon: i32,
        scene: &mut FarTerrainScene,
    ) -> bool {
        let replaced = self.publish_source(Some(stream), scene);
        let revision = stream.terrain_publication_revision();
        let centre = stream.far_terrain_centre();
        let key = (
            revision,
            self.tiles.revision(),
            [centre.x, centre.z],
            horizon,
        );
        if self.publication == Some(key) && scene.target.is_some() {
            return replaced;
        }
        let mut target = self
            .publication
            .filter(|previous| previous.0 == revision && previous.2 == [centre.x, centre.z])
            .and(scene.target.as_deref())
            .cloned()
            .unwrap_or_else(|| stream.terrain_publication_target(horizon));
        target.centre = centre;
        target.horizon = horizon;
        target.empty_far = Arc::new(self.tiles.empty_columns().collect());
        scene.target = Some(Arc::new(target));
        self.publication = Some(key);
        true
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

/// Destination, level, dimension, stream/transition generation and terrain resource identity.
type WorldIdentity<D> = (D, Option<u64>, i32, (u64, u64), [u8; 32]);

/// The world and session the cache was last pointed at, so unchanged frames skip the
/// identity hash.
#[derive(Default)]
pub(crate) struct ActiveWorld(Option<WorldIdentity<String>>);

/// Keeps extraction, the cache namespace, the loaded area and the drawn tiles current.
#[allow(clippy::too_many_arguments)]
pub(crate) fn feed_far_terrain(
    far: Option<ResMut<FarTerrain>>,
    world: Option<ResMut<ClientWorld>>,
    session: Option<Res<SessionController>>,
    settings: Res<RuntimeSettings>,
    menu: Option<Res<client_shell::menu::MenuRuntime>>,
    scene: Option<ResMut<FarTerrainScene>>,
    pack: Option<Res<ShaderPackStatus>>,
    mut active: Local<ActiveWorld>,
) {
    let (Some(mut far), Some(mut world), Some(mut scene)) = (far, world, scene) else {
        return;
    };
    let far = &mut *far;
    let video = &settings.user_settings_update().1.video;
    let tier = menu.as_ref().map_or(DeviceTier::Mid, |menu| {
        DeviceTier::from_recommended_render_distance(
            menu.settings_snapshot()
                .0
                .render_distance_defaults()
                .recommended,
        )
    });
    // Auto runs far terrain only past what this device meshes in full detail by default;
    // below that nothing of it runs, exactly as with Off. A loaded shader pack draws the
    // world as Java does, without far terrain (its frames show sky past render distance), so
    // none of it runs then either and near terrain meshes every loaded column.
    let shader_pack = pack.as_deref().is_some_and(ShaderPackStatus::is_ready);
    let wanted = !shader_pack
        && video
            .far_terrain
            .enabled(u32::from(video.render_distance_chunks), tier);
    // A transfer or rejoin to the same address is a new session and so a fresh visit: the
    // lobby behind it may be another instance under the same identity.
    let destination = session
        .as_deref()
        .and_then(SessionController::destination_identity);
    let now = Instant::now();
    // Turned off, by the setting or by Auto, far terrain stays until near terrain has drawn
    // every column it covered; a shader pack never shows it, so nothing waits for it then.
    let draining = !wanted
        && !shader_pack
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
        .map(|(stream, destination)| {
            (
                destination,
                world.level_identity,
                stream.current_dimension(),
                stream.terrain_generation(),
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
        far.publication = None;
        far.drained.clear();
        far.unloaded.clear();
        *scene = FarTerrainScene {
            publications: scene.publications.clone(),
            ..Default::default()
        };
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
        .and_then(|stream| full_detail_radius(stream, video, tier));
    if world
        .stream
        .as_ref()
        .is_some_and(|stream| stream.full_detail_radius() != full_detail)
        && let Some(stream) = world.stream.as_mut()
    {
        stream.set_full_detail_radius(full_detail);
    }
    let Some(stream) = world.stream.as_ref().filter(|_| enabled) else {
        // Borrowed mutably only on a change, so extraction does not copy the scene every frame.
        if far.publish_source(world.stream.as_ref(), scene.bypass_change_detection()) {
            scene.set_changed();
        }
        far.cache.poll_budgeted(now, &mut summary_budget);
        if far.published.take().is_some() {
            *scene = FarTerrainScene {
                publications: scene.publications.clone(),
                ..Default::default()
            };
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
    let loaded = loaded_radius(stream, video);
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
    let near_changed = far.update_near(stream, centre, near_radius, loaded + reach);
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
    let revision = far.tiles.revision();
    if near_changed || far.published != Some(revision) {
        *scene = FarTerrainScene {
            publications: scene.publications.clone(),
            tiles: far.tiles.drawn().into(),
            near: Arc::clone(&far.near),
            residency: scene.residency.clone(),
            target: scene.target.clone(),
        };
        far.published = Some(revision);
    }
    // A CPU-ready tile may still be waiting for its upload or a covering GPU fallback.
    let store = far.cache.store();
    let covered: Vec<_> = stream
        .far_handoffs()
        .filter(|column| {
            let pos = ColumnPos::new(column.x, column.z);
            store.contains(pos)
                && far
                    .tiles
                    .ready_mesh(pos)
                    .is_some_and(|mesh| scene.residency.ready(pos, mesh))
        })
        .collect();
    if !covered.is_empty()
        && let Some(stream) = world.stream.as_mut()
    {
        for column in covered {
            stream.finish_far_handoff(column);
        }
        // This CPU mask selects tiles; render publication owns the effective draw mask.
        if far.update_near(stream, centre, near_radius, loaded + reach) {
            scene.near = Arc::clone(&far.near);
        }
    }
    if let Some(stream) = world.stream.as_ref()
        && far.publish_target(stream, loaded + reach, scene.bypass_change_detection())
    {
        scene.set_changed();
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
#[path = "far_terrain/transfer_tests.rs"]
mod transfer_tests;

#[cfg(test)]
#[path = "far_terrain/tests.rs"]
mod tests;
