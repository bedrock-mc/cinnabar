//! Joinable destinations used by desktop presence.
/// Where a join to `address` plays and the address a Discord invite joins. Servers (by endpoint
/// with a port), experiences and friends' worlds are joinable; a friend's world still needs the
/// joiner to see it through Xbox. Realms carry no invite, a local world's comes from its host
/// (see `crate::menu::MenuRuntime::hosted_world_address`), and no identifier is ever shown on the card.
pub fn presence_target(address: &str, local_world: bool) -> rich_presence::Target {
    use bridge::ConnectTarget;
    use rich_presence::{Destination, Target};
    if local_world {
        return Target {
            destination: Destination::LocalWorld(address.to_owned()),
            join: None,
            badge: None,
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
    Target {
        destination,
        join,
        badge: None,
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
        let joinable = |destination, join: &str| Target {
            destination,
            join: Some(join.to_owned()),
            badge: None,
            max_players: None,
        };
        let server = |endpoint: &str| joinable(Destination::Server(endpoint.to_owned()), endpoint);
        let private = |destination| Target {
            destination,
            join: None,
            badge: None,
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
            private(Destination::Realm)
        );
        assert_eq!(
            presence_target("friend_xuid/2535400000000000", false),
            joinable(Destination::FriendWorld, "friend_xuid/2535400000000000")
        );
        let experience = format!("{}fixture", launcher::menu::EXPERIENCE_ADDRESS_PREFIX);
        assert_eq!(
            presence_target(&experience, false),
            joinable(Destination::Experience, &experience)
        );
        assert_eq!(
            presence_target("My World", true),
            private(Destination::LocalWorld("My World".into()))
        );
        assert!(invite_joinable("play.example.net:19132"));
        assert!(invite_joinable(&experience));
        assert!(invite_joinable("friend_xuid/2535400000000000"));
        assert!(!invite_joinable("realm_id/123"));
    }
}
