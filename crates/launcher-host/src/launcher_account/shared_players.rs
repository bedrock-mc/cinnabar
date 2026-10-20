//! Coalesced nearby player exchange on the account API, independent of slow launcher services.
use super::*;
use bridge::contract::{CinnabarPlayerState, CinnabarPlayerUpdates};

const STALE_EXCHANGE_MS: u64 = 5_000;

#[derive(Default)]
pub(super) struct Exchange {
    session: Option<u64>,
    local: Option<CinnabarPlayerState>,
    remote: CinnabarPlayerUpdates,
    received: Option<Instant>,
    updated: Option<Instant>,
}

impl LauncherAccount {
    /// Replaces nearby subscriptions and clears old answers at every game-session boundary.
    pub fn set_shared_players(&self, session: u64, state: CinnabarPlayerState) {
        publish(&self.snapshot, |snapshot| {
            let exchange = &mut snapshot.shared_players;
            if exchange.session != Some(session) {
                exchange.remote = Default::default();
                exchange.received = None;
            }
            exchange.session = Some(session);
            exchange.local = Some(state);
            exchange.updated = Some(Instant::now());
        });
    }

    /// Returns current session loadouts and unexpired playback, including when the account core stops answering.
    pub fn shared_player_updates(&self) -> CinnabarPlayerUpdates {
        self.with(|snapshot| {
            let age = snapshot.shared_players.received.map_or(u64::MAX, |time| {
                u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX)
            });
            let remote = &snapshot.shared_players.remote;
            let ownership_current = age < STALE_EXCHANGE_MS
                && snapshot
                    .account
                    .as_ref()
                    .is_some_and(|account| matches!(account.state, CoreAuth::SignedIn));
            CinnabarPlayerUpdates {
                ownership_xuid: if ownership_current {
                    remote.ownership_xuid.clone()
                } else {
                    String::new()
                },
                owned_cosmetics: if ownership_current {
                    remote.owned_cosmetics.clone()
                } else {
                    Vec::new()
                },
                emotes: remote
                    .emotes
                    .iter()
                    .filter(|value| age < value.valid_for_ms)
                    .map(|value| {
                        let mut value = value.clone();
                        value.elapsed_ms = value.elapsed_ms.saturating_add(age);
                        value.valid_for_ms -= age;
                        value
                    })
                    .collect(),
                loadouts: if age < STALE_EXCHANGE_MS {
                    remote.loadouts.clone()
                } else {
                    Vec::new()
                },
            }
        })
    }
}

/// Polls the selected account owner; native unavailability withdraws its last published snapshot.
pub(super) fn start(
    dir: Option<PathBuf>,
    shared: Arc<Mutex<Snapshot>>,
    stop: Receiver<()>,
    runtime: Handle,
    tasks: TaskTracker,
) -> thread::JoinHandle<()> {
    thread::spawn(crate::worker::tracked(
        move || {
            loop {
                if !matches!(
                    stop.recv_timeout(Duration::from_millis(100)),
                    Err(crossbeam_channel::RecvTimeoutError::Timeout)
                ) {
                    return;
                }
                let (generation, source, session, local) = {
                    let snapshot = shared.lock().unwrap_or_else(|poison| poison.into_inner());
                    (
                        snapshot.auth_generation,
                        snapshot.account_source,
                        snapshot.shared_players.session,
                        snapshot.shared_players.local.clone().map(|mut local| {
                            if snapshot.shared_players.updated.is_none_or(|updated| {
                                updated.elapsed() > Duration::from_millis(STALE_EXCHANGE_MS)
                            }) {
                                local.emote_id.clear();
                                local.slots.clear();
                                local.xuids.clear();
                                local.uuids.clear();
                            }
                            local
                        }),
                    )
                };
                let Some(local) = local else { continue };
                let result = runtime.block_on(async {
                    #[cfg(all(feature = "native-networking", not(target_arch = "wasm32")))]
                    if source == AccountSource::Native {
                        let snapshot = shared.lock().unwrap_or_else(|p| p.into_inner());
                        if !snapshot.same_account_source(generation, source)
                            || snapshot.shared_players.session != session
                        {
                            return Ok(Ok(None));
                        }
                        let remote = match snapshot
                            .native_product_api
                            .as_ref()
                            .map(|api| api.sync(local))
                        {
                            Some(crate::product_api::PlayerSync::Current(remote)) => Some(remote),
                            Some(crate::product_api::PlayerSync::Pending) => None,
                            Some(crate::product_api::PlayerSync::Unavailable) | None => {
                                Some(CinnabarPlayerUpdates::default())
                            }
                        };
                        return Ok(Ok(remote));
                    }
                    tokio::time::timeout(Duration::from_secs(2), async {
                        bridge::sync_cinnabar_players(
                            super::workers::require_core_directory(dir.as_deref())?,
                            &local,
                        )
                        .await
                        .map(Some)
                    })
                    .await
                });
                if let Ok(Ok(Some(remote))) = result {
                    publish(&shared, |snapshot| {
                        if snapshot.same_account_source(generation, source)
                            && snapshot.shared_players.session == session
                        {
                            snapshot.shared_players.remote = remote;
                            snapshot.shared_players.received = Some(Instant::now());
                        }
                    });
                }
            }
        },
        &tasks,
    ))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;

    #[test]
    fn emote_worker_exchanges_state_on_the_supplied_tokio_runtime() {
        let dir =
            std::env::temp_dir().join(format!("cinnabar-emote-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let listener = UnixListener::bind(dir.join("control.sock")).unwrap();
        listener.set_nonblocking(true).unwrap();
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        {
            let mut snapshot = shared.lock().unwrap();
            snapshot.shared_players.session = Some(1);
            snapshot.shared_players.local = Some(CinnabarPlayerState::default());
            snapshot.shared_players.updated = Some(Instant::now());
        }
        let (alive, stop) = bounded(0);
        let runtime = worker_runtime();
        let worker = start(
            Some(dir.clone()),
            Arc::clone(&shared),
            stop,
            runtime.handle().clone(),
            TaskTracker::new(),
        );
        let mut connection = None;
        test_time::eventually("emote worker control request", || {
            connection = listener.accept().ok().map(|(stream, _)| stream);
            connection.is_some()
        });
        let mut connection = connection.unwrap();
        connection.set_nonblocking(false).unwrap();
        connection
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut size = [0; 4];
        connection.read_exact(&mut size).unwrap();
        let mut body = vec![0; u32::from_be_bytes(size) as usize];
        connection.read_exact(&mut body).unwrap();
        let request: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(request["method"], bridge::contract::METHOD_CINNABAR_PLAYERS);
        let response = serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0", "id":request["id"],
            "result":{"schema_version":1,"emotes":[{"xuid":"11","uuid":"00112233-4455-6677-8899-aabbccddeeff",
            "emote_id":"fixture:shared_emote","elapsed_ms":42,"valid_for_ms":15000}],"loadouts":[{"xuid":"11","uuid":"00112233-4455-6677-8899-aabbccddeeff","revision":1,"slots":[{"slot":"head","item_id":"cinnabar:bunny_ears"}]}]}})).unwrap();
        connection
            .write_all(&(response.len() as u32).to_be_bytes())
            .unwrap();
        connection.write_all(&response).unwrap();
        test_time::eventually("published shared emote", || {
            let snapshot = shared.lock().unwrap();
            snapshot.shared_players.remote.emotes.len() == 1
                && snapshot.shared_players.remote.loadouts.len() == 1
        });
        drop(alive);
        worker.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[cfg(all(test, feature = "native-networking", not(target_arch = "wasm32")))]
mod native_tests {
    use super::*;

    /// An unavailable native socket withdraws grants through the original worker without a Core directory.
    #[test]
    fn native_shared_player_worker_withdraws_disconnected_ownership() {
        let mut snapshot = Snapshot::native(None, "");
        snapshot.shared_players.session = Some(1);
        snapshot.shared_players.local = Some(CinnabarPlayerState::default());
        snapshot.shared_players.updated = Some(Instant::now());
        snapshot.shared_players.remote.ownership_xuid = "11".into();
        snapshot.shared_players.remote.owned_cosmetics = vec!["fixture:paid".into()];
        let shared = Arc::new(Mutex::new(snapshot));
        let (alive, stop) = bounded(0);
        let runtime = worker_runtime();
        let worker = start(
            None,
            Arc::clone(&shared),
            stop,
            runtime.handle().clone(),
            TaskTracker::new(),
        );
        test_time::eventually("native ownership withdrawal", || {
            shared.lock().unwrap().shared_players.received.is_some()
        });
        drop(alive);
        worker.join().unwrap();
        let snapshot = shared.lock().unwrap();
        assert!(snapshot.shared_players.remote.ownership_xuid.is_empty());
        assert!(snapshot.shared_players.remote.owned_cosmetics.is_empty());
    }
}
