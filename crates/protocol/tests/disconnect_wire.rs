//! Disconnect wire fixtures emitted by the pinned public Go packet codec.
use bytes::{Buf, Bytes};
use protocol::{BedrockSession, decode_batch, encode};
use valentine::bedrock::version::v1_26_44::{BorrowedMcpePacket, McpePacketData};

const VISIBLE: &[u8] = include_bytes!("../fixtures/disconnect_visible.bin");
const FILTERED: &[u8] = include_bytes!("../fixtures/disconnect_filtered.bin");
const HIDDEN: &[u8] = include_bytes!("../fixtures/disconnect_hidden.bin");

#[test]
fn server_disconnect_wire_preserves_conditional_messages() {
    let session = BedrockSession { shield_item_id: 0 };
    for (wire, hidden, message, filtered) in [
        (VISIBLE, false, "Server closing", ""),
        (FILTERED, false, "Original message", "Filtered message"),
        (HIDDEN, true, "", ""),
    ] {
        let packets =
            decode_batch(Bytes::from_static(wire), &session).expect("server wire decodes");
        assert_eq!(packets.len(), 1);
        let McpePacketData::DisconnectPacket(disconnect) = &packets[0].data else {
            panic!("expected Disconnect");
        };
        assert_eq!(disconnect.hide_disconnection_screen, hidden);
        assert_eq!(disconnect.messages.message, message);
        assert_eq!(disconnect.messages.filtered_message, filtered);
        assert_eq!(encode(&packets[0], &session).unwrap().as_ref(), wire);
        let mut frame = Bytes::from_static(wire).slice(1..);
        BorrowedMcpePacket::decode_inner(&mut frame).expect("borrowed server wire decodes");
        assert!(!frame.has_remaining());
    }
}

#[test]
fn truncated_or_extended_disconnect_bodies_remain_fatal() {
    let session = BedrockSession { shield_item_id: 0 };
    for wire in [VISIBLE, FILTERED, HIDDEN] {
        assert!(wire.len() < 128);
        let mut truncated = wire[..wire.len() - 1].to_vec();
        truncated[1] -= 1;
        assert!(decode_batch(Bytes::from(truncated), &session).is_err());
        let mut extended = wire.to_vec();
        extended[1] += 1;
        extended.push(0);
        assert!(decode_batch(Bytes::from(extended), &session).is_err());
    }
}
