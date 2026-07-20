use super::*;

#[test]
fn roster_unique_id_lookup_is_exact_and_fails_closed_on_ambiguity() {
    let skin = |value| {
        PlayerSkin::Standard(StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![value; 64 * 64 * 4].into(),
        })
    };
    let add = |uuid, unique_id, value| {
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Add {
                uuid,
                unique_id,
                username: format!("player-{value}").into(),
                verified: true,
                skin: skin(value),
            }]),
        })
    };
    let mut store = ActorStore::new(1, 0);

    store.apply(1, 1, add([1; 16], -9, 1));
    assert_eq!(
        store
            .player_profile_by_unique_id(-9)
            .map(|profile| &profile.skin),
        Some(&skin(1))
    );
    assert!(store.player_profile_by_unique_id(77).is_none());

    store.apply(1, 2, add([2; 16], -9, 2));
    assert!(
        store.player_profile_by_unique_id(-9).is_none(),
        "duplicate unique IDs must not select an arbitrary UUID"
    );

    store.apply(
        1,
        3,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Remove { uuid: [2; 16] }]),
        }),
    );
    assert_eq!(
        store
            .player_profile_by_unique_id(-9)
            .map(|profile| &profile.skin),
        Some(&skin(1)),
        "removing the conflicting roster entry restores exact authority"
    );
}
