use bedrock_protocol_1_26_51::{
    enums::{TextPacketType, TitleType},
    packets::{SetTitle, Text},
    types::TextData,
    wire::{self, Encode},
};
use protocol::{
    Packet, UiEvent, UiPacketError, WorldEvent, decode_batch, encode, into_world_event,
};

use crate::ui::MAX_TEXT_MESSAGE_BYTES as TEXT_SCHEMA_MAX_BYTES;

/// Builds each Text payload shape with the same message and filtered message.
fn text_packets(message: &str, filtered: &str) -> [Text; 3] {
    [
        TextData::MessageOnly {
            message_type: TextPacketType::Raw,
            message: message.into(),
        },
        TextData::AuthorAndMessage {
            message_type: TextPacketType::Chat,
            player_name: "sender".into(),
            message: message.into(),
        },
        TextData::MessageAndParams {
            message_type: TextPacketType::Translate,
            message: message.into(),
            parameter_list: vec!["parameter".into()],
        },
    ]
    .map(|body| Text {
        body,
        filtered_message: Some(filtered.into()),
        ..Default::default()
    })
}

/// Normalizes a packet through the world receive path and returns its literal Text event.
fn normalize(packet: Packet) -> protocol::TextEvent {
    match into_world_event(packet, 0).expect("normalize admitted Text") {
        Some(WorldEvent::Ui(UiEvent::Text(event))) => event,
        other => panic!("expected literal Text, got {other:?}"),
    }
}

#[test]
fn text_messages_keep_schema_sized_text_through_wire_normalization() {
    for len in [protocol::MAX_UI_TEXT_BYTES + 1, TEXT_SCHEMA_MAX_BYTES] {
        let message = "é".repeat(len / 2) + if len % 2 == 0 { "" } else { "x" };
        for text in text_packets(&message, "filtered") {
            let event = normalize(Packet::new(text.clone()));
            assert_eq!(event.message.as_ref(), message);
            let bytes = encode(&Packet::new(text)).expect("encode schema-sized message");
            let packet = decode_batch(bytes)
                .expect("admit schema-sized message")
                .pop()
                .unwrap();
            assert_eq!(normalize(packet).message.as_ref(), message);
        }
    }
}

#[test]
fn text_filtered_message_keeps_its_schema_bound() {
    let filtered = "x".repeat(TEXT_SCHEMA_MAX_BYTES);
    for text in text_packets("message", &filtered) {
        assert_eq!(
            normalize(Packet::new(text.clone()))
                .filtered_message
                .as_deref(),
            Some(filtered.as_str())
        );
        let bytes = encode(&Packet::new(text)).unwrap();
        let packet = decode_batch(bytes)
            .expect("admit filtered message")
            .pop()
            .unwrap();
        assert_eq!(
            normalize(packet).filtered_message.as_deref(),
            Some(filtered.as_str())
        );
    }
}

#[test]
fn text_message_bounds_match_the_generated_codec() {
    for len in [TEXT_SCHEMA_MAX_BYTES, TEXT_SCHEMA_MAX_BYTES + 1] {
        let value = "x".repeat(len);
        for text in text_packets(&value, "")
            .into_iter()
            .chain(text_packets("message", &value))
        {
            assert_eq!(text.encode_to_vec().is_ok(), len == TEXT_SCHEMA_MAX_BYTES);
            let normalized = into_world_event(Packet::new(text), 0);
            if len == TEXT_SCHEMA_MAX_BYTES {
                assert!(normalized.is_ok(), "{normalized:?}");
            } else {
                assert!(
                    matches!(normalized, Err(protocol::WorldPacketError::Ui(UiPacketError::TextTooLong { bytes, max })) if bytes == len && max == TEXT_SCHEMA_MAX_BYTES)
                );
            }
        }
    }
}

#[test]
fn text_wire_schema_rejects_oversized_messages_and_filtered_messages() {
    let oversized = "x".repeat(TEXT_SCHEMA_MAX_BYTES + 1);
    for (message, filtered) in [(oversized.as_str(), ""), ("message", oversized.as_str())] {
        let mut batch = vec![wire::BATCH_HEADER];
        // Write strings without schema checks to exercise the generated decoder's own bound.
        wire::encode_frame_with(
            &mut batch,
            wire::PacketHeader::new(Text::ID),
            usize::MAX,
            |writer| {
                false.encode(writer)?;
                wire::VarUInt(0).encode(writer)?;
                TextPacketType::Raw.encode(writer)?;
                wire::Str::from(message).encode(writer)?;
                wire::Str::default().encode(writer)?;
                wire::Str::default().encode(writer)?;
                true.encode(writer)?;
                wire::Str::from(filtered).encode(writer)
            },
        )
        .unwrap();
        assert!(matches!(
            decode_batch(batch.into()),
            Err(protocol::ProtocolError::Ui(
                UiPacketError::SchemaViolation { .. }
            ))
        ));
    }
}

#[test]
fn text_long_ordinary_json_remains_literal() {
    for message in [
        "{".to_owned() + &"x".repeat(TEXT_SCHEMA_MAX_BYTES - 1),
        format!(
            "{{\"text\":\"{}\"}}",
            "x".repeat(TEXT_SCHEMA_MAX_BYTES - 11)
        ),
    ] {
        assert_eq!(message.len(), TEXT_SCHEMA_MAX_BYTES);
        let text = text_packets(&message, "")[0].clone();
        let bytes = encode(&Packet::new(text)).unwrap();
        let packet = decode_batch(bytes)
            .expect("admit ordinary JSON text")
            .pop()
            .unwrap();
        assert_eq!(normalize(packet).message.as_ref(), message);
    }
}

#[test]
fn text_message_allowance_does_not_raise_other_ui_limits() {
    let value = "x".repeat(protocol::MAX_UI_TEXT_BYTES + 1);
    let mut text = text_packets("message", "")[2].clone();
    let TextData::MessageAndParams { parameter_list, .. } = &mut text.body else {
        unreachable!()
    };
    *parameter_list = vec![value.clone().into()];
    for packet in [
        Packet::new(text),
        Packet::new(SetTitle {
            title_type: TitleType::Title,
            title_text: value.into(),
            ..Default::default()
        }),
    ] {
        assert!(matches!(
            into_world_event(packet, 0),
            Err(protocol::WorldPacketError::Ui(UiPacketError::TextTooLong {
                max: protocol::MAX_UI_TEXT_BYTES,
                ..
            }))
        ));
    }
}
