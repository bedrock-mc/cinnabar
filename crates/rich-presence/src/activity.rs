//! Builds Discord activity payloads from session observations and privacy settings.

use discord_presence::models::Activity;

use crate::{
    Destination, GRASS_BLOCK_ICON_URL, LARGE_IMAGE_URL, MAX_IMAGE_BYTES, MAX_STATE_BYTES,
    MINECRAFT_VERSION, PresenceSettings, State, Target, join,
};

impl State {
    /// `players` is how many are in the session now, for the party size.
    pub fn activity(
        self,
        started_at: u64,
        target: Option<&Target>,
        players: Option<u32>,
    ) -> Activity {
        self.activity_with_settings(started_at, target, players, PresenceSettings::default())
    }

    /// Builds an activity card using the user's Discord privacy and interaction settings.
    pub fn activity_with_settings(
        self,
        started_at: u64,
        target: Option<&Target>,
        players: Option<u32>,
        settings: PresenceSettings,
    ) -> Activity {
        let destination = target.map(|target| &target.destination);
        let mut state = if self == Self::Playing && !settings.show_server_details {
            "Playing Minecraft".to_owned()
        } else {
            match (self, destination) {
                (Self::Menus, _) => "In the menus".to_owned(),
                (Self::Joining, _) => "Joining a world".to_owned(),
                (Self::Playing, None) => "In a world".to_owned(),
                (Self::Playing, Some(Destination::Server(endpoint))) => {
                    if let Some(badge) = target.and_then(|t| t.badge.as_ref()) {
                        let badge_name = badge.name.trim();
                        if !badge_name.is_empty()
                            && badge_name != "Minecraft Bedrock"
                            && !badge_name.contains(':')
                        {
                            format!("Playing on {badge_name}")
                        } else {
                            format!("Playing on {endpoint}")
                        }
                    } else {
                        format!("Playing on {endpoint}")
                    }
                }
                (Self::Playing, Some(Destination::Realm)) => "Playing on a Realm".to_owned(),
                (Self::Playing, Some(Destination::FriendWorld)) => {
                    "Playing in a friend's world".to_owned()
                }
                (Self::Playing, Some(Destination::Experience)) => {
                    "Playing an experience".to_owned()
                }
                (Self::Playing, Some(Destination::LocalWorld(name))) => {
                    format!("Singleplayer: {}", name.trim())
                }
            }
        };
        state.truncate(state.floor_char_boundary(MAX_STATE_BYTES));
        let badge = (self == Self::Playing && settings.show_server_details)
            .then(|| target?.badge.as_ref())
            .flatten()
            .filter(|badge| {
                badge.image_url.starts_with("https://") && badge.image_url.len() <= MAX_IMAGE_BYTES
            });
        let mut activity = Activity::new().details(MINECRAFT_VERSION).state(state);
        if settings.show_timestamp {
            activity = activity.timestamps(|timestamps| timestamps.start(started_at));
        }
        let activity = activity.assets(|assets| {
            let assets = assets
                .large_image(LARGE_IMAGE_URL.as_str())
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
                None => {
                    if self == Self::Playing {
                        assets
                            .small_image(GRASS_BLOCK_ICON_URL)
                            .small_text("Minecraft Bedrock")
                    } else {
                        assets
                    }
                }
            }
        });
        let target = target.filter(|_| self == Self::Playing);
        let (party, secret) = if settings.allow_joins {
            target
                .and_then(|target| target.join.as_deref())
                .and_then(|address| Some((join::party_id(address), join::secret(address)?)))
                .unzip()
        } else {
            (None, None)
        };
        // Discord shows "(current of max)" and rejects a current above max.
        let size = target
            .filter(|_| settings.show_party_size)
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
