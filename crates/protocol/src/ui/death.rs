//! The local player's server-authored death reason, independent of chat broadcasts.

use std::sync::Arc;

use valentine::bedrock::version::v1_26_51::DeathInfoPacket;

use super::{MAX_CHAT_PARAMETERS, UiEvent, UiPacketError, bounded_text};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeathInfoEvent {
    pub message: Arc<str>,
    pub parameters: Arc<[Arc<str>]>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BedrockSession, ProtocolError, decode_batch, encode};

    #[test]
    fn death_information_rejects_parameter_overflow_before_owned_decode() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet = DeathInfoPacket {
            death_cause_attack_name: String::new(),
            death_cause_message_list: vec![String::new(); MAX_CHAT_PARAMETERS + 1],
        }
        .into();
        let wire = encode(&packet, &session).unwrap();
        assert!(matches!(
            decode_batch(wire, &session),
            Err(ProtocolError::Ui(
                UiPacketError::TooManyChatParameters { .. }
            ))
        ));
    }

    #[test]
    fn death_information_rejects_invalid_utf8_in_reason_and_parameter() {
        let session = BedrockSession { shield_item_id: 0 };
        let packet = DeathInfoPacket {
            death_cause_attack_name: "reason".into(),
            death_cause_message_list: vec!["argument".into()],
        }
        .into();
        let wire = encode(&packet, &session).unwrap();
        for field in [b"reason".as_slice(), b"argument".as_slice()] {
            let mut invalid = wire.to_vec();
            let start = invalid
                .windows(field.len())
                .position(|value| value == field)
                .unwrap();
            invalid[start] = 0xff;
            assert!(matches!(
                decode_batch(invalid.into(), &session),
                Err(ProtocolError::Ui(UiPacketError::InvalidUtf8 { .. }))
            ));
        }
    }
}

/// Bounds the translation key and ordered parameters before retaining them.
pub(crate) fn normalize_death_info(packet: DeathInfoPacket) -> Result<UiEvent, UiPacketError> {
    if packet.death_cause_message_list.len() > MAX_CHAT_PARAMETERS {
        return Err(UiPacketError::TooManyChatParameters {
            count: packet.death_cause_message_list.len(),
            max: MAX_CHAT_PARAMETERS,
        });
    }
    Ok(UiEvent::DeathInfo(DeathInfoEvent {
        message: bounded_text(packet.death_cause_attack_name)?,
        parameters: packet
            .death_cause_message_list
            .into_iter()
            .map(bounded_text)
            .collect::<Result<Vec<_>, _>>()?
            .into(),
    }))
}
