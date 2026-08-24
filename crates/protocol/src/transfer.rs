use valentine::bedrock::version::v1_26_44::McpePacketData;

/// Longest retained transfer host, in bytes.
///
/// DNS names cap at 253 octets and literal IPv6 at 45 characters, so this
/// bounds every legitimate target while refusing unbounded wire strings.
pub const MAX_TRANSFER_HOST_BYTES: usize = 255;

/// Bounded, vendor-neutral record of a server-directed transfer target.
///
/// The host is trimmed of surrounding whitespace and validated for
/// well-formedness only; following the target is a policy decision owned above
/// the protocol layer. Vanilla servers legitimately transfer across unrelated
/// hosts, so no address allowlist exists here. The optional gatherings
/// configuration is deliberately not retained: no production consumer exists
/// and the field is absent from ordinary retail transfers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerTransferEvent {
    pub host: String,
    pub port: u16,
    pub reload_world: bool,
}

/// Why a completely decoded Transfer packet named an unusable target.
///
/// These are semantic oddities, not wire failures: the packet decoded within
/// every length bound, so the session survives and the caller counts the
/// rejection instead of tearing down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerTransferRejection {
    EmptyHost,
    HostTooLong { bytes: usize },
    InvalidHostCharacter,
    ZeroPort,
}

impl ServerTransferEvent {
    /// Normalizes one decoded packet into a bounded transfer target.
    ///
    /// Non-transfer packets normalize to nothing. Well-formed wire naming an
    /// unusable target (empty or oversized host, forbidden characters, zero
    /// port) returns the counted [`ServerTransferRejection`] instead of an
    /// event; truncated or otherwise malformed wire never reaches here because
    /// decode failures stay fatal upstream.
    pub fn from_packet_data(
        data: &McpePacketData,
    ) -> Result<Option<Self>, ServerTransferRejection> {
        let McpePacketData::TransferPacket(packet) = data else {
            return Ok(None);
        };
        Self::from_wire_fields(
            &packet.server_address,
            packet.server_port,
            packet.reload_world,
        )
    }

    fn from_wire_fields(
        address: &str,
        port: u16,
        reload_world: bool,
    ) -> Result<Option<Self>, ServerTransferRejection> {
        let host = address.trim();
        if host.is_empty() {
            return Err(ServerTransferRejection::EmptyHost);
        }
        if host.len() > MAX_TRANSFER_HOST_BYTES {
            return Err(ServerTransferRejection::HostTooLong { bytes: host.len() });
        }
        if host.chars().any(is_forbidden_host_character) {
            return Err(ServerTransferRejection::InvalidHostCharacter);
        }
        if port == 0 {
            return Err(ServerTransferRejection::ZeroPort);
        }
        Ok(Some(Self {
            host: host.to_owned(),
            port,
            reload_world,
        }))
    }
}

/// Rejects ASCII control characters and interior ASCII whitespace.
///
/// This is a well-formedness floor, not a hostname grammar: unusual but
/// dialable characters are retained and fail visibly when the core dials the
/// target, which keeps the boundary honest without inventing policy.
fn is_forbidden_host_character(character: char) -> bool {
    matches!(character, '\0'..='\x1f' | '\x7f' | ' ')
}

#[cfg(test)]
mod tests {
    use super::*;
    use valentine::bedrock::version::v1_26_44::TransferPacket;

    fn transfer_data(address: &str, port: u16) -> McpePacketData {
        McpePacketData::TransferPacket(Box::new(TransferPacket {
            server_address: address.to_owned(),
            server_port: port,
            reload_world: false,
            ..Default::default()
        }))
    }

    #[test]
    fn normalization_retains_a_bounded_target() {
        let event =
            ServerTransferEvent::from_packet_data(&transfer_data("play.example.net", 19133))
                .expect("well-formed target")
                .expect("transfer packet normalizes");
        assert_eq!(event.host, "play.example.net");
        assert_eq!(event.port, 19133);
        assert!(!event.reload_world);
    }

    #[test]
    fn normalization_retains_the_reload_flag_and_trims_surrounding_whitespace() {
        let data = McpePacketData::TransferPacket(Box::new(TransferPacket {
            server_address: " game.example.net ".to_owned(),
            server_port: 19133,
            reload_world: true,
            ..Default::default()
        }));
        let event = ServerTransferEvent::from_packet_data(&data)
            .expect("well-formed target")
            .expect("transfer packet normalizes");
        assert_eq!(event.host, "game.example.net");
        assert!(event.reload_world);
    }

    #[test]
    fn normalization_ignores_non_transfer_packets() {
        let other =
            McpePacketData::SetTimePacket(valentine::bedrock::version::v1_26_44::SetTimePacket {
                time: 7,
            });
        assert_eq!(
            ServerTransferEvent::from_packet_data(&other).expect("non-transfer normalizes"),
            None
        );
    }

    #[test]
    fn empty_and_whitespace_only_hosts_are_semantic_rejections() {
        assert_eq!(
            ServerTransferEvent::from_packet_data(&transfer_data("", 19133)),
            Err(ServerTransferRejection::EmptyHost)
        );
        assert_eq!(
            ServerTransferEvent::from_packet_data(&transfer_data("   ", 19133)),
            Err(ServerTransferRejection::EmptyHost)
        );
    }

    #[test]
    fn oversize_hosts_are_semantic_rejections_without_truncation() {
        let bytes = "a".repeat(MAX_TRANSFER_HOST_BYTES + 1);
        match ServerTransferEvent::from_packet_data(&transfer_data(&bytes, 19133)) {
            Err(ServerTransferRejection::HostTooLong { bytes }) => {
                assert_eq!(bytes, MAX_TRANSFER_HOST_BYTES + 1);
            }
            other => panic!("oversize host must be rejected, got {other:?}"),
        }
        let exact = "a".repeat(MAX_TRANSFER_HOST_BYTES);
        assert!(
            ServerTransferEvent::from_packet_data(&transfer_data(&exact, 19133))
                .expect("exact bound is well-formed")
                .is_some()
        );
    }

    #[test]
    fn control_characters_are_semantic_rejections() {
        assert_eq!(
            ServerTransferEvent::from_packet_data(&transfer_data("play.exam\x07ple.net", 19133)),
            Err(ServerTransferRejection::InvalidHostCharacter)
        );
        assert_eq!(
            ServerTransferEvent::from_packet_data(&transfer_data("play exam ple.net", 19133)),
            Err(ServerTransferRejection::InvalidHostCharacter)
        );
    }

    #[test]
    fn zero_port_is_a_semantic_rejection() {
        assert_eq!(
            ServerTransferEvent::from_packet_data(&transfer_data("play.example.net", 0)),
            Err(ServerTransferRejection::ZeroPort)
        );
    }

    #[test]
    fn cross_host_targets_are_not_filtered_by_any_allowlist() {
        // A vanilla lobby routinely hops clients to an unrelated minigame
        // host. Well-formedness must not reject the shape of that target.
        let event = ServerTransferEvent::from_packet_data(&transfer_data(
            "minigames.other-host.example",
            19321,
        ))
        .expect("well-formed target")
        .expect("cross-host transfer normalizes");
        assert_eq!(event.host, "minigames.other-host.example");
    }
}
