//! Optional Discord desktop activity, with all IPC owned by the RPC worker.

use discord_presence::{Client, event_handler::EventCallbackHandle, models::Activity};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

pub const APPLICATION_ID_ENV: &str = "CINNABAR_DISCORD_APPLICATION_ID";

/// Discord application used by the distribution unless overridden or disabled.
pub const DEFAULT_APPLICATION_ID: u64 = 1557101333037711371;

/// Original icon served from an immutable version of the public repository.
pub const LARGE_IMAGE_URL: &str = "https://raw.githubusercontent.com/bedrock-mc/cinnabar/0f96fadfe4f9bb7c450147f2462d4a5d654da7f2/assets/branding/icon.png";

/// `0` explicitly disables presence; other values must be nonzero numeric application IDs.
pub fn application_id(value: Option<&str>) -> Result<Option<u64>, &'static str> {
    match value {
        None => Ok(Some(DEFAULT_APPLICATION_ID)),
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

/// A public server endpoint with an explicit port, including brackets for IPv6.
pub fn normalize_endpoint(address: &str) -> String {
    let (host, port) = launcher::menu::split_address(address.trim());
    if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

impl State {
    pub fn activity(self, started_at: u64, address: Option<&str>) -> Activity {
        let mut state = match self {
            Self::Menus => "In the menus".to_owned(),
            Self::Joining => "Joining a world".to_owned(),
            Self::Playing => match address {
                Some(address) => format!("Playing on {address}"),
                None => "In a world".to_owned(),
            },
        };
        state.truncate(state.floor_char_boundary(MAX_STATE_BYTES));
        Activity::new()
            .details(launcher::PRODUCT_NAME)
            .state(state)
            .assets(|assets| {
                assets
                    .large_image(LARGE_IMAGE_URL)
                    .large_text(launcher::PRODUCT_NAME)
            })
            .timestamps(|timestamps| timestamps.start(started_at))
    }
}

const MAX_STATE_BYTES: usize = 128;

#[derive(Default)]
struct Publication {
    last: Option<(State, u64, Option<String>)>,
}

impl Publication {
    fn changed(&mut self, state: State, connection: u64, address: Option<&str>) -> bool {
        let address = if state == State::Playing {
            address
        } else {
            None
        };
        if let Some((last_state, last_connection, last_address)) = &self.last
            && *last_state == state
            && *last_connection == connection
            && last_address.as_deref() == address
        {
            return false;
        }
        self.last = Some((state, connection, address.map(str::to_owned)));
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

    pub fn update(&mut self, state: State, address: Option<&str>) {
        if self
            .publication
            .changed(state, self.connection.load(Ordering::Relaxed), address)
            && let Some(client) = self.client.as_mut()
        {
            let started_at = self.started_at;
            client.queue_activity(|_| state.activity(started_at, address));
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
