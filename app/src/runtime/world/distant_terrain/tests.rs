use bevy::prelude::App;
use chunk_pipeline::WorldStream;
use client_runtime::distant_terrain::{DistantTerrainReport, StoreState};
use lod_store::{identity::StoreKey, root::StoreRoot};
use protocol::{StartupWorldIdentity, WorldBootstrap};
use test_time::eventually;

use super::*;

/// An app with a session at `example.test` whose distant terrain setting is `enabled`.
fn app(lod_cache_dir: &Path, enabled: bool) -> App {
    let mut app = App::new();
    let mut settings = client_state::RuntimeSettings::default();
    let mut user = settings.user_settings_update().1.clone();
    user.video.distant_terrain = enabled;
    settings.replace_user_settings(user);
    let stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 0,
        block_network_ids_are_hashes: false,
    });
    app.insert_resource(settings)
        .insert_resource(SessionController::default().with_address(Some("example.test:19132")))
        .init_resource::<LocalViewPose>()
        .init_resource::<Time<Real>>()
        .insert_resource(ClientWorld {
            stream: Some(stream),
            world_identity: Some((
                1,
                StartupWorldIdentity {
                    level_id: "level".into(),
                    seed: 7,
                },
            )),
            ..Default::default()
        });
    configure_distant_terrain(&mut app, lod_cache_dir);
    app
}

fn state(app: &mut App) -> StoreState {
    app.world_mut()
        .resource_mut::<DistantTerrainResource>()
        .0
        .get_mut()
        .unwrap_or_else(PoisonError::into_inner)
        .report()
        .state
}

fn ingesting(app: &App) -> bool {
    app.world()
        .resource::<ClientWorld>()
        .stream
        .as_ref()
        .is_some_and(WorldStream::distant_terrain_enabled)
}

#[test]
fn the_video_setting_and_session_identity_open_the_store_and_leaving_closes_it() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut app = app(dir.path(), true);
    eventually("the session's store to open", || {
        app.update();
        state(&mut app) == StoreState::Open { read_only: false }
    });
    assert!(ingesting(&app));
    let store = StoreRoot::new(dir.path()).store_dir(&StoreKey::new(
        "remote:example.test:19132",
        "level",
        7,
    ));
    assert!(store.is_dir(), "keyed by destination, level id and seed");

    // Leaving the session forgets its identity, which closes the store.
    app.world_mut().resource_mut::<ClientWorld>().world_identity = None;
    app.update();
    assert_eq!(state(&mut app), StoreState::Off);
    assert!(!ingesting(&app));
}

#[test]
fn the_video_setting_meshes_selects_and_hands_settings_to_the_renderer() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut app = app(dir.path(), true);
    eventually("selection to run on the open store", || {
        app.update();
        app.world()
            .resource::<ClientWorld>()
            .stream
            .as_ref()
            .is_some_and(|stream| stream.distant_terrain_selection_stats().is_some())
    });
    let link = app
        .world()
        .resource::<render_scene::distant_terrain::DistantTerrainLink>()
        .clone();
    let inbox = link.take();
    let settings = inbox.settings.expect("drawing is on");
    assert_eq!(
        settings.distance_chunks,
        render_api::distant_terrain::DEFAULT_DISTANT_TERRAIN_CHUNKS
    );
    assert!(
        inbox.generation > 0,
        "the renderer starts from nothing drawn"
    );
    let fog = |app: &App| app.world().resource::<draw::DistantTerrainFogDistance>().0;
    assert_eq!(fog(&app), None, "no fog change before the renderer draws");
    link.publish(render_scene::distant_terrain::DistantTerrainRenderStats {
        ready: true,
        ..Default::default()
    });
    app.update();
    assert_eq!(
        fog(&app),
        None,
        "a ready renderer drawing nothing changes no fog"
    );
    link.publish(render_scene::distant_terrain::DistantTerrainRenderStats {
        ready: true,
        sections: 1,
        ..Default::default()
    });
    app.update();
    assert_eq!(
        fog(&app),
        Some((settings.distance_chunks * world::SUB_CHUNK_SIDE as u32) as f32),
        "air fog follows the drawn distance"
    );

    // Turning the setting off stops drawing and restores the chunk fog.
    let mut settings = app
        .world_mut()
        .resource_mut::<client_state::RuntimeSettings>();
    let mut user = settings.user_settings_update().1.clone();
    user.video.distant_terrain = false;
    settings.replace_user_settings(user);
    app.update();
    assert_eq!(link.take().settings, None);
    assert_eq!(
        app.world().resource::<draw::DistantTerrainFogDistance>().0,
        None
    );
}

#[test]
fn the_store_stays_closed_while_the_setting_is_off() {
    let dir = tempfile::tempdir().expect("temp dir");
    let mut app = app(dir.path(), false);
    app.update();
    app.update();
    assert_eq!(state(&mut app), StoreState::Off);
    assert!(!ingesting(&app));
    assert!(
        !StoreRoot::new(dir.path()).dir().exists(),
        "nothing is written"
    );
}

#[test]
fn counters_are_formatted_only_when_they_changed_and_the_interval_passed() {
    let start = Instant::now();
    let mut reported = ReportedCounters::default();
    let off = DistantTerrainReport::default();
    let unread = || -> Instant { panic!("an unchanged report never reads the clock") };
    assert_eq!(report_counters(off, &mut reported, unread), None);

    let open = DistantTerrainReport {
        state: StoreState::Open { read_only: false },
        ..off
    };
    assert!(report_counters(open, &mut reported, || start).is_some());
    assert_eq!(report_counters(open, &mut reported, unread), None);

    // Turned off with cumulative counters left: logged once, then quiet.
    let turned_off = DistantTerrainReport {
        epoch_changes: 2,
        ..off
    };
    assert_eq!(
        report_counters(turned_off, &mut reported, || start + REPORT_INTERVAL / 2),
        None,
        "within the interval"
    );
    assert!(report_counters(turned_off, &mut reported, || start + REPORT_INTERVAL).is_some());
    for _ in 0..3 {
        assert_eq!(report_counters(turned_off, &mut reported, unread), None);
    }
}
