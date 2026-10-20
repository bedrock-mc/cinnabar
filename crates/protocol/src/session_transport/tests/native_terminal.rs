//! Terminal metadata does not validate unused bodies or weaken ordinary batch framing.

use super::*;
use bedrock_protocol_1_26_51::{packets, types::DisconnectMessages};

#[test]
fn native_terminal_projection_skips_unknown_unused_and_invalid_transfer_before_first_cause() {
    let unknown = (0_u32..)
        .find(|id| PacketId::from_raw(*id).is_none())
        .unwrap();
    assert_eq!(wire::PacketHeader::from_raw(unknown).id, unknown);
    let mut body = Vec::new();
    append_raw(&mut body, unknown, &[0xff]);
    append_raw(&mut body, PacketId::SetTime as u32, &[0x80]);
    body.extend_from_slice(&batch_body(&[transfer(" ", 19133)]).unwrap());
    body.extend_from_slice(&batch_body(&[disconnect()]).unwrap());
    body.extend_from_slice(&batch_body(&[transfer("later.example.test", 19134)]).unwrap());

    let Some(Terminal::Disconnect(terminal)) = native_batch_terminal(&body.into()).unwrap() else {
        panic!("unused bodies or an unusable transfer masked the first valid terminal")
    };
    assert_eq!(terminal.message, "original reason");
    assert_eq!(terminal.filtered_message, "original filtered reason");
}

#[test]
fn native_terminal_projection_retains_first_transfer_and_rejects_malformed_tail() {
    let body = batch_body(&[transfer("first.example.test", 19133), disconnect()]).unwrap();
    let Some(Terminal::Transfer(terminal)) = native_batch_terminal(&body).unwrap() else {
        panic!("the first valid transfer must outrank the later disconnect")
    };
    assert_eq!(terminal.address, "first.example.test");
    assert_eq!(terminal.port, 19133);
    let mut malformed = body.to_vec();
    wire::Writer::new(&mut malformed).write_var_u32(2).unwrap();
    malformed.push(0);
    assert!(matches!(
        native_batch_terminal(&malformed.into()),
        Err(SessionError(SessionFailure::TruncatedEntry {
            declared: 2,
            available: 1,
        }))
    ));
}

/// Appends one entry through the canonical length/header writer, without decoding its payload.
fn append_raw(body: &mut Vec<u8>, id: u32, payload: &[u8]) {
    let mut packet = Vec::new();
    wire::Writer::new(&mut packet).write_var_u32(id).unwrap();
    packet.extend_from_slice(payload);
    wire::Writer::new(body).write_byte_slice(&packet).unwrap();
}

/// Uses the generated Transfer packet and ordinary target policy input.
fn transfer(address: &str, port: u16) -> Packet {
    Packet::new(packets::Transfer {
        server_address: address.into(),
        server_port: wire::U16LE(port),
        reload_world: true,
        ..Default::default()
    })
}

/// Uses the generated Disconnect packet with both original display strings.
fn disconnect() -> Packet {
    Packet::new(packets::Disconnect {
        messages: DisconnectMessages::DisconnectPacketMessages {
            message: "original reason".into(),
            filtered_message: "original filtered reason".into(),
        },
        ..Default::default()
    })
}
