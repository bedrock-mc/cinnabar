//! The invite screen's requests, the account's Xbox friends and the invites the player sends,
//! run in order on their own worker so neither waits on the polling workers.

use super::profile_worker::RESPONSE_TIMEOUT;
use super::*;
use tokio::sync::watch;

mod source;
pub(super) use source::Source;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Wake {
    Account,
    Stop,
}

/// The original request queue plus cancellation for its retained asynchronous operation.
pub(super) struct Requests {
    requests: Sender<Request>,
    wake: watch::Sender<Wake>,
}

struct Selection {
    generation: u64,
    source: Source,
    destination: Destination,
}

enum Destination {
    Core,
    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
    Native(Option<crate::native_account::HostedInvitation>),
}

impl Destination {
    /// Separates the actual hosting owner from the independently selected People reader.
    fn capture(snapshot: &Snapshot) -> Self {
        #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
        if snapshot
            .native_host_wake
            .as_ref()
            .is_some_and(|wake| matches!(&*wake.borrow(), super::native_host::Desired::Host(_)))
        {
            return Self::Native(snapshot.native_host_invitation.clone());
        }
        let _ = snapshot;
        Self::Core
    }

    /// Uses only the host selected when queued; an unavailable native incarnation never falls back.
    async fn send(&self, socket_dir: Option<&std::path::Path>, xuid: &str) -> Result<(), ()> {
        match self {
            Self::Core => bridge::invite_to_world(
                super::workers::require_core_directory(socket_dir).map_err(|_| ())?,
                xuid,
            )
            .await
            .map_err(|_| ()),
            #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
            Self::Native(host) => host
                .as_ref()
                .ok_or(())?
                .send(xuid, Instant::now() + RESPONSE_TIMEOUT)
                .await
                .map_err(|_| ()),
        }
    }
}

impl Selection {
    /// Captures both the UI generation and the concrete reader when the request is queued.
    fn capture(snapshot: &Snapshot) -> Self {
        Self {
            generation: snapshot.auth_generation,
            source: snapshot.people_source.clone(),
            destination: Destination::capture(snapshot),
        }
    }

    /// Rejects replaced account requests before dispatch and after artwork.
    fn current(&self, snapshot: &Snapshot) -> bool {
        self.generation == snapshot.auth_generation
            && self.source.same_account(&snapshot.people_source)
    }
}

impl Requests {
    /// A closed queue for account-state tests that never start the invitation worker.
    #[cfg(test)]
    pub(super) fn disconnected() -> Self {
        Self {
            requests: crossbeam_channel::unbounded().0,
            wake: watch::channel(Wake::Stop).0,
        }
    }

    /// Queues the selected owner rather than resolving a later account at worker dispatch.
    pub(super) fn people(&self, shared: &Mutex<Snapshot>) {
        let selected = Selection::capture(&shared.lock().unwrap_or_else(|p| p.into_inner()));
        let _ = self.requests.send(Request::People(selected));
    }

    /// Captures the original active host capability and requesting account generation.
    pub(super) fn send(&self, shared: &Mutex<Snapshot>, xuids: Vec<String>) {
        let selected = Selection::capture(&shared.lock().unwrap_or_else(|p| p.into_inner()));
        let _ = self.requests.send(Request::Send(selected, xuids));
    }
}

impl Drop for Requests {
    /// Cancels the actual in-flight read/RPC before its tracked worker leaves the app runtime.
    fn drop(&mut self) {
        self.wake.send_replace(Wake::Stop);
    }
}

/// Coalesces account retirement without reviving a stopped request worker.
pub(super) fn retire(sender: &watch::Sender<Wake>) {
    sender.send_if_modified(|wake| *wake != Wake::Stop);
}

enum Request {
    People(Selection),
    Send(Selection, Vec<String>),
}

/// Starts the worker; dropping the account link stops it and drops what is still queued.
pub(super) fn start(
    socket_dir: Option<PathBuf>,
    artwork_dir: PathBuf,
    shared: Arc<Mutex<Snapshot>>,
    stop: Receiver<()>,
    runtime: Handle,
    tasks: TaskTracker,
) -> Requests {
    let (sender, requests) = crossbeam_channel::unbounded();
    let (wake, mut changed) = watch::channel(Wake::Account);
    shared.lock().unwrap_or_else(|p| p.into_inner()).people_wake = Some(wake.clone());
    thread::spawn(crate::worker::tracked(
        move || {
            let images = artwork::people_images(&artwork_dir);
            loop {
                let request = crossbeam_channel::select_biased! {
                    recv(stop) -> _ => return,
                    recv(requests) -> request => match request {
                        Ok(request) => request,
                        Err(_) => return,
                    },
                };
                match request {
                    Request::People(selected) => {
                        let people = runtime.block_on(people(
                            socket_dir.as_deref(),
                            &images,
                            &shared,
                            &selected,
                            &mut changed,
                        ));
                        let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
                        if *changed.borrow() != Wake::Stop && selected.current(&snapshot) {
                            snapshot.people = Some(people);
                        }
                    }
                    Request::Send(selected, xuids) => runtime.block_on(send(
                        socket_dir.as_deref(),
                        &xuids,
                        &shared,
                        &selected,
                        &mut changed,
                    )),
                }
                if *changed.borrow() == Wake::Stop {
                    return;
                }
            }
        },
        &tasks,
    ));
    Requests {
        requests: sender,
        wake,
    }
}

/// Keeps one actual fetch and one actual artwork future through unrelated wakeups.
async fn people(
    socket_dir: Option<&std::path::Path>,
    images: &crate::remote_images::ImageDirectory,
    shared: &Mutex<Snapshot>,
    selected: &Selection,
    changed: &mut watch::Receiver<Wake>,
) -> Result<Vec<bridge::Person>, ()> {
    let loaded = wait(
        tokio::time::timeout(RESPONSE_TIMEOUT, selected.source.fetch(socket_dir)),
        shared,
        selected,
        changed,
    )
    .await?;
    let mut loaded = match loaded {
        Ok(Ok(people)) => people,
        Ok(Err(())) => {
            tracing::warn!("friends list unavailable");
            return Err(());
        }
        Err(_) => {
            tracing::warn!("friends list timed out");
            return Err(());
        }
    };
    wait(
        artwork::fill(images, artwork::people_slots(loaded.people_mut())),
        shared,
        selected,
        changed,
    )
    .await?;
    wait(loaded.into_current(), shared, selected, changed).await?
}

/// Retains the pinned phase until completion, account retirement or terminal worker stop.
async fn wait<T>(
    phase: impl std::future::Future<Output = T>,
    shared: &Mutex<Snapshot>,
    selected: &Selection,
    changed: &mut watch::Receiver<Wake>,
) -> Result<T, ()> {
    tokio::pin!(phase);
    loop {
        let stopped = *changed.borrow_and_update() == Wake::Stop;
        if stopped || !selected.current(&shared.lock().unwrap_or_else(|p| p.into_inner())) {
            return Err(());
        }
        tokio::select! { biased;
            result = changed.changed() => if result.is_err() { return Err(()); },
            result = &mut phase => return Ok(result),
        }
    }
}

/// Invites through the captured selected owner; native commands never resolve a replacement Go host.
async fn send(
    socket_dir: Option<&std::path::Path>,
    xuids: &[String],
    shared: &Mutex<Snapshot>,
    selected: &Selection,
    changed: &mut watch::Receiver<Wake>,
) {
    for xuid in xuids {
        let sent = wait(
            tokio::time::timeout(
                RESPONSE_TIMEOUT,
                selected.destination.send(socket_dir, xuid),
            ),
            shared,
            selected,
            changed,
        )
        .await;
        match sent {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(_))) => tracing::warn!("game invite not sent"),
            Ok(Err(_)) => tracing::warn!("game invite timed out"),
            Err(()) => return,
        }
    }
}

#[cfg(test)]
mod tests;
