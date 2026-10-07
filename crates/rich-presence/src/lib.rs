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

/// Where a session plays. Realm, friend and experience identifiers are never published.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Destination {
    /// A server endpoint with an explicit port, IPv6 bracketed.
    Server(String),
    Realm,
    FriendWorld,
    Experience,
    /// A local world, by name.
    LocalWorld(String),
}

impl State {
    pub fn activity(self, started_at: u64, destination: Option<&Destination>) -> Activity {
        let mut state = match (self, destination) {
            (Self::Menus, _) => "In the menus".to_owned(),
            (Self::Joining, _) => "Joining a world".to_owned(),
            (Self::Playing, None) => "In a world".to_owned(),
            (Self::Playing, Some(Destination::Server(endpoint))) => {
                format!("Playing on {endpoint}")
            }
            (Self::Playing, Some(Destination::Realm)) => "Playing on a Realm".to_owned(),
            (Self::Playing, Some(Destination::FriendWorld)) => {
                "Playing in a friend's world".to_owned()
            }
            (Self::Playing, Some(Destination::Experience)) => "Playing an experience".to_owned(),
            (Self::Playing, Some(Destination::LocalWorld(name))) => {
                format!("Singleplayer: {}", name.trim())
            }
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
    last: Option<(State, u64, Option<Destination>)>,
    /// When the current destination went live, so the in-game timer counts the session.
    playing_since: Option<u64>,
}

impl Publication {
    fn changed(
        &mut self,
        state: State,
        connection: u64,
        destination: Option<&Destination>,
        now: u64,
    ) -> bool {
        let destination = destination.filter(|_| state == State::Playing);
        if let Some((last_state, last_connection, last_destination)) = &self.last
            && *last_state == state
            && *last_connection == connection
            && last_destination.as_ref() == destination
        {
            return false;
        }
        let same_session = matches!(
            &self.last,
            Some((State::Playing, _, last)) if last.as_ref() == destination
        );
        if state != State::Playing {
            self.playing_since = None;
        } else if !same_session {
            self.playing_since = Some(now);
        }
        self.last = Some((state, connection, destination.cloned()));
        true
    }
}

/// Queues only changes; the library worker coalesces updates and applies Discord's rate limit.
pub struct Presence {
    client: Option<Client>,
    connected: Option<EventCallbackHandle>,
    connection: Arc<AtomicU64>,
    publication: Publication,
    /// Launch time, shown in the menus and while joining.
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
        let started_at = unix_seconds();
        client.start();
        Self {
            client: Some(client),
            connected: Some(connected),
            connection,
            publication: Publication::default(),
            started_at,
        }
    }

    pub fn update(&mut self, state: State, destination: Option<&Destination>) {
        if self.publication.changed(
            state,
            self.connection.load(Ordering::Relaxed),
            destination,
            unix_seconds(),
        ) && let Some(client) = self.client.as_mut()
        {
            let started_at = self.publication.playing_since.unwrap_or(self.started_at);
            client.queue_activity(|_| state.activity(started_at, destination));
        }
    }
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
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
