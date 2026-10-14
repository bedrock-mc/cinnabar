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
