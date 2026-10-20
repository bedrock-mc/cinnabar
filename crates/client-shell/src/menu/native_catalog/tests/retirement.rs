//! Actual catalog cancellation and stale publication retain one bounded retiring worker.

use super::*;
use launcher_host::{auth::test_support::native_state, native_account::CatalogListing};
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::mpsc,
    task::{Context, Poll, Waker},
};
use tokio_util::task::TaskTracker;

/// The actual read capture stays in destruction until its fixture release sender is dropped.
struct Read {
    started: Option<mpsc::Sender<()>>,
    dropping: mpsc::Sender<std::thread::ThreadId>,
    release: mpsc::Receiver<()>,
    complete: bool,
    result_owner: Arc<()>,
}

impl Future for Read {
    type Output = io::Result<CatalogListing>;

    fn poll(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(started) = self.started.take() {
            let _ = started.send(());
        }
        if self.complete {
            Poll::Ready(Err(io::Error::other(CompletedResult {
                _lifetime: Arc::clone(&self.result_owner),
            })))
        } else {
            Poll::Pending
        }
    }
}

/// The completed error retains this receipt until its actual queued result is consumed.
#[derive(Debug)]
struct CompletedResult {
    _lifetime: Arc<()>,
}

impl std::fmt::Display for CompletedResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("synthetic catalog section failure")
    }
}

impl std::error::Error for CompletedResult {}

impl Drop for Read {
    fn drop(&mut self) {
        let _ = self.dropping.send(std::thread::current().id());
        // Sender destruction also releases this owner if an earlier fixture assertion panics.
        let _ = self.release.recv();
    }
}

/// Waits for an actual worker observation without a sleep or elapsed-time assertion.
fn observed<T>(receive: &mpsc::Receiver<T>, label: &str) -> T {
    let mut value = None;
    test_time::eventually(label, || match receive.try_recv() {
        Ok(observed) => {
            value = Some(observed);
            true
        }
        Err(mpsc::TryRecvError::Empty) => false,
        Err(mpsc::TryRecvError::Disconnected) => panic!("worker observation disconnected"),
    });
    value.unwrap()
}

/// Repeated refresh and account replacement cannot refill while the actual old capture retires.
#[test]
fn actual_catalog_retirement_coalesces_refresh_and_rejects_stale_publication() {
    for complete in [false, true] {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let tasks = TaskTracker::new();
        let layout = launcher::test_support::scratch("catalog-retirement");
        let mut menu = MenuRuntime::new_with_layout(
            true,
            Some(2),
            "Fixture".into(),
            layout,
            crate::player_skin::LocalPlayerSkin::generated_default("Fixture"),
        )
        .with_native_sign_in(true)
        .with_io(runtime.handle().clone(), tasks.clone());
        menu.auth_process = Some(native_state(AuthState::Authenticated));
        menu.feeds.account_active_id = Some("123".into());
        let path = menu.layout.auth_cache();
        let account = menu.native_account_for(path.clone());
        let (started, running) = mpsc::channel();
        let (dropping, held) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let result_owner = Arc::new(());
        let worker = CatalogWorker::start_test_read(
            Read {
                started: Some(started),
                dropping,
                release: blocked,
                complete,
                result_owner: Arc::clone(&result_owner),
            },
            runtime.handle().clone(),
            tasks.clone(),
        )
        .unwrap();
        menu.native_catalog.request = Some(Request {
            epoch: menu.native_catalog.epoch,
            worker,
            completed: None,
        });
        observed(&running, "the actual catalog read to be polled");
        menu.native_catalog.refresh();
        let worker_thread = observed(&held, "the actual catalog read capture to enter Drop");
        assert_ne!(worker_thread, std::thread::current().id());
        tasks.close();
        let mut wait = std::pin::pin!(tasks.wait());
        let mut context = Context::from_waker(Waker::noop());
        assert!(wait.as_mut().poll(&mut context).is_pending());

        menu.feeds.account_active_id = Some("456".into());
        let replacement = menu.native_account_for(path);
        assert!(!Arc::ptr_eq(&account, &replacement));
        menu.apply_native_catalog(CatalogCards {
            realms: Ok(vec![realm("replacement")]),
            friends: Ok(vec![friend("replacement").into()]),
        });
        menu.catalog_message = Some("replacement presentation".into());
        let now = Instant::now();
        for _ in 0..4 {
            menu.native_catalog.refresh();
            menu.native_catalog.pause();
            menu.poll_native_catalog(now);
            assert!(menu.native_catalog.request.is_none());
            assert!(
                menu.native_catalog
                    .retiring
                    .as_ref()
                    .is_some_and(|worker| !worker.is_finished())
            );
            assert_eq!(tasks.len(), 1);
            assert!(wait.as_mut().poll(&mut context).is_pending());
            assert_eq!(menu.realms[0].name, "replacement");
            assert_eq!(
                menu.catalog_message.as_deref(),
                Some("replacement presentation")
            );
        }

        drop(release);
        test_time::eventually("the actual retiring catalog thread to finish", || {
            menu.native_catalog
                .retiring
                .as_ref()
                .is_some_and(CatalogWorker::is_finished)
        });
        assert!(wait.as_mut().poll(&mut context).is_ready());
        assert_eq!(
            Arc::strong_count(&result_owner),
            if complete { 2 } else { 1 },
            "the selected stale result must remain queued for the real controller"
        );
        menu.poll_native_catalog(now);
        assert!(menu.native_catalog.retiring.is_none());
        assert_eq!(Arc::strong_count(&result_owner), 1);
        assert!(
            menu.native_catalog.request.is_some(),
            "retired cleanup still blocked real replacement admission"
        );
        assert_eq!(menu.realms[0].name, "replacement");
        assert_eq!(
            menu.catalog_message.as_deref(),
            Some("replacement presentation")
        );
        drop(menu);
        drop(account);
        drop(replacement);
        runtime.block_on(tasks.wait());
    }
}
