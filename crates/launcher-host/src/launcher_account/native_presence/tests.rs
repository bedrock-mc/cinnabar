//! Actual account-lifetime driver behavior through its leased-service boundary.
use super::*;
use tokio::sync::{mpsc, oneshot};

struct LeaseContendedService<'a> {
    account: Arc<NativeAccount>,
    lifetime: &'a TitlePresenceLifetime,
    held: Mutex<Option<Box<dyn FnOnce()>>>,
    attempted: mpsc::Sender<()>,
    removed: std::sync::atomic::AtomicUsize,
}

impl Service for LeaseContendedService<'_> {
    type Scope = PresenceScope;

    async fn bind(&self) -> io::Result<PresenceScope> {
        let scope = NativeService {
            account: Arc::clone(&self.account),
            lifetime: self.lifetime,
        }
        .bind()
        .await?;
        *self.held.lock().unwrap() = Some(crate::native_account::presence::tests::hold_operation(
            &self.account,
        ));
        Ok(scope)
    }

    async fn update(
        &self,
        scope: &PresenceScope,
        state: &bridge::XboxPresenceState,
        dispatched: &mut bool,
    ) -> io::Result<Option<Duration>> {
        self.attempted.send(()).await.unwrap();
        Arc::clone(&self.account)
            .update_title_presence(scope, state, dispatched)
            .await
    }

    async fn remove(&self, _: &PresenceScope) -> io::Result<()> {
        // A successful cleanup cannot manufacture a prior remote write; this
        // control lets the real publisher report its own retirement outcome.
        self.removed
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Ok(())
    }
}

/// A timeout on the real account lease precedes every SDK/title request and cannot retain ambiguity.
#[tokio::test(start_paused = true)]
async fn lease_contended_update_does_not_block_original_presence_retirement() {
    let namespace = tempfile::tempdir().unwrap();
    let account = crate::native_account::presence::tests::verified_account(namespace.path());
    let shared = Mutex::new(Snapshot::default());
    let lifetime = TitlePresenceLifetime::new();
    let (attempted, mut attempts) = mpsc::channel(1);
    let service = LeaseContendedService {
        account,
        lifetime: &lifetime,
        held: Mutex::new(None),
        attempted,
        removed: std::sync::atomic::AtomicUsize::new(0),
    };
    let scenario = async {
        attempts.recv().await.unwrap();
        assert!(service.held.lock().unwrap().is_some());
        tokio::time::advance(REQUEST_BUDGET + Duration::from_millis(1)).await;
        lifetime.retire();
        service.held.lock().unwrap().take().unwrap()();
    };
    tokio::join!(publish(&service, &shared, &lifetime), scenario);
    assert_eq!(
        lifetime.retirement(),
        Some(TitlePresenceRetirement::Complete),
        "lease contention before dispatch retained a permanent remote-write obligation"
    );
    assert_eq!(
        service.removed.load(std::sync::atomic::Ordering::Relaxed),
        0,
        "an undispatched first update cannot require a remote cleanup"
    );
}

struct Fixture {
    updates: mpsc::Sender<bridge::XboxPresenceState>,
    update_release: Mutex<Vec<oneshot::Receiver<io::Result<()>>>>,
    removals: mpsc::Sender<()>,
    remove_release: Mutex<Option<oneshot::Receiver<io::Result<()>>>>,
    committed: Mutex<Vec<bridge::XboxPresenceState>>,
}
impl Service for Fixture {
    type Scope = ();
    async fn bind(&self) -> io::Result<()> {
        Ok(())
    }
    async fn update(
        &self,
        _: &(),
        state: &bridge::XboxPresenceState,
        dispatched: &mut bool,
    ) -> io::Result<Option<Duration>> {
        self.updates.send(state.clone()).await.unwrap();
        *dispatched = true;
        let release = self.update_release.lock().unwrap().pop();
        let outcome = if let Some(release) = release {
            release.await.unwrap()
        } else {
            Ok(())
        };
        self.committed.lock().unwrap().push(state.clone());
        outcome?;
        Ok(Some(Duration::from_secs(3600)))
    }
    async fn remove(&self, _: &()) -> io::Result<()> {
        self.removals.send(()).await.unwrap();
        let release = self.remove_release.lock().unwrap().take().unwrap();
        release.await.unwrap()
    }
}

struct Harness {
    service: Fixture,
    updates: mpsc::Receiver<bridge::XboxPresenceState>,
    removed: mpsc::Receiver<()>,
    release: oneshot::Sender<io::Result<()>>,
    cleanup: oneshot::Sender<io::Result<()>>,
}

/// Creates only test rendezvous; production retains no service payload queue.
fn fixture() -> Harness {
    let (updates, received) = mpsc::channel(1);
    let (removals, removed) = mpsc::channel(1);
    let (sent, release) = oneshot::channel();
    let (clean, cleanup) = oneshot::channel();
    Harness {
        service: Fixture {
            updates,
            update_release: Mutex::new(vec![release]),
            removals,
            remove_release: Mutex::new(Some(cleanup)),
            committed: Mutex::new(Vec::new()),
        },
        updates: received,
        removed,
        release: sent,
        cleanup: clean,
    }
}

#[test]
fn retirement_finishes_accepted_update_and_cleanup_before_authority_handoff() {
    runtime().unwrap().block_on(async {
        let Harness {
            service,
            mut updates,
            mut removed,
            release,
            cleanup,
        } = fixture();
        let shared = Mutex::new(Snapshot::default());
        let lifetime = TitlePresenceLifetime::new();
        let scenario = async {
            let original = updates.recv().await.unwrap();
            shared.lock().unwrap().xbox_presence = bridge::XboxPresenceState {
                in_world: true,
                game_mode: bridge::contract::PRESENCE_GAME_MODE_CREATIVE,
                ..Default::default()
            };
            lifetime.wake();
            lifetime.retire();
            assert_eq!(lifetime.retirement(), None);
            assert!(
                removed.try_recv().is_err(),
                "cleanup overtook the retained original write"
            );
            release.send(Ok(())).unwrap();
            removed.recv().await.unwrap();
            assert_eq!(service.committed.lock().unwrap().as_slice(), &[original]);
            assert_eq!(
                lifetime.retirement(),
                None,
                "unfinished removal released authority"
            );
            cleanup.send(Ok(())).unwrap();
        };
        tokio::join!(publish(&service, &shared, &lifetime), scenario);
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Complete)
        );
        assert!(
            updates.try_recv().is_err(),
            "retirement admitted a superseding write"
        );
    });
}

#[test]
fn changed_activity_is_coalesced_without_recreating_the_accepted_update() {
    runtime().unwrap().block_on(async {
        let Harness {
            service,
            mut updates,
            mut removed,
            release,
            cleanup,
        } = fixture();
        let shared = Mutex::new(Snapshot::default());
        let lifetime = TitlePresenceLifetime::new();
        let scenario = async {
            let original = updates.recv().await.unwrap();
            shared.lock().unwrap().xbox_presence.in_world = true;
            lifetime.wake();
            let latest = bridge::XboxPresenceState {
                in_world: true,
                experience: true,
                ..Default::default()
            };
            shared.lock().unwrap().xbox_presence = latest.clone();
            lifetime.wake();
            release.send(Ok(())).unwrap();
            assert_eq!(updates.recv().await.unwrap(), latest);
            lifetime.retire();
            removed.recv().await.unwrap();
            assert_eq!(
                service.committed.lock().unwrap().as_slice(),
                &[original, latest]
            );
            cleanup.send(Ok(())).unwrap();
        };
        tokio::join!(publish(&service, &shared, &lifetime), scenario);
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Complete)
        );
    });
}

#[test]
fn uncertain_cleanup_never_grants_a_replacement_lifetime() {
    runtime().unwrap().block_on(async {
        let Harness {
            service,
            mut updates,
            mut removed,
            release,
            cleanup,
        } = fixture();
        let shared = Mutex::new(Snapshot::default());
        let lifetime = TitlePresenceLifetime::new();
        let scenario = async {
            updates.recv().await.unwrap();
            lifetime.retire();
            release.send(Ok(())).unwrap();
            removed.recv().await.unwrap();
            cleanup
                .send(Err(io::Error::other("uncertain removal")))
                .unwrap();
        };
        tokio::join!(publish(&service, &shared, &lifetime), scenario);
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Unconfirmed)
        );
        lifetime.retire();
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Unconfirmed)
        );
    });
}

#[test]
fn expired_service_phase_cannot_poll_a_ready_write() {
    runtime().unwrap().block_on(async {
        let admitted = std::sync::atomic::AtomicBool::new(false);
        let ready = async {
            admitted.store(true, std::sync::atomic::Ordering::Relaxed);
        };
        let result = bounded_until(tokio::time::Instant::now(), ready).await;
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        assert!(!admitted.load(std::sync::atomic::Ordering::Relaxed));
    });
}

#[test]
fn uncertain_update_and_successful_removal_hold_original_authority() {
    runtime().unwrap().block_on(async {
        let Harness {
            service,
            mut updates,
            mut removed,
            release,
            cleanup,
        } = fixture();
        let shared = Mutex::new(Snapshot::default());
        let lifetime = TitlePresenceLifetime::new();
        let scenario = async {
            let original = updates.recv().await.unwrap();
            lifetime.retire();
            // The actual admitted service future may have committed before transport failed.
            release
                .send(Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "synthetic transport lost the update response",
                )))
                .unwrap();
            removed.recv().await.unwrap();
            assert_eq!(service.committed.lock().unwrap().as_slice(), &[original]);
            cleanup.send(Ok(())).unwrap();
        };
        tokio::join!(publish(&service, &shared, &lifetime), scenario);
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Unconfirmed),
            "successful DELETE erased a possibly applied original POST"
        );
    });
}

#[test]
fn uncertain_update_survives_later_successful_update_and_removal() {
    runtime().unwrap().block_on(async {
        let Harness {
            service,
            mut updates,
            mut removed,
            release,
            cleanup,
        } = fixture();
        let shared = Mutex::new(Snapshot::default());
        let lifetime = TitlePresenceLifetime::new();
        let scenario = async {
            let original = updates.recv().await.unwrap();
            let latest = bridge::XboxPresenceState {
                in_world: true,
                experience: true,
                ..Default::default()
            };
            shared.lock().unwrap().xbox_presence = latest.clone();
            lifetime.wake();
            release
                .send(Err(io::Error::new(
                    io::ErrorKind::ConnectionReset,
                    "synthetic transport lost the update response",
                )))
                .unwrap();
            assert_eq!(updates.recv().await.unwrap(), latest);
            lifetime.retire();
            removed.recv().await.unwrap();
            assert_eq!(
                service.committed.lock().unwrap().as_slice(),
                &[original, latest]
            );
            cleanup.send(Ok(())).unwrap();
        };
        tokio::join!(publish(&service, &shared, &lifetime), scenario);
        assert_eq!(
            lifetime.retirement(),
            Some(TitlePresenceRetirement::Unconfirmed),
            "later successful requests cannot order a prior uncertain POST remotely"
        );
    });
}
