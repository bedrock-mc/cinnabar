//! Leaving a BDS world saves and stops its server before any later open can use its files.

use std::sync::atomic::Ordering;

use super::*;
use crate::local_worlds::test_support::{FAKE_BDS, installed_provisioner};

/// A manager with an installed BDS served by `runner`.
fn bds_fixture(runner: &Arc<FakeRunner>) -> Fixture {
    let runner: Arc<dyn Runner> = runner.clone();
    fixture(|parts, dir| {
        parts.runners.bds = Some(runner);
        parts.setup = Some(Arc::new(installed_provisioner(dir.path(), LINUX, FAKE_BDS)));
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

/// Leaving requests a save immediately, and reopening waits for it before starting a new server.
#[test]
fn leaving_bds_stops_it_and_reopening_waits_for_its_save() {
    let (runner, save) = FakeRunner::saving();
    let runner = Arc::new(runner);
    let fixture = bds_fixture(&runner);
    let save = save;
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let old = runner.last();
    manager.leave();
    assert_eq!(manager.status().state, WorldState::Stopping);
    assert!(matches!(manager.delete(&world.id), Err(WorldError::InUse)));
    manager.open(&world.id, None).expect("reopen while saving");
    assert_eq!(manager.status().state, WorldState::Starting);
    assert_eq!(runner.entered.load(Ordering::SeqCst), 1);
    drop(save);
    wait_for(manager, WorldState::Running);
    assert!(old.stopped());
    assert_eq!(runner.entered.load(Ordering::SeqCst), 2);
    assert!(!runner.overlapped.load(Ordering::SeqCst));
}

/// Leaving saves live edits, while settings for the next start are loaded by the new server.
#[test]
fn leaving_bds_preserves_settings_for_the_next_open() {
    use launcher::local_worlds::{
        in_world::{Action, update},
        options::WorldOption as O,
    };

    for unclean in [false, true] {
        let runner = Arc::new(FakeRunner {
            unclean,
            ..FakeRunner::default()
        });
        let fixture = bds_fixture(&runner);
        let manager = &fixture.manager;
        let world = bds(manager, "w");
        manager.open(&world.id, None).expect("open");
        wait_for(manager, WorldState::Running);
        let pvp = update(&world, Action::Option(O::Pvp, 0)).expect("live PvP edit");
        manager.apply(&world.id, &pvp).expect("apply live edit");
        manager
            .update(
                &world.id,
                &WorldUpdate {
                    game_mode: Some(bridge::GameMode::Creative),
                    ..WorldUpdate::default()
                },
            )
            .expect("save startup setting");
        let old = runner.last();
        manager.leave();
        wait_for(manager, WorldState::Idle);
        assert!(old.stopped());
        let (_, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
        assert_eq!(pending.is_empty(), !unclean, "unclean: {unclean}");
        manager.open(&world.id, None).expect("reopen");
        wait_for(manager, WorldState::Running);
        let started = lock(&runner.started);
        assert_eq!(started.len(), 2);
        assert_eq!(started[1].game_mode, bridge::GameMode::Creative);
        assert!(!started[1].options.enabled(O::Pvp));
    }
}

/// Leaving during startup cancels that start and permits a later explicit open.
#[test]
fn leaving_bds_during_startup_cancels_it() {
    let (runner, release) = FakeRunner::gated();
    let runner = Arc::new(runner);
    let fixture = bds_fixture(&runner);
    let release = release;
    let manager = &fixture.manager;
    let world = bds(manager, "w");
    manager.open(&world.id, None).expect("open");
    test_time::eventually("the start begins", || {
        runner.entered.load(Ordering::SeqCst) == 1
    });
    manager.leave();
    drop(release);
    wait_for(manager, WorldState::Idle);
    manager.open(&world.id, None).expect("explicit reopen");
    wait_for(manager, WorldState::Running);
    assert_eq!(runner.entered.load(Ordering::SeqCst), 2);
}
