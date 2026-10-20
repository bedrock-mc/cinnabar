//! Account switching cancels readers before the original publisher needs their lease.

use super::*;
use bevy::{ecs::world::CommandQueue, prelude::World};
use launcher_host::launcher_account::{TitlePresenceRetirement, retirement_test_support};
use std::sync::atomic::Ordering;
use tokio::sync::oneshot;

/// Observes an actual worker rendezvous within the fixture's finite scheduling bound.
async fn receive<T>(receiver: oneshot::Receiver<T>) -> T {
    tokio::time::timeout(test_time::DEFAULT_TIMEOUT, receiver)
        .await
        .unwrap()
        .unwrap()
}

#[test]
fn account_switch_cancels_home_lease_before_waiting_for_original_presence_cleanup() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let tasks = tokio_util::task::TaskTracker::new();
    let layout = launcher::test_support::scratch("presence-read-retirement");
    let namespace = tempfile::tempdir().unwrap();
    let (account, signals) =
        retirement_test_support::account(namespace.path(), runtime.handle().clone(), tasks.clone());
    let lifetime = account.title_presence_lifetime();
    let mut slot = LauncherServices {
        running: Some(Running::Native {
            cache: None,
            identity: None,
            account: None,
        }),
        presence_lifetime: Some(lifetime.clone()),
        ..Default::default()
    };
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Fixture".into(),
        layout,
        crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
    )
    .with_io(runtime.handle().clone(), tasks.clone())
    .with_native_sign_in(true);
    let mut world = World::new();
    world.insert_resource(LauncherAccount(account));
    runtime.block_on(receive(signals.read_entered));
    menu.accounts.operation = Some(launcher_host::accounts::Operation::SignOut);
    let mut queue = CommandQueue::default();
    slot.drive(
        &mut Commands::new(&mut queue, &world),
        &mut menu,
        true,
        false,
        None,
        Instant::now(),
    );
    queue.apply(&mut world);
    runtime.block_on(receive(signals.cleanup_entered));
    let operation_queued = menu.accounts.operation.is_some() && menu.accounts.work.is_none();
    let publisher_pending = lifetime.retirement().is_none();
    let _ = signals.inspect_read.send(());
    let observed = runtime.block_on(receive(signals.observation));

    world.remove_resource::<LauncherAccount>();
    let _ = signals.release_cleanup.send(());
    drop(world);
    drop(menu);
    drop(slot);
    tasks.close();
    runtime.block_on(async {
        tokio::time::timeout(test_time::DEFAULT_TIMEOUT, tasks.wait())
            .await
            .unwrap();
    });
    let released = signals.lease_released.load(Ordering::Acquire);
    assert!(
        operation_queued,
        "account mutation overtook the original publisher"
    );
    assert!(
        publisher_pending,
        "cleanup completed without its service ACK"
    );
    assert!(
        released,
        "fixture teardown retained the original read lease"
    );
    assert_eq!(
        lifetime.retirement(),
        Some(TitlePresenceRetirement::Complete)
    );
    assert!(lifetime.worker_finished());
    assert_eq!(
        observed,
        retirement_test_support::ReadObservation::Canceled,
        "account switch waited for presence cleanup while Home still held its lease"
    );
}
