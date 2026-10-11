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
