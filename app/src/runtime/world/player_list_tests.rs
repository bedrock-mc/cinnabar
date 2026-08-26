use std::sync::Arc;

use client_world::{CommittedControlEvent, WorldStream};
use protocol::{
    ActorEvent, PlayerListEntry, PlayerListUpdateEvent, PlayerSkin, PlayerSkinUnavailable,
    WorldBootstrap, WorldEvent,
};

use super::refresh_player_list_cache_for_controls;
use crate::ui_runtime::UiRuntime;

#[test]
fn player_list_marker_refreshes_tab_rows_without_a_ui_event() {
    let mut stream = WorldStream::new(WorldBootstrap {
        local_player_unique_id: 1,
        dimension: 0,
        local_player_runtime_id: 1,
        player_position: [0.0; 3],
        world_spawn_position: [0; 3],
        air_network_id: 12_530,
        block_network_ids_are_hashes: false,
    });
    stream
        .submit(
            1,
            WorldEvent::Actor(ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: Arc::from([PlayerListEntry::Add {
                    uuid: [7; 16],
                    unique_id: 7,
                    username: Arc::from("Alex"),
                    verified: true,
                    skin: PlayerSkin::Unavailable(PlayerSkinUnavailable::UnsupportedPersona),
                }]),
            })),
        )
        .unwrap();
    let controls = stream.take_committed_controls();
    assert_eq!(
        controls,
        vec![CommittedControlEvent::PlayerListChanged { sequence: 1 }]
    );
    assert!(stream.take_committed_ui().is_empty());

    let mut ui = UiRuntime::new(1);
    refresh_player_list_cache_for_controls(&stream, &mut ui, &controls);
    assert_eq!(
        ui.player_list_overlay_rows(),
        vec![(Arc::from("Alex"), None)]
    );
}
