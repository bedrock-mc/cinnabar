//! Moves column summaries from the world stream into the far-terrain cache, keeps the cache
//! on the server, world and dimension being played, and publishes the tiles to draw.

use std::{sync::Arc, time::Instant};

use bevy::prelude::{Local, Res, ResMut, Resource};
use chunk_pipeline::{FarColumnSummary, FarSummaryInbox, WorldStream};
use client_state::RuntimeSettings;
use far_terrain::{
    ColumnPos, EncodedSummary, FarTerrainCache, FarTerrainConfig, FarTiles, NamespaceKey, NearMask,
    TileView,
    config::{DeviceTier, far_reach_chunks},
};
use render_scene::FarTerrainScene;

use crate::{runtime::world::ClientWorld, session::SessionController};

/// The far-terrain cache, the selected tiles and the view they were chosen for.
#[derive(Resource)]
pub(crate) struct FarTerrain {
    cache: FarTerrainCache,
    tiles: FarTiles,
    drained: Vec<FarColumnSummary>,
    near: Arc<NearMask>,
    near_view: Option<([i32; 2], i32)>,
    published: Option<u64>,
    retiring: Vec<Retiring>,
}

/// A retired session's final summaries, written to its namespace once all have arrived.
struct Retiring {
    namespace: NamespaceKey,
    inbox: FarSummaryInbox,
    columns: Vec<(ColumnPos, EncodedSummary)>,
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
            retiring: Vec::new(),
        }
    }

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
}

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
    // Far terrain fills from what the server sends out to the render distance.
    let loaded = stream.server_view_radius().unwrap_or_else(|| {
        stream
            .retained_view_radius()
            .unwrap_or(i32::from(video.render_distance_chunks))
    });
    let reach = far_reach_chunks(
        u32::from(video.render_distance_chunks),
        loaded.max(0) as u32,
        tier,
    ) as i32;
    let near_chunks = loaded + ::world::CHUNK_VIEW_SLACK + 1;
    let horizon_chunks = loaded + reach;
    far.cache
        .set_focus(ColumnPos::new(centre.x, centre.z), horizon_chunks);
    far.cache.poll(now);
    let changed = far.cache.store().take_changed();
    far.tiles.mark_changed(&changed);
    far.tiles.set_view(TileView {
        centre: [centre.x as f32 * 16.0 + 8.0, centre.z as f32 * 16.0 + 8.0],
        near_blocks: (near_chunks * 16) as f32,
        far_blocks: ((horizon_chunks - near_chunks).max(0) * 16) as f32,
    });
    far.tiles.update(far.cache.store(), now);
    let near_view = ([centre.x, centre.z], loaded);
    let near_changed = far.near_view != Some(near_view);
    if near_changed {
        far.near = Arc::new(NearMask::retained(near_view.0, loaded));
        far.near_view = Some(near_view);
    }
    let revision = far.tiles.revision();
    if near_changed || far.published != Some(revision) {
        *scene = FarTerrainScene {
            tiles: far.tiles.drawn().into(),
            near: Arc::clone(&far.near),
        };
        far.published = Some(revision);
    }
}
