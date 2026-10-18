use super::*;

#[test]
fn clock_daylight_and_weather_commit_in_fifo_order_without_dirtying_world_meshes() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    let pending_mesh_count = stream.mesh_jobs.pending.len();
    let mesh_change_count = stream.mesh_changes.len();
    let connectivity_generation = stream.connectivity_generation;

    stream
        .submit(1, WorldEvent::SetTime(SetTimeEvent { time: i32::MIN }))
        .unwrap();
    stream
        .submit(
            2,
            WorldEvent::GameRules(protocol::GameRulesEvent {
                daylight_cycle: Some(DaylightCycleUpdateEvent { enabled: false }),
                weather_cycle: None,
                death: Default::default(),
                crafting: Default::default(),
                hud: protocol::HudRules {
                    show_coordinates: Some(true),
                    show_days_played: None,
                },
            }),
        )
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::Weather(WeatherUpdateEvent {
                channel: WeatherChannel::Rain,
                level: 1.0,
            }),
        )
        .unwrap();
    stream
        .submit(
            4,
            WorldEvent::Weather(WeatherUpdateEvent {
                channel: WeatherChannel::Lightning,
                level: 0.0,
            }),
        )
        .unwrap();

    assert_eq!(
        stream.take_committed_controls(),
        vec![
            super::CommittedControlEvent::SetTime {
                sequence: 1,
                update: SetTimeEvent { time: i32::MIN },
            },
            super::CommittedControlEvent::DaylightCycle {
                sequence: 2,
                update: DaylightCycleUpdateEvent { enabled: false },
            },
            super::CommittedControlEvent::Weather {
                sequence: 3,
                update: WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 1.0,
                },
            },
            super::CommittedControlEvent::Weather {
                sequence: 4,
                update: WeatherUpdateEvent {
                    channel: WeatherChannel::Lightning,
                    level: 0.0,
                },
            },
        ]
    );
    // The same packet's HUD rules commit to the UI queue.
    assert!(stream.take_committed_ui().iter().any(|event| matches!(
        event,
        super::CommittedUiEvent::Ui {
            sequence: 2,
            event: protocol::UiEvent::GameRules { .. },
        }
    )));
    assert_eq!(stream.mesh_jobs.pending.len(), pending_mesh_count);
    assert_eq!(stream.mesh_changes.len(), mesh_change_count);
    assert_eq!(stream.connectivity_generation, connectivity_generation);
}

#[test]
fn clock_and_weather_commit_in_order_past_an_earlier_chunk_decode() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: protocol_ids::SEQUENTIAL_AIR_NETWORK_ID,
        block_network_ids_are_hashes: false,
    });
    stream.submit(1, inline_air_event(0)).unwrap();
    stream
        .submit(2, WorldEvent::SetTime(SetTimeEvent { time: -1 }))
        .unwrap();
    stream
        .submit(
            3,
            WorldEvent::Weather(WeatherUpdateEvent {
                channel: WeatherChannel::Rain,
                level: 0.25,
            }),
        )
        .unwrap();

    assert_eq!(
        stream.committed_sequence(),
        0,
        "the chunk is still decoding"
    );
    assert_eq!(
        stream.take_committed_controls(),
        vec![
            super::CommittedControlEvent::SetTime {
                sequence: 2,
                update: SetTimeEvent { time: -1 },
            },
            super::CommittedControlEvent::Weather {
                sequence: 3,
                update: WeatherUpdateEvent {
                    channel: WeatherChannel::Rain,
                    level: 0.25,
                },
            },
        ]
    );
}
