//! Typed menu destinations shared by launcher and browser presentation.

use bridge::ConnectTarget;
use launcher::menu::DEFAULT_PORT;

/// The kind of join `address` starts, for its progress titles.
pub fn join_kind(address: &str, local_world: bool) -> launcher::menu::view::JoinKind {
    use launcher::menu::view::JoinKind;
    match target_for(address) {
        _ if local_world => JoinKind::Local,
        ConnectTarget::Realm(_) => JoinKind::Realm,
        // Friend worlds and experiences use the external-server title until vanilla's is confirmed.
        ConnectTarget::RakNet(_) | ConnectTarget::Friend(_) | ConnectTarget::Gathering(_) => {
            JoinKind::External
        }
    }
}

/// The typed menu destination, preserving service prefixes and the default server port.
pub fn target_for(address: &str) -> ConnectTarget {
    let address = address.trim();
    if let Some(id) = address.strip_prefix(launcher::menu::EXPERIENCE_ADDRESS_PREFIX) {
        return ConnectTarget::Gathering(id.to_owned());
    }
    if let Some(id) = address.strip_prefix(bridge::contract::REALM_TARGET_PREFIX) {
        return ConnectTarget::Realm(id.to_owned());
    }
    if let Some(xuid) = address.strip_prefix(launcher::menu::FRIEND_ADDRESS_PREFIX) {
        return ConnectTarget::Friend(xuid.to_owned());
    }
    let has_port = address.rsplit_once(':').is_some_and(|(host, port)| {
        port.parse::<u16>().is_ok() && (host.ends_with(']') || !host.contains(':'))
    });
    ConnectTarget::RakNet(if has_port {
        address.to_owned()
    } else if address.contains(':') && !address.starts_with('[') {
        format!("[{address}]:{DEFAULT_PORT}")
    } else {
        format!("{address}:{DEFAULT_PORT}")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_addresses_map_to_connect_targets() {
        assert_eq!(
            target_for(&format!("{}42", bridge::contract::REALM_TARGET_PREFIX)),
            ConnectTarget::Realm("42".into())
        );
        assert_eq!(
            target_for(&format!(
                "{}5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f",
                bridge::contract::GATHERING_TARGET_PREFIX
            )),
            ConnectTarget::Gathering("5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f".into())
        );
        assert_eq!(
            target_for(&format!("{}2535", bridge::contract::FRIEND_TARGET_PREFIX)),
            ConnectTarget::Friend("2535".into())
        );
        assert_eq!(
            target_for("play.example.net:19133"),
            ConnectTarget::RakNet("play.example.net:19133".into())
        );
        assert_eq!(
            target_for("play.example.net"),
            ConnectTarget::RakNet(format!(
                "play.example.net:{}",
                bridge::contract::DEFAULT_BEDROCK_PORT_TEXT
            ))
        );
        assert_eq!(
            target_for("[::1]:19134"),
            ConnectTarget::RakNet("[::1]:19134".into())
        );
        assert_eq!(
            target_for("::1"),
            ConnectTarget::RakNet(format!(
                "[::1]:{}",
                bridge::contract::DEFAULT_BEDROCK_PORT_TEXT
            ))
        );
    }
}
