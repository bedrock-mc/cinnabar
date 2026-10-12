use bedrock_protocol_1_26_51::enums::ConnectionDisconnectFailReason;
use bedrock_protocol_1_26_51::packets::{self, Disconnect};
use bedrock_protocol_1_26_51::types::DisconnectMessages;

use crate::codec::SessionFailure;
use crate::session_wire::SessionDisconnect;

/// Longest retained server disconnect text per message field, in bytes.
pub(crate) const MAX_DISCONNECT_TEXT_BYTES: usize = 512;
const MAX_REASON_LABEL_BYTES: usize = 128;

/// Bounded, vendor-neutral record of a server-initiated disconnect.
///
/// The wire's typed fail-reason is retained as its bounded protocol-vocabulary
/// label. Both server-provided message strings drop empty values so consumers
/// can fall back cleanly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerDisconnectEvent {
    pub reason: String,
    pub message: Option<String>,
    pub filtered_message: Option<String>,
}

impl ServerDisconnectEvent {
    /// Normalizes one decoded packet; non-disconnect packets normalize to nothing.
    pub(crate) fn from_packet_data(data: &packets::Packet) -> Option<Self> {
        let packets::Packet::Disconnect(packet) = data else {
            return None;
        };
        let (message, filtered_message) = packet_texts(packet);
        Some(Self::from_fields(
            &packet.reason,
            &message,
            &filtered_message,
        ))
    }

    /// Normalizes the core's terminal Disconnect as the server's packet would read.
    pub(crate) fn from_session(disconnect: &SessionDisconnect) -> Self {
        let (message, filtered_message) = delivered_texts(disconnect);
        Self::from_fields(
            &disconnect_reason(disconnect.reason),
            message,
            filtered_message,
        )
    }

    fn from_fields(
        reason: &ConnectionDisconnectFailReason,
        message: &str,
        filtered_message: &str,
    ) -> Self {
        Self {
            reason: reason_label(reason),
            message: bounded_text(message),
            filtered_message: bounded_text(filtered_message),
        }
    }
}

/// This protocol's reason for a core disconnect code; a code it does not name reads as unknown.
pub(crate) fn disconnect_reason(code: i32) -> ConnectionDisconnectFailReason {
    ConnectionDisconnectFailReason::from(code)
}

/// The texts a Disconnect packet carries; a hidden disconnect screen sends none.
pub(crate) fn packet_texts(packet: &Disconnect) -> (String, String) {
    match &packet.messages {
        DisconnectMessages::DisconnectPacketMessages {
            message,
            filtered_message,
        } => (
            message.to_string_lossy().into_owned(),
            filtered_message.to_string_lossy().into_owned(),
        ),
        DisconnectMessages::Empty => (String::new(), String::new()),
    }
}

/// The texts a terminal Disconnect carries once the session has started: a hidden disconnect
/// screen sends none on the wire.
pub(crate) fn delivered_texts(disconnect: &SessionDisconnect) -> (&str, &str) {
    if disconnect.hide_screen {
        ("", "")
    } else {
        (
            disconnect.message.as_str(),
            disconnect.filtered_message.as_str(),
        )
    }
}

/// The failure a Disconnect ending the join reports, keeping the server's texts unbounded.
pub(crate) fn join_disconnect(
    stage: &'static str,
    reason: &ConnectionDisconnectFailReason,
    message: &str,
    filtered_message: &str,
) -> SessionFailure {
    SessionFailure::ServerDisconnect {
        stage,
        reason: reason_name(reason),
        message: message.to_owned(),
        filtered_message: filtered_message.to_owned(),
    }
}

impl crate::ProtocolError {
    /// The server's Disconnect that ended the join, when one did.
    pub fn server_disconnect(&self) -> Option<ServerDisconnectEvent> {
        let Self::Session(crate::SessionError(SessionFailure::ServerDisconnect {
            reason,
            message,
            filtered_message,
            ..
        })) = self
        else {
            return None;
        };
        Some(ServerDisconnectEvent {
            reason: clamp_bytes(reason, MAX_REASON_LABEL_BYTES),
            message: bounded_text(message),
            filtered_message: bounded_text(filtered_message),
        })
    }
}

fn reason_label(reason: &ConnectionDisconnectFailReason) -> String {
    clamp_bytes(&reason_name(reason), MAX_REASON_LABEL_BYTES)
}

/// The reason's protocol-vocabulary name; a code this protocol does not name keeps its value.
fn reason_name(reason: &ConnectionDisconnectFailReason) -> String {
    match reason {
        ConnectionDisconnectFailReason::Unknown2(code) => format!("UnknownValue({code})"),
        reason => format!("{reason:?}"),
    }
}

fn bounded_text(value: &str) -> Option<String> {
    if value.is_empty() {
        return None;
    }
    Some(clamp_bytes(value, MAX_DISCONNECT_TEXT_BYTES))
}

fn clamp_bytes(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_owned();
    }
    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value[..end].to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn disconnect_data(
        reason: ConnectionDisconnectFailReason,
        message: &str,
        filtered_message: &str,
    ) -> packets::Packet {
        packets::Packet::Disconnect(Disconnect {
            reason,
            messages: DisconnectMessages::DisconnectPacketMessages {
                message: message.into(),
                filtered_message: filtered_message.into(),
            },
        })
    }

    #[test]
    fn normalization_retains_the_server_reason_fields() {
        let event = ServerDisconnectEvent::from_packet_data(&disconnect_data(
            ConnectionDisconnectFailReason::Kicked,
            "We've detected movement cheats",
            "",
        ))
        .expect("disconnect packet normalizes");

        assert_eq!(event.reason, "Kicked");
        assert_eq!(
            event.message.as_deref(),
            Some("We've detected movement cheats")
        );
        assert_eq!(event.filtered_message, None);
    }

    #[test]
    fn normalization_is_lenient_about_empty_and_oversized_text() {
        let empty = ServerDisconnectEvent::from_packet_data(&disconnect_data(
            ConnectionDisconnectFailReason::Noreason,
            "",
            "",
        ))
        .expect("disconnect packet normalizes");
        assert_eq!(empty.message, None);
        assert_eq!(empty.filtered_message, None);

        let oversized = ServerDisconnectEvent::from_packet_data(&disconnect_data(
            ConnectionDisconnectFailReason::Unknown,
            &"日".repeat(400),
            &"é".repeat(600),
        ))
        .expect("oversized disconnect text stays well-formed");
        let message = oversized.message.expect("nonempty text is retained");
        assert!(message.len() <= MAX_DISCONNECT_TEXT_BYTES);
        assert!(
            "日".repeat(400).starts_with(message.as_str()),
            "truncation keeps the character prefix"
        );
        let filtered = oversized.filtered_message.expect("filtered text is kept");
        assert!(filtered.len() <= MAX_DISCONNECT_TEXT_BYTES);
        assert_eq!(filtered.chars().count(), MAX_DISCONNECT_TEXT_BYTES / 2);
    }

    #[test]
    fn normalization_labels_known_and_unknown_wire_reasons() {
        let known = ServerDisconnectEvent::from_packet_data(&disconnect_data(
            ConnectionDisconnectFailReason::Kickedforexploit,
            "x",
            "",
        ))
        .expect("known reason normalizes");
        assert_eq!(known.reason, "Kickedforexploit");

        let unknown = ServerDisconnectEvent::from_packet_data(&disconnect_data(
            ConnectionDisconnectFailReason::Unknown2(-7),
            "x",
            "",
        ))
        .expect("unknown reason normalizes");
        assert_eq!(unknown.reason, "UnknownValue(-7)");
    }

    #[test]
    fn a_join_time_disconnect_keeps_the_server_text() {
        let error = crate::ProtocolError::from(join_disconnect(
            "login",
            &ConnectionDisconnectFailReason::Unknown,
            "disconnectionScreen.cantConnect",
            "",
        ));
        let event = error.server_disconnect().expect("disconnect retained");
        assert_eq!(
            event.message.as_deref(),
            Some("disconnectionScreen.cantConnect")
        );
        assert_eq!(event.filtered_message, None);
        let closed = crate::ProtocolError::from(SessionFailure::ConnectionClosed);
        assert!(closed.server_disconnect().is_none());
    }

    /// Core codes map onto this protocol's reasons, an unnamed one keeps its value, and a hidden
    /// screen drops the texts as the server's packet would.
    #[test]
    fn terminal_disconnects_read_as_the_servers_packet() {
        let disconnect = |reason, hide_screen| SessionDisconnect {
            reason,
            message: "kicked".into(),
            filtered_message: "k*****".into(),
            hide_screen,
        };
        assert_eq!(
            disconnect_reason(25),
            ConnectionDisconnectFailReason::Serverfull
        );
        assert_eq!(
            disconnect_reason(i32::MAX),
            ConnectionDisconnectFailReason::Unknown2(i32::MAX)
        );
        let shown = ServerDisconnectEvent::from_session(&disconnect(25, false));
        assert_eq!(shown.reason, "Serverfull");
        assert_eq!(
            (shown.message.as_deref(), shown.filtered_message.as_deref()),
            (Some("kicked"), Some("k*****"))
        );
        let hidden = ServerDisconnectEvent::from_session(&disconnect(i32::MAX, true));
        assert_eq!(hidden.reason, format!("UnknownValue({})", i32::MAX));
        assert_eq!((hidden.message, hidden.filtered_message), (None, None));
    }

    #[test]
    fn normalization_ignores_non_disconnect_packets() {
        let other = packets::Packet::SetTime(packets::SetTime {
            time: bedrock_protocol_1_26_51::wire::ZigZag32(7),
        });
        assert!(ServerDisconnectEvent::from_packet_data(&other).is_none());
    }
}
