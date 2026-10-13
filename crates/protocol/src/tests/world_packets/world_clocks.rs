use super::*;
use bedrock_protocol_1_26_51::packets::SyncWorldClocks;
use bedrock_protocol_1_26_51::types::{
    SyncWorldClockStateData, SyncWorldClocksData, WorldClockData,
};
use bedrock_protocol_1_26_51::wire::{Decode, Encode};
use protocol::{
    OVERWORLD_CLOCK_ID, OVERWORLD_CLOCK_NAME, WorldClockDefinition, WorldClockState,
    WorldClockUpdateEvent,
};

#[test]
fn world_clock_initialization_retains_server_identity_signed_time_pause_and_order() {
    let packet = SyncWorldClocks {
        data: SyncWorldClocksData::InitializeRegistryData {
            clock_data: vec![
                WorldClockData {
                    id: wire::VarULong(u64::MAX),
                    name: "custom:clock".into(),
                    time: wire::ZigZag32(-1),
                    is_paused: false,
                    ..Default::default()
                },
                WorldClockData {
                    id: wire::VarULong(31),
                    name: OVERWORLD_CLOCK_NAME.into(),
                    time: wire::ZigZag32(i32::MIN),
                    is_paused: true,
                    ..Default::default()
                },
            ],
        },
    };
    let bytes = packet.encode_to_vec().expect("encode clocks");
    let decoded = SyncWorldClocks::decode_exact(&bytes).expect("decode clocks");
    assert_eq!(
        into_world_event(protocol::Packet::new(decoded), 2).unwrap(),
        Some(WorldEvent::WorldClocks(vec![
            WorldClockUpdateEvent::Initialize(WorldClockDefinition {
                id: u64::MAX,
                time: -1,
                paused: false,
            }),
            WorldClockUpdateEvent::Initialize(WorldClockDefinition {
                id: 31,
                time: i32::MIN,
                paused: true,
            }),
        ]))
    );
}

#[test]
fn world_clock_state_sync_keeps_pause_separate_from_time_and_dimension() {
    let packet = SyncWorldClocks {
        data: SyncWorldClocksData::SyncStateData {
            clock_data: vec![SyncWorldClockStateData {
                clock_id: wire::VarULong(31),
                time: wire::ZigZag32(18_000),
                is_paused: true,
            }],
        },
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 1).unwrap(),
        Some(WorldEvent::WorldClocks(vec![WorldClockUpdateEvent::Sync(
            WorldClockState {
                id: 31,
                time: 18_000,
                paused: true
            }
        )]))
    );
}

#[test]
fn world_clock_packet_names_never_substitute_for_the_native_hashed_identity() {
    let foreign_id = OVERWORLD_CLOCK_ID.wrapping_add(1);
    let packet = SyncWorldClocks {
        data: SyncWorldClocksData::InitializeRegistryData {
            clock_data: vec![
                WorldClockData {
                    id: wire::VarULong(foreign_id),
                    name: OVERWORLD_CLOCK_NAME.into(),
                    time: wire::ZigZag32(18_000),
                    ..Default::default()
                },
                WorldClockData {
                    id: wire::VarULong(OVERWORLD_CLOCK_ID),
                    name: "custom:well_formed_alias".into(),
                    time: wire::ZigZag32(6_000),
                    ..Default::default()
                },
            ],
        },
    };
    assert_eq!(
        into_world_event(protocol::Packet::new(packet), 0).unwrap(),
        Some(WorldEvent::WorldClocks(vec![
            WorldClockUpdateEvent::Initialize(WorldClockDefinition {
                id: foreign_id,
                time: 18_000,
                paused: false,
            }),
            WorldClockUpdateEvent::Initialize(WorldClockDefinition {
                id: OVERWORLD_CLOCK_ID,
                time: 6_000,
                paused: false,
            }),
        ]))
    );
}

#[test]
fn clock_markers_have_no_time_consumer_but_truncated_markers_are_wire_errors() {
    for data in [
        SyncWorldClocksData::AddTimeMarkerData {
            clock_id: Default::default(),
            time_markers: Vec::new(),
        },
        SyncWorldClocksData::RemoveTimeMarkerData {
            clock_id: Default::default(),
            time_marker_ids: Vec::new(),
        },
    ] {
        let packet = SyncWorldClocks { data };
        assert_eq!(
            into_world_event(protocol::Packet::new(packet.clone()), 0).unwrap(),
            None
        );
        let mut bytes = packet.encode_to_vec().expect("encode markers");
        bytes.truncate(bytes.len() - 1);
        assert!(SyncWorldClocks::decode_exact(&bytes).is_err());
    }
}

#[test]
fn declared_absent_clock_rows_fail_before_untrusted_collection_reservation() {
    let mut bytes = Vec::new();
    let mut writer = wire::Writer::new(&mut bytes);
    writer.write_var_u32(1).expect("initialization type");
    writer.write_var_u32(u32::MAX).expect("row count");
    assert!(SyncWorldClocks::decode_exact(&bytes).is_err());
}
