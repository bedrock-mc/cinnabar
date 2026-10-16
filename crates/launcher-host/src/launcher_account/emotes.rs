//! Coalesced emote exchange on the account API, independent of slow launcher services.
use super::*;
use bridge::contract::{CinnabarEmoteState, CinnabarRemoteEmote};

#[derive(Default)]
pub(super) struct Exchange {
    session: Option<u64>,
    local: Option<CinnabarEmoteState>,
    remote: Vec<CinnabarRemoteEmote>,
    received: Option<Instant>,
    updated: Option<Instant>,
}

impl LauncherAccount {
    /// Replaces nearby subscriptions and clears old answers at every game-session boundary.
    pub fn set_cinnabar_emotes(&self, session: u64, state: CinnabarEmoteState) {
        publish(&self.snapshot, |snapshot| {
            let exchange = &mut snapshot.emotes;
            if exchange.session != Some(session) {
                exchange.remote.clear();
                exchange.received = None;
            }
            exchange.session = Some(session);
            exchange.local = Some(state);
            exchange.updated = Some(Instant::now());
        });
    }

    /// Returns current phases, expiring playback even if the account core stops answering.
    pub fn cinnabar_emotes(&self) -> Vec<CinnabarRemoteEmote> {
        self.with(|snapshot| {
            let age = snapshot.emotes.received.map_or(u64::MAX, |time| {
                u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX)
            });
            snapshot
                .emotes
                .remote
                .iter()
                .filter(|emote| age < emote.valid_for_ms)
                .map(|emote| {
                    let mut emote = emote.clone();
                    emote.elapsed_ms = emote.elapsed_ms.saturating_add(age);
                    emote.valid_for_ms -= age;
                    emote
                })
                .collect()
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
                    snapshot.emotes.session,
                    snapshot.emotes.local.clone().map(|mut local| {
                        if snapshot
                            .emotes
                            .updated
                            .is_none_or(|updated| updated.elapsed() > Duration::from_secs(5))
                        {
                            local.emote_id.clear();
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
                    bridge::sync_cinnabar_emotes(&dir, &local),
                )
                .await
            });
            if let Ok(Ok(remote)) = result {
                publish(&shared, |snapshot| {
                    if snapshot.auth_generation == generation && snapshot.emotes.session == session
                    {
                        snapshot.emotes.remote = remote;
                        snapshot.emotes.received = Some(Instant::now());
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
            snapshot.emotes.session = Some(1);
            snapshot.emotes.local = Some(CinnabarEmoteState::default());
            snapshot.emotes.updated = Some(Instant::now());
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
        assert_eq!(request["method"], bridge::contract::METHOD_CINNABAR_EMOTES);
        let response = serde_json::to_vec(&serde_json::json!({"jsonrpc":"2.0", "id":request["id"],
            "result":{"schema_version":1,"emotes":[{"xuid":"11","uuid":"00112233-4455-6677-8899-aabbccddeeff",
            "emote_id":"cinnabar:twerk","elapsed_ms":42,"valid_for_ms":15000}]}})).unwrap();
        connection
            .write_all(&(response.len() as u32).to_be_bytes())
            .unwrap();
        connection.write_all(&response).unwrap();
        test_time::eventually("published shared emote", || {
            shared.lock().unwrap().emotes.remote.len() == 1
        });
        drop(alive);
        worker.join().unwrap();
        std::fs::remove_dir_all(dir).unwrap();
    }
}
