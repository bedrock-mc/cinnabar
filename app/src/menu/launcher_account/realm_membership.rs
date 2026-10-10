//! One cancellable invitation request at a time, independent of catalog polling.

use {super::*, launcher::menu::view::MenuRealmCard};

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
                    Ok(Ok(result)) => Ok(result),
                    _ => Err(()),
                };
                publish_account(&shared, generation, |snapshot| {
                    let result = result.map(|result| {
                        if accept {
                            snapshot.realm_catalog_revision = snapshot.realm_catalog_revision.wrapping_add(1);
                            let realms = snapshot.realms.get_or_insert_with(Vec::new);
                            if let Some(existing) = realms.iter_mut().find(|old| old.target == result.realm.target) {
                                *existing = result.realm.clone();
                            } else {
                                realms.push(result.realm.clone());
                            }
                            if let Some(wake) = &snapshot.catalog_wake { let _ = wake.try_send(()); }
                        }
                        (result.code, realm_card(&result.realm))
                    });
                    snapshot.realm_membership = Some((ticket, accept, result));
                });
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
    pub(super) fn realm_membership_control(&mut self) -> Option<RealmMembershipResponse> {
        self.with(|snapshot| snapshot.realm_membership.take())
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        os::unix::net::UnixListener,
    };

    /// Waits for an asynchronous answer, with a deadline only to bound fixture failures.
    fn answer(shared: &Arc<Mutex<Snapshot>>) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while shared.lock().unwrap().realm_membership.is_none() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        assert!(shared.lock().unwrap().realm_membership.is_some());
    }

    #[test]
    fn accepted_membership_survives_the_account_catalog_snapshot() {
        let dir = std::env::temp_dir().join(format!("realm-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join("control.sock")).unwrap();
        let fixture = thread::spawn(move || {
            for accept in [false, true] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut size = [0; 4];
                stream.read_exact(&mut size).unwrap();
                let mut bytes = vec![0; u32::from_be_bytes(size) as usize];
                stream.read_exact(&mut bytes).unwrap();
                let request: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(request["method"], "realm_membership.v1");
                assert_eq!(request["params"]["accept"], accept);
                let response = serde_json::to_vec(&serde_json::json!({
                    "jsonrpc":"2.0", "id":request["id"], "result":{
                        "schema_version":1, "code":"invitation", "realm":{
                            "name":"Accepted Realm", "state":"OPEN", "target":"realm_id/7", "member":true
                        }
                    }
                })).unwrap();
                stream
                    .write_all(&(response.len() as u32).to_be_bytes())
                    .unwrap();
                stream.write_all(&response).unwrap();
            }
        });
        let shared = Arc::new(Mutex::new(Snapshot {
            realms: Some(Vec::new()),
            ..Default::default()
        }));
        let requests = start(dir.clone(), Arc::clone(&shared));
        requests
            .send(Request::Run {
                ticket: 1,
                code: "invitation".into(),
                accept: false,
            })
            .unwrap();
        answer(&shared);
        {
            let mut snapshot = shared.lock().unwrap();
            assert!(
                snapshot.realms.as_ref().unwrap().is_empty(),
                "preview must not grant membership"
            );
            snapshot.realm_membership = None;
        }
        requests
            .send(Request::Run {
                ticket: 2,
                code: "invitation".into(),
                accept: true,
            })
            .unwrap();
        answer(&shared);
        {
            let snapshot = shared.lock().unwrap();
            let realms = snapshot.realms.as_ref().unwrap();
            assert_eq!(
                realms.len(),
                1,
                "catalog polling must retain the accepted Realm on subsequent frames"
            );
            assert_eq!(realms[0].target, "realm_id/7");
        }
        drop(requests);
        fixture.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
