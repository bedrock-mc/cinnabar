//! Optional Discord desktop activity, with all IPC owned by the RPC worker.

mod activity;
mod join;
mod launch;
mod presence;

pub use join::join_address;
pub use presence::{JoinRequest, Presence};

pub const APPLICATION_ID_ENV: &str = "CINNABAR_DISCORD_APPLICATION_ID";

/// Discord application used by the distribution unless overridden or disabled.
pub const DEFAULT_APPLICATION_ID: u64 = 1557101333037711371;

/// Original icon served from an immutable version of the public repository.
pub static LARGE_IMAGE_URL: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let manifest: serde_json::Value =
        serde_json::from_str(include_str!("../../../packaging/release-assets.json"))
            .expect("release asset manifest must be valid");
    let repository = manifest["repository"]
        .as_str()
        .expect("release asset manifest must name the repository");
    format!(
        "https://raw.githubusercontent.com/{repository}/0f96fadfe4f9bb7c450147f2462d4a5d654da7f2/assets/branding/icon.png"
    )
});

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

pub const MINECRAFT_VERSION: &str = "Minecraft 26.52";

/// Classic Minecraft 3D grass block icon for unknown servers and singleplayer worlds.
pub const GRASS_BLOCK_ICON_URL: &str = "https://minecraft.wiki/images/Invicon_Grass_Block.png";

/// Optional information and interaction exposed by the Discord activity card.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PresenceSettings {
    /// Show a server address or local world name in the activity state.
    pub show_server_details: bool,
    /// Show the current and maximum player count when the destination supplies a limit.
    pub show_party_size: bool,
    /// Include join secrets and accept Discord join requests.
    pub allow_joins: bool,
    /// Include the activity start timestamp.
    pub show_timestamp: bool,
}

impl Default for PresenceSettings {
    fn default() -> Self {
        Self {
            show_server_details: true,
            show_party_size: true,
            allow_joins: true,
            show_timestamp: true,
        }
    }
}

const MAX_STATE_BYTES: usize = 128;

/// The state and preferences used to publish the last activity.
struct PublishedActivity {
    state: State,
    connection: u64,
    target: Option<Target>,
    players: Option<u32>,
    settings: PresenceSettings,
}

#[derive(Default)]
struct Publication {
    last: Option<PublishedActivity>,
    /// When the current target went live, so the in-game timer counts the session.
    playing_since: Option<u64>,
}

impl Publication {
    #[cfg(test)]
    fn changed(
        &mut self,
        state: State,
        connection: u64,
        target: Option<&Target>,
        players: Option<u32>,
        now: u64,
    ) -> bool {
        self.changed_with_settings(
            state,
            connection,
            target,
            players,
            now,
            PresenceSettings::default(),
        )
    }

    fn changed_with_settings(
        &mut self,
        state: State,
        connection: u64,
        target: Option<&Target>,
        players: Option<u32>,
        now: u64,
        settings: PresenceSettings,
    ) -> bool {
        // The library sends one update per 15 s, so a join's brief loading card would hold the
        // in-world card back; the previous card stays up while joining instead.
        if state == State::Joining {
            return false;
        }
        let target = target.filter(|_| state == State::Playing);
        let players = players.filter(|_| state == State::Playing);
        if let Some(last) = &self.last
            && last.state == state
            && last.connection == connection
            && last.target.as_ref() == target
            && last.players == players
            && last.settings == settings
        {
            return false;
        }
        let same_session = self
            .last
            .as_ref()
            .is_some_and(|last| last.state == State::Playing && last.target.as_ref() == target);
        if state != State::Playing {
            self.playing_since = None;
        } else if !same_session {
            self.playing_since = Some(now);
        }
        self.last = Some(PublishedActivity {
            state,
            connection,
            target: target.cloned(),
            players,
            settings,
        });
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
