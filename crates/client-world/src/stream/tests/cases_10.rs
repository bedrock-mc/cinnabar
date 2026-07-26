use super::*;

#[test]
fn ui_and_block_crack_events_publish_fifo_with_committed_dimension() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 2,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    let ui = UiEvent::Hud(HudEvent::Health { health: 17 });
    let crack = BlockCrackEvent {
        position: [-3, 72, 9],
        action: BlockCrackAction::UpdateSpeed {
            progress_per_tick: 2_048,
        },
    };

    stream.submit(1, WorldEvent::Ui(ui.clone())).unwrap();
    stream.submit(2, WorldEvent::BlockCrack(crack)).unwrap();

    assert_eq!(
        stream.take_committed_ui(),
        vec![
            CommittedUiEvent::Ui {
                sequence: 1,
                event: ui,
            },
            CommittedUiEvent::BlockCrack {
                sequence: 2,
                dimension: 2,
                event: crack,
            },
        ]
    );
    assert!(stream.take_committed_ui().is_empty());
}
