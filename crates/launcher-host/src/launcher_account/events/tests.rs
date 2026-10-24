use super::*;

/// Runs the actual joined owner with the same enclosing guard, permitting deterministic polls.
pub(in crate::launcher_account) async fn run_owner(
    shared: &Mutex<Snapshot>,
    requests: &Receiver<()>,
    lifetime: &TitlePresenceLifetime,
    _process_stop: impl std::future::Future<Output = ()>,
) {
    let guard = lifetime.worker_guard();
    run(shared, requests, lifetime, _process_stop).await;
    drop(guard);
}

#[tokio::test]
async fn native_signout_retires_the_original_worker() {
    tokio::time::timeout(Duration::from_secs(5), async {
        let shared = Mutex::new(Snapshot::native(
            None,
            bridge::contract::DEFAULT_SERVICE_LANGUAGE,
        ));
        let lifetime = TitlePresenceLifetime::new();
        let (requests, receive) = bounded(1);
        requests.send(()).unwrap();
        let guard = lifetime.worker_guard();
        run(&shared, &receive, &lifetime, std::future::pending()).await;
        drop(guard);
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Complete)
        );
        assert!(lifetime.worker_finished());
    })
    .await
    .expect("native signout owner did not retire");
}

#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
#[tokio::test]
async fn final_process_stop_finishes_even_an_unpolled_original_product_owner() {
    let api = crate::product_api::Client::default();
    let shared = Mutex::new(Snapshot {
        native_product_api: Some(api.clone()),
        ..Snapshot::default()
    });
    let (_requests, received) = bounded(1);
    let lifetime = TitlePresenceLifetime::new();
    let (execution, stop) = bridge::tasks::NativeExecution::for_process(
        Handle::current(),
        tokio_util::task::TaskTracker::new(),
    );
    stop.stop();
    run(&shared, &received, &lifetime, execution.stopped()).await;
    let mut finished = std::pin::pin!(api.finished());
    let mut context = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(std::future::Future::poll(finished.as_mut(), &mut context).is_ready());
}

/// A quiet release queue still checks its real account scope without blocking frame snapshot reads.
#[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
#[tokio::test]
async fn release_account_check_keeps_the_frame_snapshot_available() {
    use bedrock_services::{minecraft, test_support::HostedAuthority};
    use std::{
        future::Future,
        num::NonZeroU32,
        sync::atomic::{AtomicBool, Ordering},
        task::{Context, Waker},
    };

    tokio::time::timeout(crate::native_account::ACQUISITION_TIMEOUT, async {
        let HostedAuthority {
            account: services,
            identity,
            peer,
        } = HostedAuthority::acquire_activation(
            crate::native_account::authentication_policy(),
            minecraft::Config::desktop(
                protocol::GAME_VERSION,
                u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
                uuid::Uuid::from_u128(1),
            ),
            NonZeroU32::new(2).unwrap(),
            bridge::contract::PRESENCE_TITLE_ID.to_string(),
            "product-fixture-audience".into(),
        )
        .await;
        let namespace = tempfile::tempdir().unwrap();
        let (account, _store) = crate::native_account::NativeAccount::product_api_fixture_account(
            namespace.path(),
            services,
            &identity,
        );
        let credential = Arc::clone(&account)
            .product_api_credential(None)
            .await
            .unwrap();
        assert_eq!(credential.scope.current(), Some(true));
        let api = crate::product_api::Client::default();
        let shared = Arc::new(Mutex::new(Snapshot {
            native_product_api: Some(api.clone()),
            ..Snapshot::default()
        }));
        let observed = Arc::new(AtomicBool::new(false));
        let available = Arc::new(AtomicBool::new(false));
        let probe_shared = Arc::downgrade(&shared);
        let probe_observed = Arc::clone(&observed);
        let probe_available = Arc::clone(&available);
        api.observe_release_check(credential.scope, move || {
            let shared = probe_shared.upgrade().unwrap();
            probe_available.store(shared.try_lock().is_ok(), Ordering::Relaxed);
            probe_observed.store(true, Ordering::Relaxed);
        });
        let (_requests, received) = bounded(1);
        let lifetime = TitlePresenceLifetime::new();
        {
            let mut worker = std::pin::pin!(control(&shared, &received, &lifetime));
            let mut context = Context::from_waker(Waker::noop());
            assert!(worker.as_mut().poll(&mut context).is_pending());
            lifetime.wake();
            assert!(worker.as_mut().poll(&mut context).is_pending());
        }
        let checked = observed.load(Ordering::Relaxed);
        let frame_available = available.load(Ordering::Relaxed);
        let _ = peer.abort_activation().await;
        assert!(
            checked,
            "the original event worker did not check its acquired release scope"
        );
        assert!(
            frame_available,
            "protected release checking held the frame snapshot lock"
        );
    })
    .await
    .expect("release frame isolation fixture stalled");
}
