//! The explicitly selected server warms independently of account polling and Login.

use super::{Arc, Mutex, PathBuf, Receiver, Sender, launcher_control, thread};

pub(super) struct ServerPreparation {
    selected: Option<String>,
    pending: Arc<Mutex<Option<String>>>,
    wake: Sender<()>,
}

impl ServerPreparation {
    pub(super) fn start(socket_dir: PathBuf, stop: Receiver<()>) -> Self {
        let (wake, requests) = crossbeam_channel::bounded(1);
        let pending = Arc::new(Mutex::new(None));
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
                bevy::log::debug!(
                    selected = target.is_some(),
                    succeeded,
                    elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
                    "selected server preparation",
                );
            });
        });
        Self {
            selected: None,
            pending,
            wake,
        }
    }

    pub(super) fn select(&mut self, address: Option<&str>) {
        if self.selected.as_deref() == address {
            return;
        }
        self.selected = address.map(str::to_owned);
        *self
            .pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = self.selected.clone();
        let _ = self.wake.try_send(());
    }

    #[cfg(test)]
    pub(super) fn disconnected() -> Self {
        Self {
            selected: None,
            pending: Arc::new(Mutex::new(None)),
            wake: crossbeam_channel::bounded(1).0,
        }
    }
}

fn run(
    requests: &Receiver<()>,
    stop: &Receiver<()>,
    pending: &Mutex<Option<String>>,
    mut prepare: impl FnMut(Option<&str>),
) {
    let mut previous = None;
    loop {
        crossbeam_channel::select_biased! {
            recv(stop) -> _ => return,
            recv(requests) -> request => if request.is_err() { return; },
        }
        while requests.try_recv().is_ok() {}
        if matches!(
            stop.try_recv(),
            Err(crossbeam_channel::TryRecvError::Disconnected)
        ) {
            return;
        }
        let selected = pending
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        if previous.as_ref() == Some(&selected) {
            continue;
        }
        prepare(selected.as_deref());
        previous = Some(selected);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn unchanged_selected_server_allocates_no_work() {
        let mut preparation = ServerPreparation::disconnected();
        preparation.select(Some("selected.test"));
        let before = crate::tests::alloc_count::thread_allocations();
        for _ in 0..32 {
            preparation.select(Some("selected.test"));
        }
        assert_eq!(crate::tests::alloc_count::thread_allocations(), before);
    }

    #[test]
    fn pending_selection_coalesces_and_cancels_without_repeating_the_old_target() {
        let (wake, requests) = crossbeam_channel::bounded(1);
        let pending = Arc::new(Mutex::new(None));
        let mut preparation = ServerPreparation {
            selected: None,
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
            })
        });
        preparation.select(Some("a.test"));
        assert_eq!(
            observed
                .recv_timeout(Duration::from_secs(2))
                .unwrap()
                .as_deref(),
            Some("a.test")
        );
        preparation.select(Some("a.test"));
        preparation.select(Some("b.test"));
        preparation.select(Some("c.test"));
        preparation.select(None);
        release.send(()).unwrap();
        assert_eq!(observed.recv_timeout(Duration::from_secs(2)).unwrap(), None);
        drop(alive);
        worker.join().unwrap();
        assert!(observed.try_recv().is_err());
    }
}
