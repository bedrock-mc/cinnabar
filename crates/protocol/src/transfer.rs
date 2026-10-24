use bedrock_protocol_1_26_51::packets;

/// Bounded record of a server-directed transfer target.
///
/// The host is trimmed and checked for well-formedness only; following it is a policy
/// decision above the protocol layer; unrelated target hosts are permitted.
/// The optional gatherings configuration is not retained: nothing consumes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerTransferEvent {
    pub host: String,
    pub port: u16,
    pub reload_world: bool,
}

/// Why a completely decoded Transfer packet named an unusable target.
///
/// These are semantic oddities, not wire failures: the session survives and the
/// caller counts the rejection instead of tearing down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerTransferRejection {
    EmptyHost,
    HostTooLong { bytes: usize },
    InvalidHostCharacter,
    ZeroPort,
}

impl ServerTransferEvent {
    /// Normalizes one decoded packet; other packets normalize to nothing.
    pub(crate) fn from_packet_data(
        data: &packets::Packet,
    ) -> Result<Option<Self>, ServerTransferRejection> {
        let packets::Packet::Transfer(packet) = data else {
            return Ok(None);
        };
        Self::from_target(
            &packet.server_address.to_string_lossy(),
            packet.server_port.0,
            packet.reload_world,
        )
        .map(Some)
    }

    /// Validates a destination, from a packet or the core's terminal Transfer, before it is kept.
    pub(crate) fn from_target(
        address: &str,
        port: u16,
        reload_world: bool,
    ) -> Result<Self, ServerTransferRejection> {
        bedrock_client::transfer::TransferTarget::from_target(address, port, reload_world)
            .map(|target| Self {
                host: target.host,
                port: target.port,
                reload_world: target.reload_world,
            })
            .map_err(|reason| match reason {
                bedrock_client::transfer::TransferRejection::EmptyHost => {
                    ServerTransferRejection::EmptyHost
                }
                bedrock_client::transfer::TransferRejection::HostTooLong { bytes } => {
                    ServerTransferRejection::HostTooLong { bytes }
                }
                bedrock_client::transfer::TransferRejection::InvalidHostCharacter => {
                    ServerTransferRejection::InvalidHostCharacter
                }
                bedrock_client::transfer::TransferRejection::ZeroPort => {
                    ServerTransferRejection::ZeroPort
                }
            })
    }
}

impl crate::ProtocolError {
    /// Returns the validated destination when a Transfer ends any login phase.
    pub fn server_transfer(&self) -> Option<ServerTransferEvent> {
        match self {
            Self::Session(crate::SessionError(crate::codec::SessionFailure::ServerTransfer(
                target,
            ))) => Some(target.clone()),
            _ => None,
        }
    }
}
