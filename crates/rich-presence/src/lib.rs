//! Optional Discord desktop activity, with all IPC owned by the RPC worker.

use discord_presence::{Client, event_handler::EventCallbackHandle, models::Activity};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub const APPLICATION_ID_ENV: &str = "CINNABAR_DISCORD_APPLICATION_ID";

/// Default application ID when one is configured for the distribution.
pub const DEFAULT_APPLICATION_ID: Option<u64> = None;

/// `0` explicitly disables presence; other values must be nonzero numeric application IDs.
pub fn application_id(value: Option<&str>) -> Result<Option<u64>, &'static str> {
    match value {
        None => Ok(DEFAULT_APPLICATION_ID),
        Some("0") => Ok(None),
        Some(value) if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) => {
            value
                .parse::<u64>()
                .ok()
                .filter(|id| *id != 0)
                .map(Some)
                .ok_or("expected a nonzero numeric Discord application ID, or 0 to disable")
        }
        Some(_) => Err("expected a nonzero numeric Discord application ID, or 0 to disable"),
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum State {
    Menus,
    Joining,
    Playing,
}

impl State {
    pub fn activity(self, started_at: u64) -> Activity {
        let state = match self {
            Self::Menus => "In the menus",
            Self::Joining => "Joining a world",
            Self::Playing => "In a world",
        };
        Activity::new()
            .details(launcher::PRODUCT_NAME)
            .state(state)
            .timestamps(|timestamps| timestamps.start(started_at))
    }
}

#[derive(Default)]
struct Publication {
    last: Option<(State, u64)>,
}

impl Publication {
    fn changed(&mut self, state: State, connection: u64) -> bool {
        let current = (state, connection);
        if self.last == Some(current) {
            return false;
        }
        self.last = Some(current);
        true
    }
}

/// Queues only changes; the library worker coalesces updates and applies Discord's rate limit.
pub struct Presence {
    client: Option<Client>,
    connected: Option<EventCallbackHandle>,
    connection: Arc<AtomicU64>,
    publication: Publication,
    started_at: u64,
}

impl Presence {
    pub fn start(application_id: u64) -> Self {
        let connection = Arc::new(AtomicU64::new(0));
        let mut client = Client::new(application_id);
        let epoch = Arc::clone(&connection);
        let connected = client.on_connected(move |_| {
            epoch.fetch_add(1, Ordering::Relaxed);
        });
        let started_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        client.start();
        Self {
            client: Some(client),
            connected: Some(connected),
            connection,
            publication: Publication::default(),
            started_at,
        }
    }

    pub fn update(&mut self, state: State) {
        if self
            .publication
            .changed(state, self.connection.load(Ordering::Relaxed))
            && let Some(client) = self.client.as_mut()
        {
            let started_at = self.started_at;
            client.queue_activity(|_| state.activity(started_at));
        }
    }
}

impl Drop for Presence {
    fn drop(&mut self) {
        drop(self.connected.take());
        if let Some(client) = self.client.take() {
            // Discord retries can sleep past the app's shutdown deadline.
            let _ = std::thread::Builder::new()
                .name("discord-shutdown".into())
                .spawn(move || {
                    let _ = client.shutdown();
                });
        }
    }
}

#[cfg(test)]
mod tests;
