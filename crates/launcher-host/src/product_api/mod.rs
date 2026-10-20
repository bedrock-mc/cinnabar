//! Optional Cinnabar product socket, owned by the application's existing account event worker.

mod handle;
mod players;
mod policy;
mod session;
mod socket;
#[cfg(test)]
mod tests;
mod wire;

use crate::native_account::ApiScope;
use bridge::{BridgeError, CinnabarApiConfig, CinnabarLookup, CinnabarRelease, contract::*};
use players::Players;
use policy::{Budget, Parameters, REQUEST_TIMEOUT};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::{Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Notify, oneshot};
use wire::Inbound;

const PENDING_LIMIT: usize = 8;

/// A bounded noncredential handle; only its existing event worker owns socket I/O and token keys.
#[derive(Clone, Default)]
pub(crate) struct Client(Arc<Shared>);

#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    changed: Notify,
}

#[derive(Default)]
struct State {
    config: Option<CinnabarApiConfig>,
    epoch: u64,
    stopped: bool,
    finished: bool,
    live: Option<Live>,
    budget: Budget,
    revision: u64,
    parameters: Parameters,
    release: Option<CinnabarRelease>,
    scope: Option<ApiScope>,
}

/// Separates temporary account contention from an actual socket/account withdrawal.
pub(crate) enum PlayerSync {
    Current(CinnabarPlayerUpdates),
    Pending,
    Unavailable,
}

struct Live {
    visible: bool,
    next_id: u32,
    players: Players,
    pending: BTreeMap<u32, Pending>,
}

struct Pending {
    xuids: Vec<String>,
    uuids: Vec<String>,
    deadline: Instant,
    sent: bool,
    reply: oneshot::Sender<Result<CinnabarLookup, BridgeError>>,
}

impl Client {
    /// Uses only short state locks; no socket future or account operation retains this guard.
    fn with<T>(&self, f: impl FnOnce(&mut State) -> T) -> T {
        f(&mut self.0.state.lock().unwrap_or_else(|p| p.into_inner()))
    }

    /// Selects an unchanged identity epoch; visibility remains a coalesced live message.
    fn current(&self, epoch: u64) -> bool {
        self.with(|state| !state.stopped && state.epoch == epoch)
    }

    /// Waits for an actual identity replacement/shutdown, retaining I/O across unrelated state wakes.
    async fn superseded(&self, epoch: u64) {
        loop {
            let changed = self.0.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            if !self.current(epoch) {
                return;
            }
            changed.await;
        }
    }
}

/// Keeps native unavailability on the existing worker's error path without invoking the Go bridge.
fn unavailable() -> BridgeError {
    BridgeError::ControlRpc {
        code: RPC_SERVICE_FAILED,
        message: "Service unavailable".into(),
    }
}

/// Preserves the existing roster worker's negotiated-budget retry path.
fn rate_limited() -> BridgeError {
    BridgeError::ControlRpc {
        code: RPC_CINNABAR_RATE_LIMITED,
        message: "Rate limited".into(),
    }
}
