//! Shared session generation, transfer addressing and off-frame retirement.

use client_state::PlayerRuntime;
use client_ui::ui_runtime::UiRuntime;

/// Moves UI and player authority to `generation` together; repeating it is a no-op.
pub fn begin_session(ui: &mut UiRuntime, player: &mut PlayerRuntime, generation: u64) {
    if ui.session_id() != generation {
        player.begin_session(generation);
        ui.begin_session(generation);
    }
}

/// Renders a validated transfer host and port as a dialable address.
///
/// IPv6 literals are bracketed the way the Go core's dialer expects.
pub fn format_transfer_address(host: &str, port: u16) -> String {
    if host.starts_with('[') && host.ends_with(']') {
        format!("{host}:{port}")
    } else if host.contains(':') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

/// The address a server-directed transfer dials, or `None` for an unusable host.
///
/// Well-formedness only, like the protocol boundary: vanilla servers
/// legitimately transfer across unrelated hosts, so there is no allowlist.
pub fn transfer_handoff_address(host: &str, port: u16) -> Option<String> {
    let trimmed = host.trim();
    (!trimmed.is_empty()).then(|| format_transfer_address(trimmed, port))
}

/// Where a join to `address` plays and the address a Discord invite joins. Servers (by endpoint
/// with a port), experiences and friends' worlds are joinable; a friend's world still needs the
/// joiner to see it through Xbox. Realms carry no invite; the local world host supplies its
/// own invite. No identifier is shown on the card.
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

/// Drops retired world and pack snapshots on a worker to avoid frame-thread destruction.
/// Falls back to dropping here only when the worker cannot start.
pub fn release_off_frame(retired: impl Send + 'static) {
    let spawned = std::thread::Builder::new()
        .name("session-release".to_owned())
        .spawn(move || drop(retired));
    if let Err(error) = spawned {
        bevy::log::warn!("session release thread unavailable, released on the frame: {error}");
    }
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

    #[test]
    fn transfer_addresses_bracket_ipv6_and_leave_ordinary_hosts_untouched() {
        assert_eq!(
            format_transfer_address("game.example.net", 19133),
            "game.example.net:19133"
        );
        assert_eq!(format_transfer_address("::1", 19132), "[::1]:19132");
        assert_eq!(
            format_transfer_address("2001:db8::10", 25565),
            "[2001:db8::10]:25565"
        );
        assert_eq!(
            format_transfer_address("[2001:db8::10]", 25565),
            "[2001:db8::10]:25565",
            "an already-bracketed transfer literal must not be bracketed twice",
        );
    }

    #[test]
    fn handoff_targets_are_well_formed_without_any_host_allowlist() {
        assert_eq!(
            transfer_handoff_address(" game.example.net ", 19133).as_deref(),
            Some("game.example.net:19133"),
            "a trimmed well-formed host is a valid target"
        );
        assert_eq!(
            transfer_handoff_address("minigames.other-host.example", 19321).as_deref(),
            Some("minigames.other-host.example:19321"),
            "cross-host transfers are legitimate vanilla behavior"
        );
        assert!(transfer_handoff_address("", 19132).is_none());
        assert!(transfer_handoff_address("   ", 19132).is_none());
    }

    /// A retired world is freed on another thread, never by the frame that leaves the server.
    #[test]
    fn a_retired_session_is_released_off_the_calling_thread() {
        struct Probe(std::sync::mpsc::Sender<std::thread::ThreadId>);
        impl Drop for Probe {
            fn drop(&mut self) {
                let _ = self.0.send(std::thread::current().id());
            }
        }
        let (dropped, dropper) = std::sync::mpsc::channel();
        release_off_frame((Probe(dropped), vec![0_u8; 1024]));
        let thread = dropper
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("the retired session is released");
        assert_ne!(thread, std::thread::current().id());
    }
}
