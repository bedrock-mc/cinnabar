use std::sync::Arc;

use protocol::{
    ActorEvent, ActorKind, PlayerListEntry, PlayerListUpdateEvent, PlayerSkin, StandardSkin,
};

use super::{ActorApplyResult, ActorStore, LocalPlayerFeed, tests::player_move};

fn standard_skin(byte: u8) -> PlayerSkin {
    PlayerSkin::Standard(StandardSkin {
        geometry: None,
        cape: None,
        width: 64,
        height: 64,
        rgba8: vec![byte; 64 * 64 * 4].into(),
    })
}

fn fed_skin() -> PlayerSkin {
    standard_skin(9)
}

fn list_add(uuid: [u8; 16], unique_id: i64, skin: PlayerSkin) -> ActorEvent {
    ActorEvent::PlayerList(PlayerListUpdateEvent {
        entries: Arc::from([PlayerListEntry::Add {
            uuid,
            unique_id,
            username: "local".into(),
            verified: true,
            skin,
        }]),
    })
}

fn profile_skin(store: &ActorStore, runtime_id: u64) -> Option<PlayerSkin> {
    store.player_profile(runtime_id).map(|p| p.skin.clone())
}

fn local_feed(x: f32, yaw: f32) -> LocalPlayerFeed {
    LocalPlayerFeed {
        uuid: [5; 16],
        username: "local".into(),
        skin: fed_skin(),
        position: [x, 64.0, 0.0],
        velocity: [0.0; 3],
        on_ground: true,
        yaw,
        head_yaw: yaw,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        teleported: false,
        first_person: false,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    }
}

#[test]
fn local_feed_overrides_only_the_predicted_sneak_and_sprint_flags() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    let mut feed = local_feed(0.0, 0.0);
    feed.sneaking = true;
    store.sync_local_player(1, -100, &feed);
    assert!(store.get(1).unwrap().flag(1) && !store.get(1).unwrap().flag(3));
    feed.sneaking = false;
    feed.sprinting = true;
    store.sync_local_player(1, -100, &feed);
    let actor = store.get(1).unwrap();
    assert!(!actor.flag(1) && actor.flag(3));
}

#[test]
fn local_player_sync_spawns_a_client_owned_player_actor_then_updates_it() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.sync_local_player(1, -100, &local_feed(2.0, 30.0));
    let actor = store.get(1).expect("local actor spawned");
    assert!(matches!(actor.kind, ActorKind::Player { .. }));
    assert_eq!(actor.unique_id, -100);
    assert_eq!(actor.received_pose.position, [2.0, 64.0, 0.0]);
    assert!(actor.spawn_revision != 0 && actor.movement_revision != 0);
    // Items stay client-owned: no equipment slot is created for the excluded runtime id.
    assert!(store.equipment(1).is_none());
    let first_revision = actor.movement_revision;

    let mut feed = local_feed(9.0, 30.0);
    feed.velocity = [1.5, 0.0, 0.0];
    feed.on_ground = false;
    store.sync_local_player(1, -100, &feed);
    let actor = store.get(1).expect("local actor retained");
    assert_eq!(actor.received_pose.position, [9.0, 64.0, 0.0]);
    assert_eq!(actor.velocity, [1.5, 0.0, 0.0]);
    assert_eq!(actor.native_velocity(), [1.5, 0.0, 0.0]);
    assert_eq!(actor.on_ground, Some(false));
    assert!(actor.movement_revision > first_revision);
}

#[test]
fn server_move_never_overrides_the_client_fed_local_pose() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.sync_local_player(1, -100, &local_feed(5.0, 0.0));
    assert_eq!(
        store.apply(1, 1, player_move(1, 999.0, false)),
        ActorApplyResult::MissingActor
    );
    assert_eq!(store.get(1).unwrap().received_pose.position[0], 5.0);
}

#[test]
fn local_player_pose_snaps_to_the_fed_position_each_tick_without_easing() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.sync_local_player(1, -100, &local_feed(5.0, 0.0));
    store.advance_interpolation_ticks(1);
    assert_eq!(store.get(1).unwrap().position, [5.0, 64.0, 0.0]);
    store.sync_local_player(1, -100, &local_feed(6.0, 0.0));
    store.advance_interpolation_ticks(1);
    let actor = store.get(1).unwrap();
    assert_eq!(actor.position, [6.0, 64.0, 0.0]);
    assert_eq!(actor.previous_pose.position, [5.0, 64.0, 0.0]);
}

#[test]
fn local_player_profile_resolves_its_skin_from_the_player_list() {
    let skin = standard_skin(7);
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.apply(1, 1, list_add([5; 16], -100, skin.clone()));
    store.sync_local_player(1, -100, &local_feed(0.0, 0.0));
    assert_eq!(profile_skin(&store, 1), Some(skin));
}

#[test]
fn local_body_resolves_the_fed_skin_without_a_self_list_entry() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.sync_local_player(1, -100, &local_feed(0.0, 0.0));
    assert_eq!(store.player_profile(1).map(|p| p.unique_id), Some(-100));
    assert_eq!(profile_skin(&store, 1), Some(fed_skin()));
    assert_eq!(store.player_count(), 1);
}

#[test]
fn a_real_list_echo_overrides_the_synthetic_local_skin() {
    let echo_skin = standard_skin(3);
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    // The synthetic profile is installed first; a self entry under a different uuid must win.
    store.sync_local_player(1, -100, &local_feed(0.0, 0.0));
    store.apply(1, 1, list_add([8; 16], -100, echo_skin.clone()));
    store.sync_local_player(1, -100, &local_feed(0.0, 0.0));
    assert_eq!(profile_skin(&store, 1), Some(echo_skin));
    // No stale synthetic profile remains at the fed uuid: only the echo.
    assert_eq!(store.player_count(), 1);
}

#[test]
fn dimension_reset_clears_the_synthetic_profile_then_respawns_it() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.sync_local_player(1, -100, &local_feed(0.0, 0.0));
    assert_eq!(store.player_count(), 1);
    assert_eq!(store.reset_dimension(1, 1, 2), ActorApplyResult::Reset);
    assert!(store.get(1).is_none());
    assert_eq!(store.player_count(), 0);
    // The next feed re-installs both the actor and its skin.
    store.sync_local_player(1, -100, &local_feed(0.0, 0.0));
    assert!(store.get(1).is_some());
    assert_eq!(profile_skin(&store, 1), Some(fed_skin()));
}

#[test]
fn predicted_item_use_sets_and_clears_use_flag_but_unpredicted_leaves_it() {
    use super::super::LocalItemUse;
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    let mut feed = local_feed(0.0, 0.0);
    feed.item_use = LocalItemUse::Using;
    store.sync_local_player(1, -100, &feed);
    assert!(store.get(1).unwrap().flag(4));
    feed.item_use = LocalItemUse::Unpredicted;
    store.sync_local_player(1, -100, &feed);
    assert!(store.get(1).unwrap().flag(4));
    feed.item_use = LocalItemUse::Idle;
    store.sync_local_player(1, -100, &feed);
    assert!(!store.get(1).unwrap().flag(4));
}

/// Shield blocking is server metadata; local use prediction must never clear it.
#[test]
fn predicted_item_use_leaves_server_blocking_flag() {
    use super::super::LocalItemUse;
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    let mut feed = local_feed(0.0, 0.0);
    store.sync_local_player(1, -100, &feed);
    // Local movement exclusion must not drop the authoritative metadata word. Shield's
    // vanilla render query reads this word; it does not reconstruct blocking from sneak.
    assert_eq!(
        store.apply(
            1,
            1,
            ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
                dimension: 0,
                runtime_id: 1,
                metadata: Arc::from([protocol::ActorMetadata {
                    key: super::EXTENDED_FLAGS_METADATA_KEY,
                    value: protocol::ActorMetadataValue::FlagsExtended(1 << (72 - 64)),
                }]),
                properties: Arc::from([]),
                tick: 0,
            })
        ),
        ActorApplyResult::Updated
    );
    feed.item_use = LocalItemUse::Idle;
    store.sync_local_player(1, -100, &feed);
    assert!(store.get(1).unwrap().flag(72));
    feed.item_use = LocalItemUse::Using;
    store.sync_local_player(1, -100, &feed);
    assert!(store.get(1).unwrap().flag(72));
}

#[test]
fn synthetic_profile_skin_updates_and_removal_preserve_the_budget() {
    let mut store = ActorStore::new(1, 0);
    let mut feed = local_feed(0.0, 0.0);
    store.sync_local_player(1, -100, &feed);
    assert_eq!(
        store.apply(
            1,
            1,
            ActorEvent::Skin {
                uuid: feed.uuid,
                skin: standard_skin(2)
            }
        ),
        ActorApplyResult::Updated
    );
    assert_eq!(
        store.retained_player_skin_bytes,
        super::retained_skin_bytes(&feed.skin)
    );
    feed.skin = standard_skin(4);
    store.sync_local_player(1, -100, &feed);
    assert_eq!(
        store.retained_player_skin_bytes,
        super::retained_skin_bytes(&feed.skin)
    );
    store.reset_dimension(1, 2, 2);
    assert_eq!(store.retained_player_skin_bytes, 0);
}

#[test]
fn synthetic_profile_obeys_the_skin_budget() {
    let mut store = ActorStore::with_limits(1, 0, 4, 4, 0);
    store.sync_local_player(1, -100, &local_feed(0.0, 0.0));
    assert_eq!(store.retained_player_skin_bytes, 0);
    assert!(matches!(
        profile_skin(&store, 1),
        Some(PlayerSkin::Unavailable(_))
    ));
}

#[test]
fn review_authoritative_echo_under_the_fed_uuid_retains_its_skin() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    let feed = local_feed(0.0, 0.0);
    store.sync_local_player(1, -100, &feed);
    let skin = standard_skin(3);
    store.apply(1, 1, list_add(feed.uuid, -100, skin.clone()));
    store.sync_local_player(1, -100, &feed);
    assert_eq!(profile_skin(&store, 1), Some(skin));
    assert!(store.player_profile(1).unwrap().verified);
}
