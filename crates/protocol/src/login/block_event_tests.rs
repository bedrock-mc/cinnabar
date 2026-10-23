//! `BlockEvent` ingress: container cues reach the world stream with their dimension.

use crate::raw_packet::decode_packet_raw;
use bytes::Buf;

use bedrock_protocol_1_26_51::packets::{BlockEvent, ClientboundMapItemData, OpenSign};
use bedrock_protocol_1_26_51::types::{ActorUniqueID, BlockPos};

use super::*;
use crate::{BlockEventEvent, MapDataEvent, OpenSignEvent, WorldEvent};

#[test]
fn block_event_packets_normalize_with_the_current_dimension() {
    let packet: Packet = crate::Packet::new(BlockEvent {
        block_position: block_pos(4, 70, -9),
        event_type: wire::ZigZag32(1),
        event_value: wire::ZigZag32(2),
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

fn block_pos(x: i32, y: i32, z: i32) -> BlockPos {
    BlockPos {
        x: wire::ZigZag32(x),
        y: wire::ZigZag32(y),
        z: wire::ZigZag32(z),
    }
}

fn map_packet(width: i32, start_x: i32, pixels: Vec<u32>) -> ClientboundMapItemData {
    ClientboundMapItemData {
        map_id: ActorUniqueID(77),
        width: Some(wire::ZigZag32(width)),
        height: Some(wire::ZigZag32(2)),
        start_x: Some(wire::ZigZag32(start_x)),
        start_y: Some(wire::ZigZag32(3)),
        pixels: Some(pixels.into_iter().map(wire::U32LE).collect()),
        ..Default::default()
    }
}

fn decode_map(packet: ClientboundMapItemData) -> Option<WorldEvent> {
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
            initialized_ids: Default::default(),
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
    let packet: Packet = crate::Packet::new(OpenSign {
        pos: block_pos(-5, 64, 12),
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

#[test]
fn map_wire_colors_normalize_without_losing_channels_or_alpha() {
    let wire_pixels = [0xff00_00ff, 0x4433_2211, 0x0056_3412, 0x80ff_0000];
    let Some(WorldEvent::MapData(event)) = decode_map(map_packet(2, 0, wire_pixels.to_vec()))
    else {
        panic!("valid map pixels must reach the world");
    };
    let rgba: Vec<_> = event
        .pixels
        .iter()
        .map(|pixel| pixel.to_le_bytes())
        .collect();
    assert_eq!(
        rgba,
        [
            [0xff, 0x00, 0x00, 0xff],
            [0x11, 0x22, 0x33, 0x44],
            [0x12, 0x34, 0x56, 0x00],
            [0x00, 0x00, 0xff, 0x80]
        ],
        "wire map colors must become canonical RGBA"
    );
}

#[test]
fn creation_without_pixels_announces_an_unpainted_saved_map() {
    assert_eq!(
        decode_map(ClientboundMapItemData {
            map_id: ActorUniqueID(77),
            creation_map_ids: Some(vec![ActorUniqueID(77)]),
            scale: Some(wire::I8(4)),
            ..Default::default()
        }),
        Some(WorldEvent::MapData(MapDataEvent {
            map_id: 77,
            initialized_ids: vec![77].into(),
            start_x: 0,
            start_y: 0,
            width: 0,
            height: 0,
            pixels: Default::default(),
        }))
    );
    assert_eq!(decode_map(ClientboundMapItemData::default()), None);
}
