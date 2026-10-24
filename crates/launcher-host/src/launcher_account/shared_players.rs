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

    /// Observes the same read boundary over actual acquired Product API data; grants are never fabricated.
    #[cfg(test)]
    pub(crate) fn observed_product_updates(
        api: crate::product_api::Client,
        remote: CinnabarPlayerUpdates,
        stale: bool,
    ) -> CinnabarPlayerUpdates {
        project(&Snapshot {
            native_product_api: Some(api),
            shared_players: Exchange {
                remote,
                received: Instant::now().checked_sub(if stale {
                    Duration::from_millis(STALE_EXCHANGE_MS)
                } else {
                    Duration::ZERO
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    /// Returns current session loadouts and unexpired playback, including when the account API is unavailable.
    pub fn shared_player_updates(&self) -> CinnabarPlayerUpdates {
        self.with(|snapshot| project(snapshot))
    }
}

/// Projects the original retained exchange without consuming or replacing service data.
fn project(snapshot: &Snapshot) -> CinnabarPlayerUpdates {
    let age = snapshot.shared_players.received.map_or(u64::MAX, |time| {
        u64::try_from(time.elapsed().as_millis()).unwrap_or(u64::MAX)
    });
    let remote = &snapshot.shared_players.remote;
    let ownership_current = age < STALE_EXCHANGE_MS
        && snapshot
            .native_product_api
            .as_ref()
            .is_some_and(|api| api.ownership_current(&remote.ownership_xuid));
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
}

/// Polls the selected account owner; native unavailability withdraws its last published snapshot.
pub(super) fn start(
    shared: Arc<Mutex<Snapshot>>,
    stop: Receiver<()>,
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
                let (generation, session, local, api) = {
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
                        snapshot.native_product_api.clone(),
                    )
                };
                let Some(local) = local else { continue };
                let result = match api.as_ref().map(|api| api.sync(local)) {
                    Some(crate::product_api::PlayerSync::Current(remote)) => Some(remote),
                    Some(crate::product_api::PlayerSync::Pending) => None,
                    Some(crate::product_api::PlayerSync::Unavailable) | None => {
                        Some(CinnabarPlayerUpdates::default())
                    }
                };
                if let Some(remote) = result {
                    publish(&shared, |snapshot| {
                        if snapshot.same_generation(generation)
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

#[cfg(all(test, feature = "native-networking", not(target_arch = "wasm32")))]
mod native_tests {
    use super::*;

    /// An unavailable native socket withdraws grants through the original worker through its original worker.
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
        let worker = start(Arc::clone(&shared), stop, TaskTracker::new());
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
