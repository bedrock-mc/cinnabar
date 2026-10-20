//! Selected-account Realm invitation requests on the existing application worker.

use tokio::sync::watch;
use {super::*, launcher::menu::view::MenuRealmCard};

mod source;
mod worker;
pub(super) use source::Source;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Wake {
    Account,
    Stop,
}

/// The original queue and cancellation signal for its single retained operation.
pub(super) struct Requests {
    requests: tokio::sync::mpsc::UnboundedSender<Request>,
    wake: watch::Sender<Wake>,
}

struct Selection {
    generation: u64,
    source: Source,
    ticket: u64,
}

impl Selection {
    /// Captures the concrete requesting owner before a later queued account change.
    fn capture(snapshot: &Snapshot, ticket: u64) -> Self {
        Self {
            generation: snapshot.auth_generation,
            source: snapshot.realm_source.clone(),
            ticket,
        }
    }

    /// Rejects replaced UI/account selections before dispatch and publication.
    fn current(&self, snapshot: &Snapshot) -> bool {
        self.generation == snapshot.auth_generation
            && self.source.same_account(&snapshot.realm_source)
            && snapshot.realm_ticket == Some(self.ticket)
    }
}

enum Request {
    Run {
        ticket: u64,
        code: String,
        accept: bool,
        selected: Selection,
    },
    Cancel,
}

impl Requests {
    /// Queues the original selected account rather than choosing an owner on later dispatch.
    fn run(&self, shared: &Mutex<Snapshot>, ticket: u64, code: String, accept: bool) -> bool {
        let selected = {
            let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
            snapshot.realm_ticket = Some(ticket);
            Selection::capture(&snapshot, ticket)
        };
        self.requests
            .send(Request::Run {
                ticket,
                code,
                accept,
                selected,
            })
            .is_ok()
    }

    /// Retires the visible intent; a possible native POST keeps its bounded observation.
    fn cancel(&self, shared: &Mutex<Snapshot>) {
        let mut snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
        snapshot.realm_ticket = None;
        snapshot.realm_membership = None;
        let _ = self.requests.send(Request::Cancel);
    }

    /// Supplies account-state fixtures that never start a membership worker.
    #[cfg(test)]
    pub(super) fn disconnected() -> Self {
        Self {
            requests: tokio::sync::mpsc::unbounded_channel().0,
            wake: watch::channel(Wake::Stop).0,
        }
    }
}

impl Drop for Requests {
    /// Stops previews immediately and suppresses publication from an already submitted acceptance.
    fn drop(&mut self) {
        self.wake.send_replace(Wake::Stop);
    }
}

/// Wakes the existing worker without reviving it after terminal shutdown.
pub(super) fn retire(sender: &watch::Sender<Wake>) {
    sender.send_if_modified(|wake| *wake != Wake::Stop);
}

/// Starts exactly the original tracked membership worker on the shared application reactor.
pub(super) fn start(
    socket_dir: Option<PathBuf>,
    shared: Arc<Mutex<Snapshot>>,
    runtime: Handle,
    tasks: TaskTracker,
) -> Requests {
    let (requests, receiver) = tokio::sync::mpsc::unbounded_channel();
    let (wake, changed) = watch::channel(Wake::Account);
    shared.lock().unwrap_or_else(|p| p.into_inner()).realm_wake = Some(wake.clone());
    thread::spawn(crate::worker::tracked(
        move || {
            runtime.block_on(worker::run(socket_dir, shared, receiver, changed));
        },
        &tasks,
    ));
    Requests { requests, wake }
}

/// Maps the core's verified catalog entry for the menu, without selecting a transport.
pub(super) fn realm_card(realm: &Realm) -> MenuRealmCard {
    MenuRealmCard {
        name: realm.name.clone(),
        state: realm.state.clone(),
        target: realm.target.clone(),
        address: realm.address.clone().unwrap_or_default(),
        owner: realm.owner.clone(),
        online_players: realm.online_players,
        max_players: realm.max_players,
        days_left: realm.days_left,
        expired: realm.expired,
        member: realm.member,
    }
}

impl LauncherAccount {
    /// Queues one preview or acceptance request for the account worker.
    pub(super) fn request_realm_membership_control(
        &mut self,
        ticket: u64,
        code: String,
        accept: bool,
    ) -> bool {
        self.realm_membership
            .run(&self.snapshot, ticket, code, accept)
    }

    /// Cancels the outstanding request and retires its unpublished response.
    pub(super) fn cancel_realm_membership_control(&mut self) {
        self.realm_membership.cancel(&self.snapshot);
    }

    /// Takes the latest response once, leaving stale-ticket rejection to the menu.
    pub(super) fn realm_membership_control(&mut self) -> Option<RealmMembershipResponse> {
        self.with(|snapshot| snapshot.realm_membership.take())
    }
}

#[cfg(all(test, unix))]
mod tests;
