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

/// A loaded shader pack turns far terrain off at once, without draining, and gives every
/// loaded column back to full-detail meshing; a pack still loading leaves it on.
#[test]
fn a_ready_shader_pack_turns_far_terrain_off_and_meshes_every_column() {
    use std::path::PathBuf;
    let mut app = far_terrain_app(64);
    app.world_mut()
        .insert_resource(ShaderPackStatus::Loading(PathBuf::from("Photon.zip")));
    app.update();
    let stream = |app: &App| {
        let stream = app
            .world()
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .unwrap();
        (stream.far_terrain_enabled(), stream.full_detail_radius())
    };
    assert_eq!(
        stream(&app),
        (
            true,
            Some(render_api::far_terrain::MIN_FULL_DETAIL_CHUNKS as i32)
        )
    );
    app.world_mut().resource_mut::<FarTerrain>().published = Some(1);
    app.world_mut()
        .insert_resource(ShaderPackStatus::Ready(PathBuf::from("Photon.zip")));
    app.update();
    assert_eq!(stream(&app), (false, None));
    let far = app.world().resource::<FarTerrain>();
    assert!(far.published.is_none() && far.drain_since.is_none());
    assert!(app.world().resource::<FarTerrainScene>().tiles.is_empty());
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

    let mut reader = FarTerrainCache::new(FarTerrainConfig::default(), Some(root.path().into()), 0);
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
    fn read(cache: &mut FarTerrainCache, pos: ColumnPos, now: Instant) -> Option<EncodedSummary> {
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
                FarSummaryInbox::wait_for_any(retiring.iter().map(|session| &session.inbox), now),
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

    let mut reader = FarTerrainCache::new(FarTerrainConfig::default(), Some(root.path().into()), 0);
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
