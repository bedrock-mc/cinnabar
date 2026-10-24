//! Live world settings, report notifications and save acknowledgment through the manager.

use super::*;

/// Saving a new default mode wakes every route subscriber without restarting the server.
#[test]
fn applied_game_mode_wakes_route_subscribers() {
    let runner = Arc::new(FakeRunner::default());
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let before = manager.world_report();
    let subscribers = [manager.subscribe(), manager.subscribe()];
    manager
        .apply(
            &world.id,
            &WorldUpdate {
                game_mode: Some(bridge::GameMode::Creative),
                ..WorldUpdate::default()
            },
        )
        .expect("save the default mode");
    for changes in subscribers {
        changes
            .try_recv()
            .expect("wake for the changed world report");
    }
    manager.open(&world.id, None).expect("idempotent open");
    let after = manager.world_report();
    assert_eq!(after.state, WorldState::Running);
    assert_eq!(after.target, before.target);
    assert_eq!(
        after.world.expect("world").game_mode,
        bridge::GameMode::Creative
    );
    assert_eq!(lock(&runner.made).len(), 1, "the server stays running");
}

/// An in-game change reaches the running server at once and stays saved; a clean stop, after
/// which the server's own data holds it, ends it being pending.
#[test]
fn applied_settings_reach_the_running_server_and_persist() {
    use launcher::local_worlds::{
        in_world::{Action, update},
        options::WorldOption as O,
        world::Difficulty,
    };
    let runner = Arc::new(FakeRunner::default());
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    let hard = update(&world, Action::Difficulty(Difficulty::Hard)).expect("an update");
    manager.apply(&world.id, &hard).expect("saved while closed");
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let instance = runner.last();
    assert!(
        lock(&instance.console).is_empty(),
        "a closed world takes no commands"
    );
    let world = manager.inner.store.get(&world.id).expect("saved world");
    assert_eq!(world.difficulty, Difficulty::Hard);
    let cheats = update(&world, Action::Option(O::ActivateCheats, 1)).expect("cheats on");
    let world = manager.apply(&world.id, &cheats).expect("apply").world;
    let keep = update(&world, Action::Option(O::KeepInventory, 1)).expect("a rule");
    let applied = manager.apply(&world.id, &keep).expect("apply");
    assert!(!applied.missed);
    let saved = applied.world;
    assert!(saved.options.enabled(O::KeepInventory) && saved.options.enabled(O::ActivateCheats));
    assert_eq!(
        *lock(&instance.console),
        [
            "changesetting allow-cheats true",
            "gamerule keepinventory true"
        ]
    );
    let (_, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
    assert_eq!(
        pending.get(O::KeepInventory),
        Some(1),
        "pending until the server saves"
    );
    manager.close();
    wait_for(manager, WorldState::Idle);
    let (reopened, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
    assert!(pending.is_empty(), "{pending:?}");
    assert!(reopened.options.enabled(O::KeepInventory));
}

/// Applies a live PvP edit and checks its saved value before and after shutdown.
fn pending_after_live_edit(
    console_fails: bool,
    unclean: bool,
) -> launcher::local_worlds::options::OptionChanges {
    use launcher::local_worlds::{
        in_world::{Action, update},
        options::WorldOption as O,
    };
    let runner = Arc::new(FakeRunner {
        console_fails,
        unclean,
        ..FakeRunner::default()
    });
    let (fixture, world) = bds_world(&runner);
    let manager = &fixture.manager;
    manager.open(&world.id, None).expect("open");
    wait_for(manager, WorldState::Running);
    let world = manager.inner.store.get(&world.id).expect("saved world");
    let pvp = update(&world, Action::Option(O::Pvp, 0)).expect("an update");
    let applied = manager.apply(&world.id, &pvp).expect("saved");
    assert_eq!(applied.missed, console_fails);
    assert!(
        !applied.world.options.enabled(O::Pvp),
        "the change is saved either way"
    );
    manager.close();
    wait_for(manager, WorldState::Idle);
    let (saved, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
    assert!(!saved.options.enabled(O::Pvp));
    pending
}

/// A failed console write is reported separately from persistence, even after a clean stop.
#[test]
fn a_failed_live_change_is_reported_and_stays_pending() {
    use launcher::local_worlds::options::WorldOption as O;
    assert_eq!(pending_after_live_edit(true, false).get(O::Pvp), Some(0));
}

/// A successful stop call without a clean exit must not acknowledge live changes.
#[test]
fn live_changes_stay_pending_after_an_unclean_stop() {
    use launcher::local_worlds::options::WorldOption as O;
    assert_eq!(pending_after_live_edit(false, true).get(O::Pvp), Some(0));
}

/// A crashed server's live edits cannot acknowledge a failed edit in its replacement.
#[test]
fn a_failed_live_change_after_reopening_a_crashed_server_stays_pending() {
    use launcher::local_worlds::{
        in_world::{Action, update},
        options::WorldOption as O,
    };
    for clear_failure in [false, true] {
        let runner = Arc::new(FakeRunner::default());
        let (fixture, world) = bds_world(&runner);
        let manager = &fixture.manager;
        manager.open(&world.id, None).expect("open");
        wait_for(manager, WorldState::Running);
        let pvp = update(&world, Action::Option(O::Pvp, 0)).expect("an update");
        assert!(!manager.apply(&world.id, &pvp).expect("applied").missed);
        runner.last().exit();
        wait_for(manager, WorldState::Failed);
        if clear_failure {
            manager.close();
        }
        manager.open(&world.id, None).expect("reopen");
        wait_for(manager, WorldState::Running);
        test_time::eventually("the replacement applies its startup settings", || {
            let (_, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
            pending.is_empty()
        });
        runner
            .last()
            .console_fails
            .store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(manager.apply(&world.id, &pvp).expect("saved").missed);
        manager.close();
        wait_for(manager, WorldState::Idle);
        let (saved, pending, _) = manager.inner.store.snapshot(&world.id).expect("snapshot");
        assert!(!saved.options.enabled(O::Pvp));
        assert_eq!(
            pending.get(O::Pvp),
            Some(0),
            "the failed edit must be retried"
        );
    }
}
