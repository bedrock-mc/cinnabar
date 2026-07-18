use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, PlayerListEntry, PlayerListUpdateEvent, PlayerSkin,
    PlayerSkinUnavailable, StandardSkin,
};

use super::{ActorApplyResult, ActorStore, spawn};

#[test]
fn render_players_join_roster_skins_and_sort_by_runtime_id() {
    let skin = PlayerSkin::Standard(StandardSkin {
        width: 64,
        height: 64,
        rgba8: vec![9; 64 * 64 * 4].into(),
    });
    let mut store = ActorStore::new(1, 0);
    for (sequence, runtime_id, unique_id, uuid) in [(1, 20, 2, [2; 16]), (2, 10, 1, [1; 16])] {
        let mut event = spawn(runtime_id, unique_id);
        let ActorEvent::Spawn(spawn) = &mut event else {
            unreachable!()
        };
        spawn.kind = ActorKind::Player {
            uuid,
            username: format!("player-{runtime_id}").into(),
        };
        store.apply(1, sequence, event);
    }
    store.apply(
        1,
        3,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid: [1; 16],
                unique_id: 1,
                username: "player-10".into(),
                verified: true,
                skin: skin.clone(),
            }]),
        }),
    );

    let players = store.render_players(None);
    assert_eq!(
        players
            .iter()
            .map(|(actor, _)| actor.runtime_id)
            .collect::<Vec<_>>(),
        [10, 20]
    );
    assert_eq!(players[0].1.map(|profile| &profile.skin), Some(&skin));
    assert!(players[1].1.is_none());

    let remote_players = store.render_players(Some(10));
    assert_eq!(remote_players.len(), 1);
    assert_eq!(remote_players[0].0.runtime_id, 20);
}

#[test]
fn incremental_player_lists_cannot_exceed_the_store_skin_byte_budget() {
    let skin_bytes = 64 * 64 * 4;
    let skin = |value| {
        PlayerSkin::Standard(StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![value; skin_bytes].into(),
        })
    };
    let add = |uuid, unique_id, skin| {
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid,
                unique_id,
                username: format!("player-{unique_id}").into(),
                verified: true,
                skin,
            }]),
        })
    };
    let mut store = ActorStore::with_limits(1, 0, 2, 2, skin_bytes);

    assert_eq!(
        store.apply(1, 1, add([1; 16], 1, skin(1))),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.apply(1, 2, add([2; 16], 2, skin(2))),
        ActorApplyResult::Updated
    );
    assert_eq!(store.retained_player_skin_bytes, skin_bytes);
    assert_eq!(
        store.players[&[2; 16]].skin,
        PlayerSkin::Unavailable(PlayerSkinUnavailable::RetainedBudgetExceeded)
    );

    let oversized_replacement = PlayerSkin::Standard(StandardSkin {
        width: 128,
        height: 128,
        rgba8: vec![9; 128 * 128 * 4].into(),
    });
    store.apply(1, 3, add([1; 16], 10, oversized_replacement));
    assert_eq!(store.retained_player_skin_bytes, skin_bytes);
    assert_eq!(store.players[&[1; 16]].unique_id, 10);
    assert_eq!(store.players[&[1; 16]].skin, skin(1));

    store.apply(
        1,
        4,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Remove { uuid: [1; 16] }]),
        }),
    );
    assert_eq!(store.retained_player_skin_bytes, 0);
    store.apply(1, 5, add([2; 16], 2, skin(3)));
    assert_eq!(store.retained_player_skin_bytes, skin_bytes);
    assert!(matches!(
        store.players[&[2; 16]].skin,
        PlayerSkin::Standard(_)
    ));
}
