//! Joinable destinations used by desktop presence.
/// Where a join to `address` plays and the address a Discord invite joins. Servers (by endpoint
/// with a port), experiences and friends' worlds are joinable; a friend's world still needs the
/// joiner to see it through Xbox. Realms carry no invite, a local world's comes from its host
/// (see `crate::menu::MenuRuntime::hosted_world_address`), and no identifier is ever shown on the card.
/// Known featured and public Bedrock servers to friendly names and icon URLs.
fn resolve_server_badge(address: &str) -> Option<rich_presence::Badge> {
    let lower = address.trim().to_lowercase();
    let known: &[(&[&str], &str, &str)] = &[
        (
            &["geo.hivebedrock.network", "hivebedrock", "thehive"],
            "The Hive",
            "https://api.mcsrvstat.us/icon/geo.hivebedrock.network",
        ),
        (
            &["cubecraft"],
            "CubeCraft Games",
            "https://api.mcsrvstat.us/icon/cubecraft.net",
        ),
        (
            &["galaxite"],
            "Galaxite",
            "https://api.mcsrvstat.us/icon/galaxite.net",
        ),
        (
            &["nethergames"],
            "NetherGames",
            "https://api.mcsrvstat.us/icon/nethergames.org",
        ),
        (
            &["lbsg", "lifeboat"],
            "Lifeboat Network",
            "https://api.mcsrvstat.us/icon/lbsg.net",
        ),
        (
            &["inpvp", "mineville"],
            "Mineville",
            "https://api.mcsrvstat.us/icon/inpvp.net",
        ),
        (
            &["zeqa"],
            "Zeqa Network",
            "https://api.mcsrvstat.us/icon/zeqa.net",
        ),
        (
            &["motaz"],
            "MotazMC",
            "https://api.mcsrvstat.us/icon/motazmc.fr",
        ),
        (
            &["veltrix"],
            "Veltrix",
            "https://api.mcsrvstat.us/icon/veltrixmc.fr",
        ),
    ];

    for &(patterns, name, icon) in known {
        if patterns.iter().any(|pattern| lower.contains(pattern)) {
            return Some(rich_presence::Badge {
                image_url: icon.to_owned(),
                name: name.to_owned(),
            });
        }
    }

    let host = address
        .trim()
        .split(':')
        .next()
        .unwrap_or("")
        .trim_start_matches('[')
        .trim_end_matches(']');
    if host.contains('.') && !host.chars().all(|c| c.is_ascii_digit() || c == '.') {
        Some(rich_presence::Badge {
            image_url: format!("https://api.mcsrvstat.us/icon/{host}"),
            name: host.to_owned(),
        })
    } else {
        Some(rich_presence::Badge {
            image_url: rich_presence::GRASS_BLOCK_ICON_URL.to_owned(),
            name: "Minecraft Bedrock".to_owned(),
        })
    }
}

pub fn presence_target(address: &str, local_world: bool) -> rich_presence::Target {
    use bridge::ConnectTarget;
    use rich_presence::{Destination, Target};
    if local_world {
        return Target {
            destination: Destination::LocalWorld(address.to_owned()),
            join: None,
            badge: Some(rich_presence::Badge {
                image_url: rich_presence::GRASS_BLOCK_ICON_URL.to_owned(),
                name: "Singleplayer".to_owned(),
            }),
            max_players: None,
        };
    }
    let address = address.trim();
    let (destination, join) = match launcher_host::launcher_core::target_for(address) {
        ConnectTarget::RakNet(endpoint) => (Destination::Server(endpoint.clone()), Some(endpoint)),
        ConnectTarget::Gathering(_) => (Destination::Experience, Some(address.to_owned())),
        ConnectTarget::Friend(_) => (Destination::FriendWorld, Some(address.to_owned())),
        ConnectTarget::Realm(_) => (Destination::Realm, None),
    };
    let badge = match &destination {
        Destination::Server(_) => resolve_server_badge(address),
        _ => None,
    };
    Target {
        destination,
        join,
        badge,
        max_players: None,
    }
}

/// Whether a received invite names a destination Cinnabar itself makes joinable.
pub fn invite_joinable(address: &str) -> bool {
    presence_target(address, false).join.as_deref() == Some(address)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joinable_destinations_and_ids_stay_off_the_card() {
        use rich_presence::{Destination, Target};
        let joinable = |destination, join: &str, badge| Target {
            destination,
            join: Some(join.to_owned()),
            badge,
            max_players: None,
        };
        let server = |endpoint: &str| {
            joinable(
                Destination::Server(endpoint.to_owned()),
                endpoint,
                resolve_server_badge(endpoint),
            )
        };
        let private = |destination, badge| Target {
            destination,
            join: None,
            badge,
            max_players: None,
        };
        assert_eq!(
            presence_target(" play.example.net ", false),
            server(&format!(
                "play.example.net:{}",
                launcher::menu::DEFAULT_PORT
            ))
        );
        assert_eq!(presence_target("[::1]:19134", false), server("[::1]:19134"));
        assert_eq!(
            presence_target("::1", false),
            server(&format!("[::1]:{}", launcher::menu::DEFAULT_PORT))
        );
        assert_eq!(
            presence_target(" realm_id/123", false),
            private(Destination::Realm, None)
        );
        assert_eq!(
            presence_target("friend_xuid/2535400000000000", false),
            joinable(
                Destination::FriendWorld,
                "friend_xuid/2535400000000000",
                None
            )
        );
        let experience = format!("{}fixture", launcher::menu::EXPERIENCE_ADDRESS_PREFIX);
        assert_eq!(
            presence_target(&experience, false),
            joinable(Destination::Experience, &experience, None)
        );
        assert_eq!(
            presence_target("My World", true),
            private(
                Destination::LocalWorld("My World".into()),
                Some(rich_presence::Badge {
                    image_url: rich_presence::GRASS_BLOCK_ICON_URL.to_owned(),
                    name: "Singleplayer".to_owned(),
                })
            )
        );
        assert!(invite_joinable("play.example.net:19132"));
        assert!(invite_joinable(&experience));
        assert!(invite_joinable("friend_xuid/2535400000000000"));
        assert!(!invite_joinable("realm_id/123"));
    }
}
