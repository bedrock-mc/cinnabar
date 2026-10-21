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

/// Polls only loopback control; the core coalesces WebSocket writes and bounds received leases.
pub(super) fn start(
    dir: PathBuf,
    shared: Arc<Mutex<Snapshot>>,
    stop: Receiver<()>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let Some(runtime) = runtime() else { return };
        loop {
            if !matches!(
                stop.recv_timeout(Duration::from_millis(100)),
                Err(crossbeam_channel::RecvTimeoutError::Timeout)
            ) {
                return;
            }
            let (generation, session, local) = {
                let snapshot = shared.lock().unwrap_or_else(|poison| poison.into_inner());
                (
                    snapshot.auth_generation,
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
                tokio::time::timeout(
                    Duration::from_secs(2),
                    bridge::sync_cinnabar_players(&dir, &local),
                )
                .await
            });
            if let Ok(Ok(remote)) = result {
                publish(&shared, |snapshot| {
                    if snapshot.auth_generation == generation
                        && snapshot.shared_players.session == session
                    {
                        snapshot.shared_players.remote = remote;
                        snapshot.shared_players.received = Some(Instant::now());
                    }
                });
            }
        }
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;

    #[test]
    fn emote_worker_exchanges_state_on_its_own_tokio_runtime() {
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
        let worker = start(dir.clone(), Arc::clone(&shared), stop);
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
