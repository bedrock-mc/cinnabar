//! One cancellable invitation request at a time, independent of catalog polling.

use super::*;

/// Commands from the launcher frame to its asynchronous membership worker.
pub(super) enum Request {
    Run {
        ticket: u64,
        code: String,
        accept: bool,
    },
    Cancel,
}

/// Runs requests on a dedicated runtime; cancelling or dropping the sender closes the RPC stream.
pub(super) fn start(
    socket_dir: PathBuf,
    shared: Arc<Mutex<Snapshot>>,
) -> tokio::sync::mpsc::UnboundedSender<Request> {
    let (sender, mut requests) = tokio::sync::mpsc::unbounded_channel();
    thread::spawn(move || {
        let Some(runtime) = runtime() else {
            while let Some(request) = requests.blocking_recv() {
                if let Request::Run { ticket, accept, .. } = request {
                    publish(&shared, |snapshot| {
                        snapshot.realm_membership = Some((ticket, accept, Err(())))
                    });
                }
            }
            return;
        };
        runtime.block_on(async {
            let mut pending = None;
            loop {
                let request = match pending.take() {
                    Some(request) => request,
                    None => match requests.recv().await { Some(request) => request, None => return },
                };
                let Request::Run { ticket, code, accept } = request else { continue; };
                let generation = auth_generation(&shared);
                let result = tokio::select! {
                    biased;
                    command = requests.recv() => {
                        match command { None => return, Some(Request::Cancel) => {}, Some(request) => pending = Some(request) }
                        continue;
                    }
                    result = tokio::time::timeout(profile_worker::RESPONSE_TIMEOUT, launcher_control::realm_membership(&socket_dir, &code, accept)) => result,
                };
                let result = match result {
                    Ok(Ok(result)) => Ok((result.code, realm_card(&result.realm))),
                    _ => Err(()),
                };
                publish_account(&shared, generation, |snapshot| snapshot.realm_membership = Some((ticket, accept, result)));
            }
        });
    });
    sender
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
            .send(realm_membership::Request::Run {
                ticket,
                code,
                accept,
            })
            .is_ok()
    }

    /// Cancels the outstanding request and retires its unpublished response.
    pub(super) fn cancel_realm_membership_control(&mut self) {
        let _ = self
            .realm_membership
            .send(realm_membership::Request::Cancel);
        self.with(|snapshot| snapshot.realm_membership = None);
    }

    /// Takes the latest response once, leaving stale-ticket rejection to the menu.
    pub(super) fn realm_membership_control(
        &mut self,
    ) -> Option<(u64, bool, Result<(String, MenuRealmCard), ()>)> {
        self.with(|snapshot| snapshot.realm_membership.take())
    }
}
