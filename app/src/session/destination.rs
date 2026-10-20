//! Native backend selection over the existing destination grammar and Realm authority.

use protocol::{NativeConnectionTarget, NativeTransport};

/// Distinguishes native provisioning from destinations requiring another owned selection.
pub(crate) enum Destination {
    Network(NativeConnectionTarget),
    LocalWorld,
    FriendSelectionRequired,
}

/// Keeps service-owned destinations on their provisioning path and normalizes only direct endpoints.
pub(crate) fn classify(
    address: &str,
    transport: NativeTransport,
    local_world: bool,
) -> Destination {
    if local_world {
        return Destination::LocalWorld;
    }
    if launcher_host::native_account::is_realm_target(address)
        || protocol::NativeRawTarget::recognizes(address)
    {
        return Destination::Network(NativeConnectionTarget::new(
            address.trim().to_owned(),
            NativeTransport::RakNet,
        ));
    }
    match launcher_host::launcher_core::target_for(address) {
        bridge::ConnectTarget::RakNet(normalized) => {
            let address = if transport == NativeTransport::Addressed {
                address.trim().to_owned()
            } else {
                normalized
            };
            Destination::Network(NativeConnectionTarget::new(address, transport))
        }
        bridge::ConnectTarget::Gathering(_) | bridge::ConnectTarget::Realm(_) => {
            Destination::Network(NativeConnectionTarget::new(
                address.trim().to_owned(),
                NativeTransport::RakNet,
            ))
        }
        bridge::ConnectTarget::Friend(_) => Destination::FriendSelectionRequired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Explicit service signaling survives classification until the selected account prepares it.
    #[test]
    fn native_raw_service_destination_retains_explicit_signaling_before_preparation() {
        for (tag, id) in [
            (
                bridge::contract::NETHERNET_SIGNALING_JSONRPC,
                "00000000-0000-0000-0000-000000000007",
            ),
            (bridge::contract::NETHERNET_SIGNALING_WEBSOCKET, "00042"),
        ] {
            let address = format!("{}{tag}/{id}", bridge::contract::NETHERNET_TARGET_PREFIX);
            let Destination::Network(target) = classify(&address, NativeTransport::RakNet, false)
            else {
                panic!("explicit service route remained on Core");
            };
            assert_eq!(
                target.address, address,
                "service grammar was normalized as a DNS endpoint"
            );
            assert_eq!(target.transport, NativeTransport::RakNet);
        }
    }
}
