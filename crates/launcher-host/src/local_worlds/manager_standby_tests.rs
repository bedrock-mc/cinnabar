//! Prestarted and kept-alive servers stay hidden until an open of their world adopts them.

use std::sync::atomic::Ordering;

use super::*;
use crate::local_worlds::test_support::{FAKE_BDS, installed_provisioner};

const HOST: &str = "2535400000000001";

/// A manager with an installed server whose BDS worlds `runner` serves, keeping left servers for
/// `standby`.
fn standby_fixture(runner: &Arc<FakeRunner>, standby: Option<Duration>) -> Fixture {
    let runner: Arc<dyn Runner> = runner.clone();
    fixture(|parts, dir| {
        parts.runners.bds = Some(runner);
        parts.setup = Some(Arc::new(installed_provisioner(dir.path(), LINUX, FAKE_BDS)));
        parts.standby = standby;
    })
}

/// A saved BDS world named `name`.
fn bds(manager: &Manager, name: &str) -> World {
    manager
        .create(&NewWorld {
            backend: Some(Backend::Bds),
            ..flat(name)
        })
        .expect("create")
}

/// Records `world` as just played, so it becomes the most recently played.
fn played(manager: &Manager, world: &World) {
    manager
        .inner
        .store
        .touch(&world.id, &Default::default())
        .expect("record play");
}

/// The lifecycle state, including a hidden server's.
fn life_state(manager: &Manager) -> WorldState {
    lock(&manager.inner.life).state
}

#[track_caller]
fn assert_hidden(manager: &Manager) {
    let status = manager.status();
    assert_eq!(
        (status.state, status.world_id),
        (WorldState::Idle, None),
        "a hidden server shows no open world"
    );
    assert_eq!(manager.core_report(), LocalWorld::default());
}

/// The most recently played world starts ahead of an open, hidden from the menu and the core,
/// and its open reuses that server instead of starting another.
#[test]
fn a_prestarted_world_opens_on_its_running_server() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    manager.inner.store.pin_clock(100);
    let _older = bds(manager, "a older");
    let latest = bds(manager, "latest");
    manager.inner.store.pin_clock(200);
    played(manager, &latest);
    manager.prestart();
    test_time::eventually("the prestart runs", || {
        life_state(manager) == WorldState::Running
    });
    assert_hidden(manager);
    assert_eq!(
        lock(&runner.started).last().map(|w| &w.id),
        Some(&latest.id)
    );
    manager.open(&latest.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    assert_eq!(runner.entered.load(Ordering::SeqCst), 1, "no new start");
    assert!(manager.core_report().target.is_some());
}

/// An open made while the prestart is still starting waits for that same server.
#[test]
fn an_open_during_a_prestart_adopts_the_starting_server() {
    let (runner, release) = FakeRunner::gated();
    let runner = Arc::new(runner);
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    played(manager, &world);
    manager.prestart();
    test_time::eventually("the prestart begins", || {
        runner.entered.load(Ordering::SeqCst) == 1
    });
    assert_hidden(manager);
    manager.open(&world.id, None).expect("open");
    assert_eq!(manager.status().state, WorldState::Starting);
    drop(release);
    wait_for(manager, WorldState::Running);
    assert_eq!(runner.entered.load(Ordering::SeqCst), 1);
}

/// Opening a different world saves and stops the hidden server before its own starts.
#[test]
fn opening_another_world_stops_the_hidden_server_first() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    manager.inner.store.pin_clock(100);
    let other = bds(manager, "other");
    let latest = bds(manager, "latest");
    manager.inner.store.pin_clock(200);
    played(manager, &latest);
    manager.prestart();
    test_time::eventually("the prestart runs", || {
        life_state(manager) == WorldState::Running
    });
    let prestarted = runner.last();
    manager.open(&other.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    assert!(prestarted.stopped());
    assert!(!runner.overlapped.load(Ordering::SeqCst));
    assert_eq!(lock(&runner.started).last().map(|w| &w.id), Some(&other.id));
}

/// A left BDS world keeps running hidden, and returning to it skips the start.
#[test]
fn a_left_world_reopens_on_its_kept_server() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    manager.leave();
    assert_hidden(manager);
    assert!(!runner.last().stopped());
    manager.open(&world.id, None).expect("reopen");
    wait_for(manager, WorldState::Running);
    assert_eq!(runner.entered.load(Ordering::SeqCst), 1);
}

/// A kept server saves and stops once its standby period ends.
#[test]
fn a_kept_server_stops_when_its_standby_ends() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = standby_fixture(&runner, Some(Duration::from_millis(1)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    manager.leave();
    test_time::eventually("the kept server stops", || runner.last().stopped());
    wait_for(manager, WorldState::Idle);
}

/// Without standby, leaving stops the server at once as before.
#[test]
fn leaving_without_standby_closes_the_world() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = standby_fixture(&runner, None);
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    played(manager, &world);
    manager.prestart();
    assert_eq!(life_state(manager), WorldState::Idle, "no prestart");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    manager.leave();
    test_time::eventually("the server stops", || runner.last().stopped());
    assert_eq!(runner.entered.load(Ordering::SeqCst), 1);
}

/// Editing a hidden world restarts it with the change, and deleting it stops it first.
#[test]
fn changing_a_hidden_world_stops_its_server() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    manager.leave();
    let kept = runner.last();
    manager
        .update(
            &world.id,
            &WorldUpdate {
                name: Some("renamed".to_owned()),
                ..WorldUpdate::default()
            },
        )
        .expect("rename");
    test_time::eventually("the edit stops the kept server", || kept.stopped());
    manager.open(&world.id, None).expect("reopen");
    wait_for(manager, WorldState::Running);
    assert_eq!(
        lock(&runner.started).last().map(|w| w.name.as_str()),
        Some("renamed")
    );
    manager.leave();
    let kept = runner.last();
    manager.delete(&world.id).expect("delete a hidden world");
    assert!(kept.stopped());
}

/// Only a latest BDS world with an installed server prestarts, and never over an open world.
#[test]
fn prestart_skips_worlds_it_cannot_start_quietly() {
    let runner = Arc::new(FakeRunner::default());
    let installed = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &installed.manager;
    manager.inner.store.pin_clock(100);
    let world = bds(manager, "bds");
    let flat_world = manager.create(&flat("dragonfly")).expect("create");
    manager.inner.store.pin_clock(200);
    played(manager, &flat_world);
    manager.prestart();
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    manager.prestart();
    assert_eq!(runner.entered.load(Ordering::SeqCst), 1);
    let missing = Arc::new(FakeRunner::default());
    let runner_dyn: Arc<dyn Runner> = missing.clone();
    let uninstalled = fixture(|parts, dir| {
        parts.runners.bds = Some(runner_dyn);
        parts.setup = Some(provisioner(dir, LINUX));
        parts.standby = Some(Duration::from_secs(600));
    });
    uninstalled.manager.accept_eula().expect("accept");
    let world = bds(&uninstalled.manager, "w");
    played(&uninstalled.manager, &world);
    uninstalled.manager.prestart();
    assert_eq!(missing.entered.load(Ordering::SeqCst), 0);
}

/// Adopting a running hidden server grants the host signed in since it started.
#[test]
fn adopting_a_running_server_grants_the_current_host() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    played(manager, &world);
    manager.prestart();
    test_time::eventually("the prestart runs", || {
        life_state(manager) == WorldState::Running
    });
    manager.set_host(Host::Xuid(HOST.to_owned()));
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    assert_eq!(*lock(&runner.last().grants), [Some(HOST.to_owned())]);
}

/// A hidden server that crashes leaves no failure for the menu to show.
#[test]
fn a_hidden_server_exit_is_not_a_failure() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    played(manager, &world);
    manager.prestart();
    test_time::eventually("the prestart runs", || {
        life_state(manager) == WorldState::Running
    });
    runner.last().exit();
    test_time::eventually("the manager idles", || {
        life_state(manager) == WorldState::Idle
    });
    assert_eq!(manager.status().error, None);
}

/// Opens `world` on its own server and returns that server's live PvP edit, saved but pending.
fn play_with_live_pvp(manager: &Manager, world: &World) {
    use launcher::local_worlds::{
        in_world::{Action, update},
        options::WorldOption as O,
    };
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let saved = manager.inner.store.get(&world.id).expect("saved world");
    let pvp = update(&saved, Action::Option(O::Pvp, 0)).expect("an update");
    manager.apply(&world.id, &pvp).expect("applied");
}

/// Settings the server has not taken live make leaving stop it, so the next open starts with them.
#[test]
fn leaving_with_settings_for_the_next_start_stops_the_server() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    manager
        .update(
            &world.id,
            &WorldUpdate {
                game_mode: Some(bridge::GameMode::Creative),
                ..WorldUpdate::default()
            },
        )
        .expect("default game mode");
    manager.leave();
    test_time::eventually("the server stops", || runner.last().stopped());
    manager.open(&world.id, None).expect("reopen");
    wait_for(manager, WorldState::Running);
    assert_eq!(runner.entered.load(Ordering::SeqCst), 2);
    assert_eq!(
        lock(&runner.started).last().map(|w| w.game_mode),
        Some(bridge::GameMode::Creative)
    );

    let failing = Arc::new(FakeRunner {
        console_fails: true,
        ..FakeRunner::default()
    });
    let fixture = standby_fixture(&failing, Some(Duration::from_secs(600)));
    let world = bds(&fixture.manager, "w");
    play_with_live_pvp(&fixture.manager, &world);
    fixture.manager.leave();
    test_time::eventually("a missed live edit stops the server", || {
        failing.last().stopped()
    });
}

/// Edits the server took live keep it warm, and its later save records them.
#[test]
fn live_edits_keep_the_left_server_and_save_with_it() {
    let runner = Arc::new(FakeRunner::default());
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    play_with_live_pvp(manager, &world);
    manager.leave();
    assert!(!runner.last().stopped());
    manager.open(&world.id, None).expect("reopen");
    wait_for(manager, WorldState::Running);
    assert_eq!(runner.entered.load(Ordering::SeqCst), 1);
    manager.close();
    wait_for(manager, WorldState::Idle);
    let (_, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
    assert!(pending.is_empty(), "{pending:?}");
}

/// A cancelled adoption still records the live edits its server saved on the way down.
#[test]
fn cancelling_an_adoption_records_saved_live_edits() {
    let (runner, _release) = FakeRunner::granting();
    let runner = Arc::new(runner);
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    play_with_live_pvp(manager, &world);
    manager.leave();
    manager.set_host(Host::Xuid(HOST.to_owned()));
    manager.open(&world.id, None).expect("reopen");
    test_time::eventually("the adoption grants the host", || {
        lock(&runner.last().grants).len() == 1
    });
    manager.close();
    wait_for(manager, WorldState::Idle);
    assert!(runner.last().stopped());
    let (_, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
    assert!(pending.is_empty(), "{pending:?}");
}

/// A server that exits while its adoption reconciles the host fails instead of reporting running.
#[test]
fn an_exit_during_adoption_fails_the_world() {
    let (runner, release) = FakeRunner::granting();
    let runner = Arc::new(runner);
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    manager.leave();
    manager.set_host(Host::Xuid(HOST.to_owned()));
    manager.open(&world.id, None).expect("reopen");
    test_time::eventually("the adoption grants the host", || {
        lock(&runner.last().grants).len() == 1
    });
    runner.last().exit();
    drop(release);
    let status = wait_for(manager, WorldState::Failed);
    assert!(status.error.is_some());
}

/// A cancelled adoption records its server's saved live edits even when another world opens
/// while that server is still saving.
#[test]
fn a_cancelled_adoption_records_saved_edits_across_a_new_open() {
    let (grant_release, grant_gate) = crossbeam_channel::bounded::<()>(0);
    let (save_release, save) = crossbeam_channel::bounded::<()>(0);
    let runner = Arc::new(FakeRunner {
        grant_gate: Some(grant_gate),
        save: Some(save),
        ..FakeRunner::default()
    });
    let fixture = standby_fixture(&runner, Some(Duration::from_secs(600)));
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    let other = bds(manager, "other");
    play_with_live_pvp(manager, &world);
    manager.leave();
    manager.set_host(Host::Xuid(HOST.to_owned()));
    manager.open(&world.id, None).expect("reopen");
    test_time::eventually("the adoption grants the host", || {
        lock(&runner.last().grants).len() == 1
    });
    manager.close();
    manager.open(&other.id, None).expect("open another world");
    drop(save_release);
    drop(grant_release);
    wait_for(manager, WorldState::Running);
    let (_, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
    assert!(pending.is_empty(), "{pending:?}");
}
