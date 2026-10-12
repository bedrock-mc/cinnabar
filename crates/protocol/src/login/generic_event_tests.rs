//! LevelEventGeneric ingress: its event data is loose NBT tags to the end of the packet.

use crate::raw_packet::decode_packet_raw;
use bytes::{BufMut, BytesMut};

use valentine::bedrock::version::v1_26_51::McpePacketName;

use super::*;

fn raw_generic(event_id: i32, tags: &[u8]) -> RawPacket {
    let mut payload = BytesMut::new();
    wire::write_var_u32(&mut payload, McpePacketName::LevelEventGenericPacket as u32);
    wire::write_var_u32(&mut payload, ((event_id << 1) ^ (event_id >> 31)) as u32);
    payload.put_slice(tags);
    let mut frame = BytesMut::new();
    wire::write_var_u32(&mut frame, payload.len() as u32);
    frame.put_slice(&payload);
    decode_packet_raw(&mut frame.freeze()).expect("raw packet")
}

/// Two loose int tags as BDS sends them: no root compound header, no end tag.
fn loose_ints(tags: &[(&str, u8)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in tags {
        out.extend_from_slice(&[3, name.len() as u8]);
        out.extend_from_slice(name.as_bytes());
        out.push(value << 1);
    }
    out
}

/// BDS 1.26.52 sends generic events at join; their trailing tags must not end the session.
#[test]
fn loose_generic_event_tags_decode_and_sleep_status_gets_a_rooted_compound() {
    let other = raw_generic(2025, &loose_ints(&[("originX", 3), ("originY", 64)]));
    assert_eq!(
        decode_world_raw_with(other, 0, super::decode_test_raw).expect("decodes"),
        None
    );
    let tags = loose_ints(&[("sleepingPlayerCount", 1), ("overworldPlayerCount", 2)]);
    let sleep = raw_generic(9801, &tags);
    let event = decode_world_raw_with(sleep, 0, super::decode_test_raw)
        .expect("decodes")
        .expect("sleep status");
    let WorldEvent::Ui(crate::UiEvent::SleepStatus(status)) = event else {
        panic!("unexpected event {event:?}");
    };
    let mut rooted = vec![0x0a, 0x00];
    rooted.extend_from_slice(&tags);
    rooted.push(0);
    assert_eq!(status.nbt.as_ref(), rooted.as_slice());
}
