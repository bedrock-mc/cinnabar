//! Disconnect wire fixtures emitted by the pinned public Go packet codec.
use bedrock_protocol_1_26_51::packets;
use bedrock_protocol_1_26_51::types::DisconnectMessages;
use bytes::Bytes;
use protocol::{decode_batch, encode};

const VISIBLE: &[u8] = include_bytes!("../../fixtures/disconnect_visible.bin");
const FILTERED: &[u8] = include_bytes!("../../fixtures/disconnect_filtered.bin");
const HIDDEN: &[u8] = include_bytes!("../../fixtures/disconnect_hidden.bin");

#[test]
fn server_disconnect_wire_preserves_conditional_messages() {
    for (wire, hidden, message, filtered) in [
        (VISIBLE, false, "Server closing", ""),
        (FILTERED, false, "Original message", "Filtered message"),
        (HIDDEN, true, "", ""),
    ] {
        let packets = decode_batch(Bytes::from_static(wire)).expect("server wire decodes");
        assert_eq!(packets.len(), 1);
        let packets::Packet::Disconnect(disconnect) = &*packets[0].data else {
            panic!("expected Disconnect");
        };
        match &disconnect.messages {
            DisconnectMessages::DisconnectPacketMessages {
                message: wire_message,
                filtered_message,
            } => {
                assert!(!hidden);
                assert_eq!(*wire_message, message);
                assert_eq!(*filtered_message, filtered);
            }
            DisconnectMessages::Empty => {
                assert!(hidden);
                assert_eq!((message, filtered), ("", ""));
            }
        }
        assert_eq!(encode(&packets[0]).unwrap().as_ref(), wire);
    }
}

#[test]
fn truncated_or_extended_disconnect_bodies_remain_fatal() {
    for wire in [VISIBLE, FILTERED, HIDDEN] {
        assert!(wire.len() < 128);
        let mut truncated = wire[..wire.len() - 1].to_vec();
        truncated[1] -= 1;
        assert!(decode_batch(Bytes::from(truncated)).is_err());
        let mut extended = wire.to_vec();
        extended[1] += 1;
        extended.push(0);
        assert!(decode_batch(Bytes::from(extended)).is_err());
    }
}
