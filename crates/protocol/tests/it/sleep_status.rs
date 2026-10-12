//! The world's sleep status, which servers send as a generic level event.

use bedrock_protocol_1_26_51::packets::LevelEventGeneric;
use bedrock_protocol_1_26_51::wire;
use bytes::Bytes;
use protocol::{SleepStatusEvent, UiEvent, WorldEvent, into_world_event};

/// A NetworkLittleEndian root compound of int tags.
fn int_compound(tags: &[(&str, i32)]) -> Vec<u8> {
    let mut out = vec![10, 0];
    for (name, value) in tags {
        out.push(3);
        out.push(name.len() as u8);
        out.extend_from_slice(name.as_bytes());
        let mut zigzag = ((value << 1) ^ (value >> 31)) as u32;
        loop {
            let byte = (zigzag & 0x7f) as u8;
            zigzag >>= 7;
            if zigzag == 0 {
                out.push(byte);
                break;
            }
            out.push(byte | 0x80);
        }
    }
    out.push(0);
    out
}

#[test]
fn sleeping_players_generic_level_event_carries_its_compound() {
    let nbt = int_compound(&[("sleepingPlayerCount", 1), ("overworldPlayerCount", 3)]);
    // On the wire the tags float loose: no root compound header and no closing end tag.
    let loose = Bytes::copy_from_slice(&nbt[2..nbt.len() - 1]);
    let packet = LevelEventGeneric {
        event_id: wire::ZigZag32(9801),
        ctd: wire::NetworkNbt(loose.clone()),
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 0).unwrap(),
        Some(WorldEvent::Ui(UiEvent::SleepStatus(SleepStatusEvent {
            nbt: nbt.into()
        })))
    );
    let other = LevelEventGeneric {
        event_id: wire::ZigZag32(1),
        ctd: wire::NetworkNbt(Bytes::from(int_compound(&[]))),
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(other), 0).unwrap(),
        None
    );
}
