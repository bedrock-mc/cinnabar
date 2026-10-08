//! Optional Discord desktop activity, with all IPC owned by the RPC worker.

mod join;
mod launch;
mod presence;

pub use join::join_address;
pub use presence::{JoinRequest, Presence};

use discord_presence::models::Activity;

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

/// Where a session plays. Realm, friend and experience identifiers are never shown.
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

/// A session's place on the card and, when others can follow, the menu address they join.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Target {
    pub destination: Destination,
    /// Sent only inside Discord's join secret, never in the visible card.
    pub join: Option<String>,
    /// A featured server's own art, shown in the card's corner.
    pub badge: Option<Badge>,
    /// The destination's player limit, for the card's party size.
    pub max_players: Option<u32>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Badge {
    /// Remote HTTPS image Discord proxies.
    pub image_url: String,
    /// Hover text naming the server.
    pub name: String,
}

/// Discord rejects longer image keys and URLs.
const MAX_IMAGE_BYTES: usize = 256;

impl State {
    /// `players` is how many are in the session now, for the party size.
    pub fn activity(
        self,
        started_at: u64,
        target: Option<&Target>,
        players: Option<u32>,
    ) -> Activity {
        let destination = target.map(|target| &target.destination);
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
        let badge = (self == Self::Playing)
            .then(|| target?.badge.as_ref())
            .flatten()
            .filter(|badge| {
                badge.image_url.starts_with("https://") && badge.image_url.len() <= MAX_IMAGE_BYTES
            });
        let activity = Activity::new()
            .state(state)
            .assets(|assets| {
                let assets = assets
                    .large_image(LARGE_IMAGE_URL)
                    .large_text(launcher::PRODUCT_NAME);
                match badge {
                    Some(badge) => {
                        let mut name = badge.name.trim().to_owned();
                        name.truncate(name.floor_char_boundary(MAX_STATE_BYTES));
                        let assets = assets.small_image(badge.image_url.as_str());
                        // Discord rejects hover text under two characters.
                        if name.chars().count() >= 2 {
                            assets.small_text(name)
                        } else {
                            assets
                        }
                    }
                    None => assets,
                }
            })
            .timestamps(|timestamps| timestamps.start(started_at));
        let target = target.filter(|_| self == Self::Playing);
        let (party, secret) = target
            .and_then(|target| target.join.as_deref())
            .and_then(|address| Some((join::party_id(address), join::secret(address)?)))
            .unzip();
        // Discord shows "(current of max)" and rejects a current above max.
        let size = target
            .and_then(|target| target.max_players)
            .filter(|max| *max > 0)
            .zip(players)
            .map(|(max, current)| (current.clamp(1, max), max));
        let activity = if party.is_some() || size.is_some() {
            activity.party(|joined| {
                let joined = match party {
                    Some(party) => joined.id(party),
                    None => joined,
                };
                match size {
                    Some(size) => joined.size(size),
                    None => joined,
                }
            })
        } else {
            activity
        };
        match secret {
            Some(secret) => activity.secrets(|secrets| secrets.join(secret)),
            None => activity,
        }
    }
}

const MAX_STATE_BYTES: usize = 128;

#[derive(Default)]
struct Publication {
    last: Option<(State, u64, Option<Target>, Option<u32>)>,
    /// When the current target went live, so the in-game timer counts the session.
    playing_since: Option<u64>,
}

impl Publication {
    fn changed(
        &mut self,
        state: State,
        connection: u64,
        target: Option<&Target>,
        players: Option<u32>,
        now: u64,
    ) -> bool {
        // The library sends one update per 15 s, so a join's brief loading card would hold the
        // in-world card back; the previous card stays up while joining instead.
        if state == State::Joining {
            return false;
        }
        let target = target.filter(|_| state == State::Playing);
        let players = players.filter(|_| state == State::Playing);
        if let Some((last_state, last_connection, last_target, last_players)) = &self.last
            && *last_state == state
            && *last_connection == connection
            && last_target.as_ref() == target
            && *last_players == players
        {
            return false;
        }
        let same_session = matches!(
            &self.last,
            Some((State::Playing, _, last, _)) if last.as_ref() == target
        );
        if state != State::Playing {
            self.playing_since = None;
        } else if !same_session {
            self.playing_since = Some(now);
        }
        self.last = Some((state, connection, target.cloned(), players));
        true
    }
}

fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests;
