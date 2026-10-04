use super::*;
use protocol::{WorldClockDefinition, WorldClockState, WorldClockUpdateEvent};

#[test]
fn named_clock_rows_keep_packet_fifo_and_survive_dimension_changes() {
    let mut stream = WorldStream::new(WorldBootstrap {
        dimension: 0,
        local_player_runtime_id: 1,
        local_player_unique_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let initialize = WorldClockUpdateEvent::Initialize(WorldClockDefinition {
        id: 31,
        time: 6_000,
        paused: false,
    });
    let other = WorldClockUpdateEvent::Initialize(WorldClockDefinition {
        id: 32,
        time: 18_000,
        paused: true,
    });
    let sync = WorldClockUpdateEvent::Sync(WorldClockState {
        id: 31,
        time: 12_000,
        paused: true,
    });
    stream
        .submit(1, WorldEvent::WorldClocks(vec![initialize, other]))
        .expect("initialize clocks");
    stream
        .submit(2, WorldEvent::SetTime(SetTimeEvent { time: -1 }))
        .expect("legacy time");
    stream
        .submit(3, WorldEvent::WorldClocks(vec![sync]))
        .expect("clock state");
    let controls = stream.take_committed_controls();
    assert_eq!(
        controls,
        vec![
            CommittedControlEvent::WorldClocks {
                sequence: 1,
                update: initialize
            },
            CommittedControlEvent::WorldClocks {
                sequence: 1,
                update: other
            },
            CommittedControlEvent::SetTime {
                sequence: 2,
                update: SetTimeEvent { time: -1 }
            },
            CommittedControlEvent::WorldClocks {
                sequence: 3,
                update: sync
            },
        ]
    );
    assert!(stream.mesh_jobs.pending.is_empty());
    assert!(stream.mesh_changes.is_empty());
    stream
        .submit(
            4,
            WorldEvent::ChangeDimension(ChangeDimensionEvent {
                dimension: 1,
                position: [0.0; 3],
            }),
        )
        .expect("dimension change");
    stream
        .submit(5, WorldEvent::WorldClocks(vec![sync]))
        .expect("clock remains session-scoped");
    assert!(matches!(
        stream.take_committed_controls().last(),
        Some(CommittedControlEvent::WorldClocks { sequence: 5, update }) if *update == sync
    ));
}
