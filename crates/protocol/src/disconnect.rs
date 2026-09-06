use valentine::bedrock::version::v1_26_44::{EnumsConnectionDisconnectFailReason, McpePacketData};

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
    pub(crate) fn from_packet_data(data: &McpePacketData) -> Option<Self> {
        let McpePacketData::DisconnectPacket(packet) = data else {
            return None;
        };
        Some(Self {
            reason: reason_label(&packet.reason),
            message: bounded_text(&packet.messages.message),
            filtered_message: bounded_text(&packet.messages.filtered_message),
        })
    }
}

fn reason_label(reason: &EnumsConnectionDisconnectFailReason) -> String {
    clamp_bytes(&format!("{reason:?}"), MAX_REASON_LABEL_BYTES)
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
    use valentine::bedrock::version::v1_26_44::{DisconnectPacket, DisconnectPacketMessages};

    fn disconnect_data(
        reason: EnumsConnectionDisconnectFailReason,
        message: &str,
        filtered_message: &str,
    ) -> McpePacketData {
        McpePacketData::DisconnectPacket(Box::new(DisconnectPacket {
            reason,
            hide_disconnection_screen: false,
            messages: DisconnectPacketMessages {
                message: message.to_owned(),
                filtered_message: filtered_message.to_owned(),
            },
        }))
    }

    #[test]
    fn normalization_retains_the_server_reason_fields() {
        let event = ServerDisconnectEvent::from_packet_data(&disconnect_data(
            EnumsConnectionDisconnectFailReason::Kicked,
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
            EnumsConnectionDisconnectFailReason::NoReason,
            "",
            "",
        ))
        .expect("disconnect packet normalizes");
        assert_eq!(empty.message, None);
        assert_eq!(empty.filtered_message, None);

        let oversized = ServerDisconnectEvent::from_packet_data(&disconnect_data(
            EnumsConnectionDisconnectFailReason::Unknown,
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
            EnumsConnectionDisconnectFailReason::KickedForExploit,
            "x",
            "",
        ))
        .expect("known reason normalizes");
        assert_eq!(known.reason, "KickedForExploit");

        let unknown = ServerDisconnectEvent::from_packet_data(&disconnect_data(
            EnumsConnectionDisconnectFailReason::UnknownValue(-7),
            "x",
            "",
        ))
        .expect("unknown reason normalizes");
        assert_eq!(unknown.reason, "UnknownValue(-7)");
    }

    #[test]
    fn normalization_ignores_non_disconnect_packets() {
        let other =
            McpePacketData::SetTimePacket(valentine::bedrock::version::v1_26_44::SetTimePacket {
                time: 7,
            });
        assert!(ServerDisconnectEvent::from_packet_data(&other).is_none());
    }
}
