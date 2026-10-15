use super::*;
use std::sync::Arc;

/// Roster badges depend on current UUID and unique ID, never appearance readiness or names.
#[test]
fn roster_xuid_tracks_actor_identity_and_removal() {
    let mut store = ActorStore::new(1, 0);
    let ActorEvent::Spawn(mut player) = super::tests::spawn(7, 70) else {
        unreachable!()
    };
    player.kind = ActorKind::Player {
        uuid: [7; 16],
        username: "ExamplePlayer".into(),
    };
    store.apply(1, 1, ActorEvent::Spawn(player));
    assert_eq!(store.player_xuid(7), None);
    store.apply(
        1,
        2,
        ActorEvent::PlayerList(protocol::PlayerListUpdateEvent {
            entries: Arc::from([protocol::PlayerListEntry::Add {
                uuid: [7; 16],
                unique_id: 70,
                username: "DifferentName".into(),
                verified: true,
                xuid: "11".into(),
                skin: protocol::PlayerSkin::Unavailable(
                    protocol::PlayerSkinUnavailable::InvalidDimensions,
                ),
            }]),
        }),
    );
    assert_eq!(store.player_xuid(7), Some("11"));
    store.actors.get_mut(&7).unwrap().unique_id = 71;
    assert_eq!(store.player_xuid(7), None);
    store.actors.get_mut(&7).unwrap().unique_id = 70;
    store.apply(
        1,
        3,
        ActorEvent::PlayerList(protocol::PlayerListUpdateEvent {
            entries: Arc::from([protocol::PlayerListEntry::Remove { uuid: [7; 16] }]),
        }),
    );
    assert_eq!(store.player_xuid(7), None);
}
