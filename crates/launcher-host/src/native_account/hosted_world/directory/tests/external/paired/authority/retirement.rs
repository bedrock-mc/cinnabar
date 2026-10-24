//! Account admission and process completion remain distinct after original host cleanup fails.

use super::*;
use crate::{
    launcher_account::retirement_test_support, local_worlds::test_support::NativeHostFixture,
};

#[test]
fn account_retirement_keeps_original_pending_host_without_blocking_quit() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let (quit_finished, pending_retained, account_blocked) = runtime.block_on(async {
        tokio::time::timeout(ACQUISITION_TIMEOUT, async {
            let world = NativeHostFixture::new();
            let issuer = super::super::Provider::new();
            let namespace = tempfile::tempdir().unwrap();
            let deadline = Instant::now() + ACQUISITION_TIMEOUT;
            let authority =
                Authority::open(&world.manager, namespace.path(), &issuer.jwks, deadline).await;
            let tasks = tokio_util::task::TaskTracker::new();
            let held = authority.account.begin().unwrap();
            let (launcher, selected) = retirement_test_support::hosted_account(
                namespace.path(),
                Arc::clone(&authority.account),
                tokio::runtime::Handle::current(),
                tasks.clone(),
            );
            let original = launcher.worker_lifetime();
            launcher.set_native_host_intent(Some(world.manager.native_host_intent().unwrap()));
            selected.await.unwrap();
            drop(launcher);
            tasks.close();
            tasks.wait().await;
            let quit_finished = tasks.is_empty();
            drop(held);

            // This is an actual signed membership, not a synthesized pending-directory flag.
            let Authority { host, peer, .. } = authority;
            drop(host);
            let pending = world.manager.native_host_cleanup().unwrap();
            let pending_retained = pending.is_some();
            drop(pending);
            let account_blocked = !original.is_empty();

            // Cancellation intentionally ends this local service scenario before conditional leave.
            drop((original, peer, world, issuer));
            (quit_finished, pending_retained, account_blocked)
        })
        .await
        .unwrap()
    });
    // Dropping this test-owned runtime retires every intentionally canceled local fixture task.
    drop(runtime);
    assert!(
        quit_finished,
        "unconfirmed hosting blocked process completion"
    );
    assert!(pending_retained, "original signed membership was discarded");
    assert!(
        account_blocked,
        "account admission overtook the original pending hosted membership"
    );
}
