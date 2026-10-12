//! `BlockEvent` ingress: container cues reach the world stream with their dimension.

use crate::raw_packet::decode_packet_raw;
use bytes::Buf;

use valentine::bedrock::version::v1_26_51::{
    ActorUniqueId, BlockEventPacket, BlockPos, ClientboundMapItemDataPacket, OpenSignPacket,
};

use super::*;
use crate::{BlockEventEvent, MapDataEvent, OpenSignEvent, WorldEvent};

#[test]
fn block_event_packets_normalize_with_the_current_dimension() {
    let packet: Packet = crate::Packet::new(BlockEventPacket {
        block_position: BlockPos { x: 4, y: 70, z: -9 },
        event_type: 1,
        event_value: 2,
    });
    let mut batch = crate::encode(&packet).expect("encode block event");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw block event");

    let event = decode_world_raw_with(raw, 1, super::decode_test_raw)
        .expect("decode block event")
        .expect("world event");

    assert_eq!(
        event,
        WorldEvent::BlockEvent(BlockEventEvent {
            dimension: 1,
            position: [4, 70, -9],
            event_type: 1,
            event_value: 2,
        })
    );
}

fn map_packet(width: i32, start_x: i32, pixels: Vec<u32>) -> ClientboundMapItemDataPacket {
    ClientboundMapItemDataPacket {
        map_id: ActorUniqueId {
            actor_unique_id: 77,
        },
        width: Some(width),
        height: Some(2),
        start_x: Some(start_x),
        start_y: Some(3),
        pixels: Some(pixels),
        ..Default::default()
    }
}

fn decode_map(packet: ClientboundMapItemDataPacket) -> Option<WorldEvent> {
    let packet: Packet = crate::Packet::new(packet);
    let mut batch = crate::encode(&packet).expect("encode map data");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw map data");
    decode_world_raw_with(raw, 0, super::decode_test_raw).expect("decode map data")
}

#[test]
fn map_pixel_updates_normalize_and_out_of_range_rectangles_are_skipped() {
    assert_eq!(
        decode_map(map_packet(2, 4, vec![1, 2, 3, 4])),
        Some(WorldEvent::MapData(MapDataEvent {
            map_id: 77,
            start_x: 4,
            start_y: 3,
            width: 2,
            height: 2,
            pixels: vec![1, 2, 3, 4].into(),
        }))
    );
    // Past the 128-pixel edge, and a pixel count that does not match the rectangle.
    assert_eq!(decode_map(map_packet(2, 127, vec![1, 2, 3, 4])), None);
    assert_eq!(decode_map(map_packet(2, 0, vec![1, 2, 3])), None);
}

#[test]
fn open_sign_packets_normalize_with_position_side_and_dimension() {
    let packet: Packet = crate::Packet::new(OpenSignPacket {
        pos: BlockPos {
            x: -5,
            y: 64,
            z: 12,
        },
        is_front_side: false,
    });
    let mut batch = crate::encode(&packet).expect("encode open sign");
    batch.advance(1);
    let raw = decode_packet_raw(&mut batch).expect("raw open sign");
    let event = decode_world_raw_with(raw, 2, super::decode_test_raw)
        .expect("decode open sign")
        .expect("world event");
    assert_eq!(
        event,
        WorldEvent::OpenSign(OpenSignEvent {
            dimension: 2,
            position: [-5, 64, 12],
            front: false,
        })
    );
}
