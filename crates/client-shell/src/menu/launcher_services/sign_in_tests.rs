//! Adding an account waits for the selected service writers to finish retirement.

use super::*;
use bevy::{ecs::world::CommandQueue, prelude::World};
use launcher::menu::{MenuAction, auth::AuthState};
use launcher_host::{
    auth::test_support::native_state,
    launcher_account::{TitlePresenceRetirement, retirement_test_support},
};
use std::io::Write;
use tokio::sync::oneshot;

/// Observes one original worker boundary without sleeping or opening an account service.
async fn receive<T>(receiver: oneshot::Receiver<T>) -> T {
    tokio::time::timeout(test_time::DEFAULT_TIMEOUT, receiver)
        .await
        .unwrap()
        .unwrap()
}

#[test]
fn add_account_waits_for_original_writers_before_starting_native_sign_in() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()
        .unwrap();
    let tasks = tokio_util::task::TaskTracker::new();
    let layout = launcher::test_support::scratch("add-account-writer-retirement");
    let cache = layout.auth_cache();
    std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(&cache).unwrap().write_all(
        br#"{"access_token":"synthetic-access","refresh_token":"synthetic-refresh","cinnabar_sign_in_generation":"selected-account"}"#,
    ).unwrap();
    let store = launcher::accounts::AccountStore::new(cache.clone());
    store
        .remember_current("123", "Selected account", None)
        .unwrap();
    let namespace = tempfile::tempdir().unwrap();
    let (release_reader, reader_cleanup) = std::sync::mpsc::channel();
    let (account, signals) = retirement_test_support::account_with_reader_cleanup(
        namespace.path(),
        runtime.handle().clone(),
        tasks.clone(),
        reader_cleanup,
    );
    let lifetime = account.title_presence_lifetime();
    let mut slot = LauncherServices {
        running: Some(Attachment {
            cache: Some(cache),
            identity: Some("123".into()),
            account: None,
        }),
        presence_lifetime: Some(lifetime.clone()),
        workers: Some(account.worker_lifetime()),
        ..Default::default()
    };
    // No execution capability: a premature spawn reports failure instead of contacting Microsoft.
    let mut menu = MenuRuntime::new_with_layout(
        true,
        Some(2),
        "Selected account".into(),
        layout,
        crate::player_skin::LocalPlayerSkin::generated_default("Selected account"),
    );
    menu.auth_process = Some(native_state(AuthState::Authenticated));
    menu.feeds.account_active_id = Some("123".into());
    let mut world = World::new();
    world.insert_resource(LauncherAccount(account));
    runtime.block_on(receive(signals.read_entered));

    menu.activate(MenuAction::AddAccount);
    let queued_before_retirement = menu.current_auth().as_ref() == &AuthState::Checking;
    menu.poll_accounts();
    let no_storage_operation = menu.accounts.operation.is_none() && !menu.accounts.pending_ready;
    menu.poll_account(true);
    let queued_after_poll = menu.current_auth().as_ref() == &AuthState::Checking;
    let mut queue = CommandQueue::default();
    slot.drive(
        &mut Commands::new(&mut queue, &world),
        &mut menu,
        true,
        None,
    );
    queue.apply(&mut world);
    runtime.block_on(receive(signals.cleanup_entered));
    menu.poll_account(false);
    let queued_during_cleanup = menu.current_auth().as_ref() == &AuthState::Checking;
    let cleanup_pending = lifetime.retirement().is_none();
    let _ = signals.inspect_read.send(());
    let observed = runtime.block_on(receive(signals.observation));

    world.remove_resource::<LauncherAccount>();
    let _ = signals.release_cleanup.send(());
    test_time::eventually("the original title worker to finish", || {
        lifetime.worker_finished()
    });
    slot.drive(
        &mut Commands::new(&mut queue, &world),
        &mut menu,
        true,
        None,
    );
    queue.apply(&mut world);
    let queued_after_publisher = menu.current_auth().as_ref() == &AuthState::Checking;

    drop(release_reader);
    tasks.close();
    runtime.block_on(async {
        tokio::time::timeout(test_time::DEFAULT_TIMEOUT, tasks.wait())
            .await
            .unwrap();
    });
    slot.drive(
        &mut Commands::new(&mut queue, &world),
        &mut menu,
        true,
        None,
    );
    queue.apply(&mut world);
    let attempted_after_retirement = matches!(menu.current_auth().as_ref(), AuthState::Failed(_));
    drop((slot, menu, world));

    assert!(
        queued_before_retirement,
        "Add account started before retiring service writers"
    );
    assert!(
        no_storage_operation,
        "account polling treated the queued sign-in as a finished attempt"
    );
    assert!(
        queued_after_poll,
        "auth polling bypassed service retirement"
    );
    assert!(
        queued_during_cleanup,
        "sign-in overtook the original cleanup acknowledgment"
    );
    assert!(
        queued_after_publisher,
        "title completion released sign-in while an original account reader was still retiring"
    );
    assert!(
        cleanup_pending,
        "cleanup finished without its service acknowledgment"
    );
    assert_eq!(observed, retirement_test_support::ReadObservation::Canceled);
    assert_eq!(
        lifetime.retirement(),
        Some(TitlePresenceRetirement::Complete)
    );
    assert!(lifetime.worker_finished());
    assert!(
        attempted_after_retirement,
        "retirement did not release the queued sign-in"
    );
}
