use super::*;
use bedrock_protocol_1_26_51::{packets, types::DisconnectMessages};

#[tokio::test]
async fn the_real_frame_cursor_preserves_prefix_and_terminal_after_driver_channel_completion() {
    let mut first = crate::Packet::new(packets::SetTime {
        time: wire::ZigZag32(31),
    });
    first.header.from_subclient = 1;
    first.header.to_subclient = 2;
    let reason = packets::Disconnect::default().reason;
    let terminal = crate::Packet::new(packets::Disconnect {
        reason,
        messages: DisconnectMessages::DisconnectPacketMessages {
            message: "original reason".into(),
            filtered_message: "filtered reason".into(),
        },
    });
    let after = crate::Packet::new(packets::SetTime {
        time: wire::ZigZag32(99),
    });
    let body = crate::session_transport::batch_body(&[first, terminal, after]).unwrap();
    let (send, incoming) = mpsc::channel(1);
    send.send(IncomingBody {
        body: Ok(body),
        permit: None,
    })
    .await
    .unwrap();
    drop(send);
    let (completed, tail) = oneshot::channel();
    completed.send(None).unwrap();
    let mut incoming = NativeRelayInbound {
        incoming,
        tail: Some(tail),
        current: None,
        ended: false,
    };
    let CoreMessage::Batch(packets) = incoming.recv().await.unwrap() else {
        panic!("accepted prefix must precede its actual terminal")
    };
    assert_eq!(packets.len(), 1);
    let mut reader = wire::Reader::from_shared(&packets[0]);
    let header = wire::PacketHeader::from_raw(reader.read_var_u32().unwrap());
    assert_eq!((header.sender_subclient, header.target_subclient), (1, 2));
    assert_eq!(header.id, PacketId::SetTime as u32);
    let packets::Packet::SetTime(first) =
        packets::Packet::decode_body(PacketId::SetTime, &mut reader).unwrap()
    else {
        panic!("original prefix packet")
    };
    assert_eq!(first.time.0, 31);
    let CoreMessage::Disconnect(actual) = incoming.recv().await.unwrap() else {
        panic!("the actual terminal must survive completed driver input")
    };
    assert_eq!(actual.reason, reason.to_raw());
    assert_eq!(actual.message, "original reason");
    assert_eq!(actual.filtered_message, "filtered reason");
    assert!(!actual.hide_screen);
    assert_eq!(
        incoming.recv().await.unwrap_err().kind(),
        io::ErrorKind::UnexpectedEof
    );
}

mod byte_backpressure;
