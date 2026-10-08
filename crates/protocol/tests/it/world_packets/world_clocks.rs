use super::*;
use protocol::{
    OVERWORLD_CLOCK_ID, OVERWORLD_CLOCK_NAME, WorldClockDefinition, WorldClockState,
    WorldClockUpdateEvent,
};
use valentine::bedrock::version::v1_26_51::{
    SyncWorldClockStateData, SyncWorldClocksPacket, SyncWorldClocksPacketData,
    SyncWorldClocksPacketPayloadInitializeRegistryData, SyncWorldClocksPacketPayloadSyncStateData,
    WorldClockData,
};

#[test]
fn world_clock_initialization_retains_server_identity_signed_time_pause_and_order() {
    let packet = SyncWorldClocksPacket {
        data: SyncWorldClocksPacketData::InitializeRegistryData(
            SyncWorldClocksPacketPayloadInitializeRegistryData {
                clock_data: vec![
                    WorldClockData {
                        id: u64::MAX,
                        name: "custom:clock".into(),
                        time: -1,
                        is_paused: false,
                        ..Default::default()
                    },
                    WorldClockData {
                        id: 31,
                        name: OVERWORLD_CLOCK_NAME.into(),
                        time: i32::MIN,
                        is_paused: true,
                        ..Default::default()
                    },
                ],
            },
        ),
    };
    let mut bytes = BytesMut::new();
    packet.encode(&mut bytes).expect("encode clocks");
    let decoded = SyncWorldClocksPacket::decode(&mut bytes.freeze(), ()).expect("decode clocks");
    assert_eq!(
        into_world_event(decoded.into(), 2).unwrap(),
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
    let packet = SyncWorldClocksPacket {
        data: SyncWorldClocksPacketData::SyncStateData(SyncWorldClocksPacketPayloadSyncStateData {
            clock_data: vec![SyncWorldClockStateData {
                clock_id: 31,
                time: 18_000,
                is_paused: true,
            }],
        }),
    };
    assert_eq!(
        into_world_event(packet.into(), 1).unwrap(),
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
    let packet = SyncWorldClocksPacket {
        data: SyncWorldClocksPacketData::InitializeRegistryData(
            SyncWorldClocksPacketPayloadInitializeRegistryData {
                clock_data: vec![
                    WorldClockData {
                        id: foreign_id,
                        name: OVERWORLD_CLOCK_NAME.into(),
                        time: 18_000,
                        ..Default::default()
                    },
                    WorldClockData {
                        id: OVERWORLD_CLOCK_ID,
                        name: "custom:well_formed_alias".into(),
                        time: 6_000,
                        ..Default::default()
                    },
                ],
            },
        ),
    };
    assert_eq!(
        into_world_event(packet.into(), 0).unwrap(),
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
        SyncWorldClocksPacketData::AddTimeMarkerData(Default::default()),
        SyncWorldClocksPacketData::RemoveTimeMarkerData(Default::default()),
    ] {
        let packet = SyncWorldClocksPacket { data };
        assert_eq!(into_world_event(packet.clone().into(), 0).unwrap(), None);
        let mut bytes = BytesMut::new();
        packet.encode(&mut bytes).expect("encode markers");
        bytes.truncate(bytes.len() - 1);
        assert!(SyncWorldClocksPacket::decode(&mut bytes.freeze(), ()).is_err());
    }
}

#[test]
fn declared_absent_clock_rows_fail_before_untrusted_collection_reservation() {
    use valentine::bedrock::codec::VarUInt;
    let mut bytes = BytesMut::new();
    VarUInt(1).encode(&mut bytes).expect("initialization type");
    VarUInt(u32::MAX).encode(&mut bytes).expect("row count");
    assert!(SyncWorldClocksPacket::decode(&mut bytes.freeze(), ()).is_err());
}
