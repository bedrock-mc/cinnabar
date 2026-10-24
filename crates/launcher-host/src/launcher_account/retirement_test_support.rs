//! Rendezvous on the original Home and publisher workers, without service authentication.

use super::*;
use crate::native_account::{NativeAccount, test_support::service_operation};
use std::{
    future::Future,
    io::{self, Write},
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
};
use tokio::sync::oneshot;

/// Whether cancellation retired the retained read before its explicit next-poll probe.
#[derive(Debug, PartialEq, Eq)]
pub enum ReadObservation {
    Canceled,
    StillHeld,
}

/// Owns only finite fixture rendezvous; no signal can complete title retirement directly.
pub struct Signals {
    pub read_entered: oneshot::Receiver<()>,
    pub cleanup_entered: oneshot::Receiver<()>,
    pub inspect_read: oneshot::Sender<()>,
    pub observation: oneshot::Receiver<ReadObservation>,
    pub release_cleanup: oneshot::Sender<()>,
    pub lease_released: Arc<AtomicBool>,
}

pub(super) struct State {
    account: Arc<NativeAccount>,
    published: Mutex<Option<oneshot::Sender<()>>>,
    publication: Mutex<Option<oneshot::Receiver<()>>>,
    read_entered: Mutex<Option<oneshot::Sender<()>>>,
    inspect_read: Mutex<Option<oneshot::Receiver<()>>>,
    observation: Mutex<Option<oneshot::Sender<ReadObservation>>>,
    cleanup_entered: Mutex<Option<oneshot::Sender<()>>>,
    cleanup: Mutex<Option<oneshot::Receiver<()>>>,
    lease_released: Arc<AtomicBool>,
    read_released: tokio::sync::Notify,
    reader_cleanup: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

/// Starts the original worker topology with a retained Home read and acknowledged fixture publisher.
pub fn account(
    namespace: &std::path::Path,
    runtime: Handle,
    tasks: TaskTracker,
) -> (LauncherAccount, Signals) {
    start(namespace, runtime, tasks, None)
}

/// Exercises the original host coordinator without enabling unrelated authenticated fixture feeds.
#[cfg(test)]
pub(crate) fn hosted_account(
    namespace: &std::path::Path,
    original: Arc<NativeAccount>,
    runtime: Handle,
    tasks: TaskTracker,
) -> (LauncherAccount, oneshot::Receiver<()>) {
    let account = LauncherAccount::start(
        namespace.join("artwork"),
        Snapshot::native(None, ""),
        runtime.clone(),
        tasks,
        std::future::pending(),
    );
    let observed = native_host::start_test(
        original,
        Arc::clone(&account.snapshot),
        runtime,
        &account.workers,
    );
    (account, observed)
}

/// Holds the actual canceled Home worker after it releases its lease, independently of title cleanup.
pub fn account_with_reader_cleanup(
    namespace: &std::path::Path,
    runtime: Handle,
    tasks: TaskTracker,
    cleanup: std::sync::mpsc::Receiver<()>,
) -> (LauncherAccount, Signals) {
    start(namespace, runtime, tasks, Some(cleanup))
}

/// Arms the original worker topology before any fixture boundary can be observed.
fn start(
    namespace: &std::path::Path,
    runtime: Handle,
    tasks: TaskTracker,
    reader_cleanup: Option<std::sync::mpsc::Receiver<()>>,
) -> (LauncherAccount, Signals) {
    let path = namespace.join("retirement-lease.json");
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&path)
        .unwrap()
        .write_all(br#"{"refresh_token":"synthetic-refresh","cinnabar_sign_in_generation":"retirement-fixture"}"#)
        .unwrap();
    let account = Arc::new(NativeAccount::new(
        launcher::accounts::AccountStore::new(path),
        namespace.join("device.json"),
        None,
    ));
    let (published, publication) = oneshot::channel();
    let (read_entered, read) = oneshot::channel();
    let (inspect, inspect_read) = oneshot::channel();
    let (observation, observed) = oneshot::channel();
    let (cleanup_entered, removal) = oneshot::channel();
    let (release_cleanup, cleanup) = oneshot::channel();
    let lease_released = Arc::new(AtomicBool::new(false));
    let state = Arc::new(State {
        account,
        published: Mutex::new(Some(published)),
        publication: Mutex::new(Some(publication)),
        read_entered: Mutex::new(Some(read_entered)),
        inspect_read: Mutex::new(Some(inspect_read)),
        observation: Mutex::new(Some(observation)),
        cleanup_entered: Mutex::new(Some(cleanup_entered)),
        cleanup: Mutex::new(Some(cleanup)),
        lease_released: Arc::clone(&lease_released),
        read_released: tokio::sync::Notify::new(),
        reader_cleanup: Mutex::new(reader_cleanup),
    });
    let initial = Snapshot {
        retirement_probe: Some(state),
        ..Snapshot::native(None, "")
    };
    let account = LauncherAccount::start(
        namespace.join("artwork"),
        initial,
        runtime,
        tasks,
        std::future::pending(),
    );
    (
        account,
        Signals {
            read_entered: read,
            cleanup_entered: removal,
            inspect_read: inspect,
            observation: observed,
            release_cleanup,
            lease_released,
        },
    )
}

impl State {
    /// Admits the Home phase only after the original publisher completed its fixture update.
    pub(super) fn published(&self) {
        if let Some(published) = self.published.lock().unwrap().take() {
            let _ = published.send(());
        }
    }

    /// Holds the same real account operation/lease until the existing Home wait cancels it.
    pub(super) async fn read(&self) {
        let publication = self.publication.lock().unwrap().take().unwrap();
        publication.await.unwrap();
        let operation = service_operation(&self.account).unwrap();
        let read = HeldRead {
            operation: Some(operation),
            entered: self.read_entered.lock().unwrap().take(),
            inspect: self.inspect_read.lock().unwrap().take(),
            state: self,
        };
        read.await;
    }

    /// Cleanup must obtain the same actual lease and then receive a separate service-boundary ACK.
    pub(super) async fn remove(&self) -> io::Result<()> {
        self.cleanup_entered
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .send(())
            .unwrap();
        self.read_released.notified().await;
        let operation = service_operation(&self.account)?;
        let cleanup = self.cleanup.lock().unwrap().take().unwrap();
        let result = cleanup
            .await
            .map_err(|_| io::Error::other("fixture cleanup ended"));
        operation();
        result
    }
}

struct HeldRead<'a> {
    operation: Option<Box<dyn FnOnce() + Send>>,
    entered: Option<oneshot::Sender<()>>,
    inspect: Option<oneshot::Receiver<()>>,
    state: &'a State,
}

impl Future for HeldRead<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let this = self.get_mut();
        if let Some(entered) = this.entered.take() {
            entered.send(()).unwrap();
        }
        if this
            .inspect
            .as_mut()
            .is_some_and(|inspect| Pin::new(inspect).poll(cx).is_ready())
        {
            this.inspect = None;
            if let Some(observation) = this.state.observation.lock().unwrap().take() {
                let _ = observation.send(ReadObservation::StillHeld);
            }
        }
        Poll::Pending
    }
}

impl Drop for HeldRead<'_> {
    fn drop(&mut self) {
        self.operation.take().unwrap()();
        self.state.lease_released.store(true, Ordering::Release);
        self.state.read_released.notify_one();
        if let Some(observation) = self.state.observation.lock().unwrap().take() {
            let _ = observation.send(ReadObservation::Canceled);
        }
        if let Some(cleanup) = self.state.reader_cleanup.lock().unwrap().take() {
            let _ = cleanup.recv();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::task::Waker;

    /// Direct resource removal retires every original worker after its real cleanup acknowledgment.
    #[test]
    fn dropping_account_retires_all_workers_after_original_cleanup_ack() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        let tasks = TaskTracker::new();
        let namespace = tempfile::tempdir().unwrap();
        let (account, signals) = account(namespace.path(), runtime.handle().clone(), tasks.clone());
        let lifetime = account.title_presence_lifetime();
        let released = Arc::clone(&signals.lease_released);
        let (observed, pending_cleanup) = runtime.block_on(async {
            tokio::time::timeout(test_time::DEFAULT_TIMEOUT, async {
                signals.read_entered.await.unwrap();
                drop(account);
                signals.cleanup_entered.await.unwrap();
                let _ = signals.inspect_read.send(());
                let observed = signals.observation.await.unwrap();
                tasks.close();
                let pending_cleanup = {
                    let mut wait = std::pin::pin!(tasks.wait());
                    let mut context = Context::from_waker(Waker::noop());
                    wait.as_mut().poll(&mut context).is_pending()
                };
                signals.release_cleanup.send(()).unwrap();
                tasks.wait().await;
                (observed, pending_cleanup)
            })
            .await
            .unwrap()
        });

        assert_eq!(observed, ReadObservation::Canceled);
        assert!(
            pending_cleanup,
            "tracked retirement overtook the original cleanup acknowledgment"
        );
        assert!(
            tasks.is_empty(),
            "an original account worker remained tracked"
        );
        assert!(released.load(Ordering::Acquire));
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Complete)
        );
        assert!(lifetime.worker_finished());
    }
}
