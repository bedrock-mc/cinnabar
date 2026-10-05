//! The visible selected server warms independently of account polling and Login.

use super::{Arc, Mutex, PathBuf, Receiver, Sender, launcher_control, thread};
use std::time::Duration;

const RPC_RETRY_MAX: Duration = Duration::from_secs(8);

#[derive(Clone, Default, Eq, PartialEq)]
struct Selection {
    address: Option<String>,
    account_generation: Option<u64>,
    joining: bool,
    revision: u64,
}

pub(super) struct ServerPreparation {
    selected: Selection,
    pending: Arc<Mutex<Selection>>,
    wake: Sender<()>,
}

impl ServerPreparation {
    pub(super) fn start(socket_dir: PathBuf, stop: Receiver<()>) -> Self {
        let (wake, requests) = crossbeam_channel::bounded(1);
        let pending = Arc::new(Mutex::new(Selection::default()));
        let shared = pending.clone();
        thread::spawn(move || {
            let Some(runtime) = super::runtime() else {
                return;
            };
            run(&requests, &stop, &shared, |address| {
                let target = address.map(super::super::launcher_core::target_for);
                let started = std::time::Instant::now();
                let succeeded = matches!(
                    runtime.block_on(async {
                        tokio::time::timeout(
                            super::super::launcher_core::SELECT_TIMEOUT,
                            launcher_control::prepare_connect_target(&socket_dir, target.as_ref()),
                        )
                        .await
                    }),
                    Ok(Ok(()))
                );
                if succeeded {
                    bevy::log::info!(
                        selected = target.is_some(),
                        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
                        "selected server preparation accepted",
                    );
                } else {
                    bevy::log::debug!(
                        selected = target.is_some(),
                        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
                        "selected server preparation deferred",
                    );
                }
                succeeded
            });
        });
        Self {
            selected: Selection::default(),
            pending,
            wake,
        }
    }

    pub(super) fn select(&mut self, address: Option<&str>, account_generation: Option<u64>) {
        if self.selected.address.as_deref() == address
            && self.selected.account_generation == account_generation
        {
            return;
        }
        self.selected.address = address.map(str::to_owned);
        self.selected.account_generation = account_generation;
        self.publish();
    }

    pub(super) fn set_joining(&mut self, joining: bool) {
        if self.selected.joining == joining {
            return;
        }
        self.selected.joining = joining;
        if joining {
            self.selected.revision = self.selected.revision.wrapping_add(1);
        }
        self.publish();
    }

    fn publish(&self) {
        *self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = self.selected.clone();
        let _ = self.wake.try_send(());
    }

    #[cfg(test)]
    pub(super) fn disconnected() -> Self {
        Self {
            selected: Selection::default(),
            pending: Arc::new(Mutex::new(Selection::default())),
            wake: crossbeam_channel::bounded(1).0,
        }
    }
}

fn run(
    requests: &Receiver<()>,
    stop: &Receiver<()>,
    pending: &Mutex<Selection>,
    prepare: impl FnMut(Option<&str>) -> bool,
) {
    run_with_retry(requests, stop, pending, crossbeam_channel::after, prepare);
}

fn run_with_retry(
    requests: &Receiver<()>,
    stop: &Receiver<()>,
    pending: &Mutex<Selection>,
    mut retry_after: impl FnMut(Duration) -> Receiver<std::time::Instant>,
    mut prepare: impl FnMut(Option<&str>) -> bool,
) {
    let mut previous = None;
    let mut attempted = None;
    let mut retry = crossbeam_channel::never();
    let mut retry_delay = super::EVENT_INTERVAL;
    loop {
        crossbeam_channel::select_biased! {
            recv(stop) -> _ => return,
            recv(requests) -> request => if request.is_err() { return; },
            recv(retry) -> _ => {},
        }
        while requests.try_recv().is_ok() {}
        if matches!(
            stop.try_recv(),
            Ok(()) | Err(crossbeam_channel::TryRecvError::Disconnected)
        ) {
            return;
        }
        let selected = pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        retry = crossbeam_channel::never();
        if selected.joining {
            previous = None;
            retry_delay = super::EVENT_INTERVAL;
            continue;
        }
        if previous.as_ref() == Some(&selected) {
            continue;
        }
        if attempted.as_ref() != Some(&selected) {
            retry_delay = super::EVENT_INTERVAL;
            attempted = Some(selected.clone());
        }
        previous = None;
        if prepare(selected.address.as_deref()) {
            previous = Some(selected);
            retry_delay = super::EVENT_INTERVAL;
        } else {
            retry = retry_after(retry_delay);
            retry_delay = (retry_delay * 2).min(RPC_RETRY_MAX);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn unchanged_selected_server_allocates_no_work() {
        let mut preparation = ServerPreparation::disconnected();
        preparation.select(Some("selected.test"), Some(1));
        let before = crate::tests::alloc_count::thread_allocations();
        for _ in 0..32 {
            preparation.select(Some("selected.test"), Some(1));
        }
        assert_eq!(crate::tests::alloc_count::thread_allocations(), before);
    }

    #[test]
    fn pending_selection_coalesces_and_cancels_without_repeating_the_old_target() {
        let (wake, requests) = crossbeam_channel::bounded(1);
        let pending = Arc::new(Mutex::new(Selection::default()));
        let mut preparation = ServerPreparation {
            selected: Selection::default(),
            pending: pending.clone(),
            wake,
        };
        let (alive, stop) = crossbeam_channel::bounded(0);
        let (entered, observed) = crossbeam_channel::unbounded();
        let (release, gate) = crossbeam_channel::bounded(0);
        let worker = thread::spawn(move || {
            run(&requests, &stop, &pending, |target| {
                entered.send(target.map(str::to_owned)).unwrap();
                if target == Some("a.test") {
                    let _ = gate.recv();
                }
                true
            })
        });
        preparation.select(Some("a.test"), Some(1));
        assert_eq!(
            observed
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .as_deref(),
            Some("a.test")
        );
        preparation.select(Some("a.test"), Some(1));
        preparation.select(Some("b.test"), Some(1));
        preparation.select(Some("c.test"), Some(1));
        preparation.select(None, Some(1));
        release.send(()).unwrap();
        assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), None);
        drop(alive);
        worker.join().unwrap();
        assert!(observed.try_recv().is_err());
    }

    #[test]
    fn failed_preparation_can_retry_the_unchanged_selection() {
        let (wake, requests) = crossbeam_channel::bounded(1);
        let pending = Arc::new(Mutex::new(Selection {
            address: Some("selected.test".to_owned()),
            ..Default::default()
        }));
        let (alive, stop) = crossbeam_channel::bounded(0);
        let (attempt, observed) = crossbeam_channel::unbounded();
        let worker = thread::spawn(move || {
            let mut attempts = 0;
            run(&requests, &stop, &pending, |target| {
                attempts += 1;
                attempt.send((attempts, target.map(str::to_owned))).unwrap();
                attempts > 1
            });
        });
        wake.send(()).unwrap();
        let first = observed.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(first, (1, Some("selected.test".into())));
        wake.send(()).unwrap();
        let retried = observed.recv_timeout(Duration::from_secs(2));
        drop(alive);
        worker.join().unwrap();
        assert_eq!(retried.unwrap(), (2, Some("selected.test".into())));
    }

    #[test]
    fn failed_rpc_retries_without_another_menu_request() {
        let (wake, requests) = crossbeam_channel::bounded(1);
        let pending = Arc::new(Mutex::new(Selection {
            address: Some("selected.test".to_owned()),
            ..Default::default()
        }));
        let (alive, stop) = crossbeam_channel::bounded(0);
        let (tick, retry) = crossbeam_channel::unbounded();
        let (scheduled, waiting) = crossbeam_channel::unbounded();
        let (attempt, observed) = crossbeam_channel::unbounded();
        let worker = thread::spawn(move || {
            let mut attempts = 0;
            run_with_retry(
                &requests,
                &stop,
                &pending,
                |_| {
                    scheduled.send(()).unwrap();
                    retry.clone()
                },
                |target| {
                    attempts += 1;
                    attempt.send(target.map(str::to_owned)).unwrap();
                    attempts > 1
                },
            );
        });
        wake.send(()).unwrap();
        assert_eq!(
            observed
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .as_deref(),
            Some("selected.test")
        );
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        tick.send(std::time::Instant::now()).unwrap();
        let retried = observed.recv_timeout(Duration::from_secs(2));
        drop(alive);
        worker.join().unwrap();
        assert_eq!(retried.unwrap().as_deref(), Some("selected.test"));
        assert!(
            waiting.try_recv().is_err(),
            "success stops background retries"
        );
    }

    #[test]
    fn account_readiness_retries_immediately_and_coalesced_join_completion_rearms() {
        let (wake, requests) = crossbeam_channel::bounded(1);
        let pending = Arc::new(Mutex::new(Selection::default()));
        let mut preparation = ServerPreparation {
            selected: Selection::default(),
            pending: pending.clone(),
            wake,
        };
        let (alive, stop) = crossbeam_channel::bounded(0);
        let (scheduled, waiting) = crossbeam_channel::unbounded();
        let (attempt, observed) = crossbeam_channel::unbounded();
        let (release, gate) = crossbeam_channel::bounded(0);
        let worker = thread::spawn(move || {
            let mut attempts = 0;
            run_with_retry(
                &requests,
                &stop,
                &pending,
                |_| {
                    scheduled.send(()).unwrap();
                    crossbeam_channel::never()
                },
                |target| {
                    attempts += 1;
                    attempt.send((attempts, target.map(str::to_owned))).unwrap();
                    if attempts == 2 {
                        gate.recv().unwrap();
                    }
                    attempts > 1
                },
            );
        });
        preparation.select(Some("selected.test"), None);
        assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap().0, 1);
        waiting.recv_timeout(Duration::from_secs(2)).unwrap();
        preparation.select(Some("selected.test"), Some(1));
        assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap().0, 2);
        preparation.set_joining(true);
        preparation.set_joining(false);
        release.send(()).unwrap();
        let rearmed = observed.recv_timeout(Duration::from_secs(2));
        drop(alive);
        worker.join().unwrap();
        assert_eq!(rearmed.unwrap(), (3, Some("selected.test".into())));
        assert!(waiting.try_recv().is_err());
    }
}
