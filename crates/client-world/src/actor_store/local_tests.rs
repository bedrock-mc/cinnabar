use std::sync::Arc;

use protocol::{ActorEvent, ActorKind, PlayerListEntry, PlayerListUpdateEvent, PlayerSkin};
use render_api::StandardSkin;

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
        game_mode: None,
        prefer_client_skin: false,
        uuid: [5; 16],
        username: "local".into(),
        skin: fed_skin(),
        position: [x, 64.0, 0.0],
        velocity: [0.0; 3],
        on_ground: true,
        flying: false,
        gliding: false,
        fall_fly_ticks: 0,
        yaw,
        head_yaw: yaw,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        main_hand_metadata: 0,
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: crate::ACTOR_SWING_TICKS,
        java_swing_ticks: crate::ACTOR_SWING_TICKS,
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
fn local_feed_game_mode_refreshes_the_rig_on_a_frame_without_a_fixed_tick() {
    let mut store = ActorStore::new(1, 0);
    let mut feed = local_feed(0.0, 0.0);
    feed.game_mode = Some(protocol::PlayerGameMode::Survival);
    store.sync_local_player(1, -100, &feed);
    store.local_view_dirty = false;
    feed.game_mode = Some(protocol::PlayerGameMode::Spectator);
    store.sync_local_player(1, -100, &feed);
    assert!(store.local_view_dirty);
    assert_eq!(
        store.get(1).unwrap().player_game_mode,
        Some(protocol::GameModeUpdate::Explicit(
            protocol::PlayerGameMode::Spectator
        ))
    );
    store.local_view_dirty = false;
    store.sync_local_player(1, -100, &feed);
    assert!(!store.local_view_dirty);
}

#[test]
fn local_flight_fact_clears_when_the_actor_session_or_dimension_is_reset() {
    let mut store = ActorStore::new(1, 0);
    let mut feed = local_feed(0.0, 0.0);
    feed.flying = true;
    store.sync_local_player(1, -1, &feed);
    assert!(store.local_flying);
    store.begin_session(2, 0);
    assert!(!store.local_flying);
    store.sync_local_player(1, -1, &feed);
    assert!(store.local_flying);
    assert_eq!(store.reset_dimension(2, 1, 1), ActorApplyResult::Reset);
    assert!(!store.local_flying);
}

#[test]
fn health_animation_requires_player_spawn_each_session_and_survives_dimension_recreation() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
    store.set_local_health(1, 0);
    assert!(store.get(1).unwrap().status.dead);
    assert_eq!(store.get(1).unwrap().status.hurt_time, 0);
    assert!(!store.get(1).unwrap().status.damage.flash_active());
    store.set_local_health(1, 20);
    store.mark_local_player_spawned(1);
    store.set_local_health(1, 16);
    assert_eq!(
        store.get(1).unwrap().status.hurt_time,
        crate::HURT_DURATION_TICKS
    );
    store.reset_dimension(1, 1, 1);
    store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
    store.set_local_health(1, 7);
    assert_eq!(
        store.get(1).unwrap().status.hurt_time,
        crate::HURT_DURATION_TICKS
    );
    store.begin_session(2, 1);
    store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
    store.set_local_health(1, 0);
    assert_eq!(store.get(1).unwrap().status.hurt_time, 0);
    assert!(!store.get(1).unwrap().status.damage.flash_active());
}

#[test]
fn health_drop_after_player_spawn_survives_the_first_local_pose() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.mark_local_player_spawned(1);
    store.set_local_health(1, 7);
    store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
    let status = store.get(1).unwrap().status;
    assert_eq!(status.hurt_time, crate::HURT_DURATION_TICKS);
    assert_eq!(status.damage.previous_health, crate::DEFAULT_PLAYER_HEALTH);
    assert!(status.damage.flash_active());
}

#[test]
fn local_health_pending_a_pose_cannot_cross_sessions() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.set_local_health(1, 0);
    store.begin_session(2, 0);
    store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
    let actor = store.get(1).unwrap();
    assert!(!actor.status.dead);
    assert!(!actor.attributes.contains_key("minecraft:health"));
}

#[test]
fn local_health_pending_a_pose_cannot_override_newer_spawn_health() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.set_local_health(1, 0);
    let ActorEvent::Spawn(mut spawn) = super::tests::spawn(1, -1) else {
        unreachable!();
    };
    spawn.attributes = Arc::from([protocol::ActorAttribute {
        name: "minecraft:health".into(),
        min: 0.0,
        max: crate::DEFAULT_PLAYER_HEALTH,
        current: 7.0,
        default: None,
        modifiers: Default::default(),
    }]);
    store.apply(1, 1, ActorEvent::Spawn(spawn));
    store.apply(
        1,
        2,
        ActorEvent::Remove(protocol::ActorRemoveEvent {
            dimension: 0,
            unique_id: -1,
        }),
    );
    store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
    assert!(!store.get(1).unwrap().status.dead);
}

#[test]
fn spawn_health_initializes_death_time_and_replacement_clears_it() {
    for local in [false, true] {
        let mut store = ActorStore::new(1, 0);
        if local {
            store.exclude_remote_state_for(1);
        }
        let ActorEvent::Spawn(mut spawn) = super::tests::spawn(1, -1) else {
            unreachable!();
        };
        spawn.attributes = Arc::from([protocol::ActorAttribute {
            name: "minecraft:health".into(),
            min: 0.0,
            max: crate::DEFAULT_PLAYER_HEALTH,
            current: 0.0,
            default: None,
            modifiers: Default::default(),
        }]);
        assert_eq!(
            store.apply(1, 1, ActorEvent::Spawn(spawn.clone())),
            ActorApplyResult::Inserted
        );
        assert!(store.get(1).unwrap().status.dead);
        store.advance_interpolation_ticks(125);
        assert_eq!(store.get(1).unwrap().status.native_death_ticks(), 125);
        Arc::make_mut(&mut spawn.attributes)[0].current = 7.0;
        assert_eq!(
            store.apply(1, 2, ActorEvent::Spawn(spawn)),
            ActorApplyResult::Replaced
        );
        assert!(!store.get(1).unwrap().status.dead);
        assert_eq!(store.get(1).unwrap().status.native_death_ticks(), 0);
    }
}

#[test]
fn local_health_respects_retained_attribute_capacity_and_invalid_ranges() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
    let actor = store.actors.get_mut(&1).unwrap();
    for index in 0..protocol::MAX_ACTOR_ATTRIBUTES {
        let name = format!("attribute{index}");
        actor.attributes.insert(
            name.clone().into(),
            protocol::ActorAttribute {
                name: name.into(),
                min: 0.0,
                max: 1.0,
                current: 0.0,
                default: None,
                modifiers: Default::default(),
            },
        );
    }
    store.set_local_health(1, 0);
    assert_eq!(
        store.get(1).unwrap().attributes.len(),
        protocol::MAX_ACTOR_ATTRIBUTES
    );
    assert!(!store.get(1).unwrap().status.dead);
    assert_eq!(store.local_health_skips(), 1);
    store.actors.get_mut(&1).unwrap().attributes.clear();
    store.set_local_health(1, 7);
    store
        .actors
        .get_mut(&1)
        .unwrap()
        .attributes
        .get_mut("minecraft:health")
        .unwrap()
        .max = f32::NAN;
    store.set_local_health(1, 0);
    assert!(!store.get(1).unwrap().status.dead);
    assert_eq!(store.local_health_skips(), 2);
}

#[test]
fn local_health_skips_values_rejected_by_the_hud_before_mutating_actor_or_pending_state() {
    for has_pose in [false, true] {
        let mut store = ActorStore::new(1, 0);
        store.exclude_remote_state_for(1);
        if has_pose {
            store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
        }
        store.set_local_health(1, 7);
        for value in [-5, i32::MIN, i32::from(u16::MAX) + 1, i32::MAX] {
            store.set_local_health(1, value);
        }
        store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
        let actor = store.get(1).unwrap();
        assert!(!actor.status.dead);
        assert_eq!(actor.attributes["minecraft:health"].current, 7.0);
        assert_eq!(store.local_health_skips(), 4);
    }
}

#[test]
fn local_health_survives_dimension_actor_recreation() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
    store.set_local_health(1, 7);
    assert_eq!(store.reset_dimension(1, 1, 2), ActorApplyResult::Reset);
    store.sync_local_player(1, -1, &local_feed(0.0, 0.0));
    assert_eq!(
        store.get(1).unwrap().attributes["minecraft:health"].current,
        7.0
    );
    store.set_local_health(1, 0);
    assert!(store.get(1).unwrap().status.dead);
    store.set_local_health(1, i32::from(u16::MAX));
    assert_eq!(
        store.get(1).unwrap().attributes["minecraft:health"].current,
        crate::DEFAULT_PLAYER_HEALTH
    );
    assert!(!store.get(1).unwrap().status.dead);
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

fn cape_skin(byte: u8) -> PlayerSkin {
    let PlayerSkin::Standard(mut skin) = standard_skin(byte) else {
        unreachable!()
    };
    skin.cape = Some(render_api::CapeImage {
        width: 64,
        height: 32,
        rgba8: vec![byte; 64 * 32 * 4].into(),
    });
    PlayerSkin::Standard(skin)
}

#[test]
fn unchanged_local_feed_preserves_server_skin_and_cape_update() {
    let mut store = ActorStore::new(1, 0);
    let mut feed = local_feed(0.0, 0.0);
    store.sync_local_player(1, -100, &feed);
    let server_skin = cape_skin(2);
    assert_eq!(
        store.apply(
            1,
            1,
            ActorEvent::Skin {
                uuid: feed.uuid,
                skin: server_skin.clone(),
            }
        ),
        ActorApplyResult::Updated
    );
    let retained = store.retained_player_skin_bytes;
    feed.position[0] = 4.0;
    feed.yaw = 90.0;
    for _ in 0..3 {
        store.sync_local_player(1, -100, &feed);
        assert_eq!(profile_skin(&store, 1), Some(server_skin.clone()));
        assert_eq!(store.retained_player_skin_bytes, retained);
    }
    feed.skin = standard_skin(4);
    store.sync_local_player(1, -100, &feed);
    assert_eq!(profile_skin(&store, 1), Some(feed.skin.clone()));
    assert_eq!(
        store.retained_player_skin_bytes,
        super::retained_skin_bytes(&feed.skin)
    );
}

#[test]
fn local_feed_preserves_unlisted_authoritative_skin_and_cape() {
    for uuid in [[5; 16], [7; 16]] {
        let mut store = ActorStore::new(1, 0);
        let feed = local_feed(0.0, 0.0);
        store.sync_local_player(1, -100, &feed);
        let server_skin = cape_skin(3);
        store.apply(1, 1, list_add(uuid, -100, server_skin.clone()));
        store.sync_local_player(1, -100, &feed);
        let retained = store.retained_player_skin_bytes;
        store.apply(
            1,
            2,
            ActorEvent::PlayerList(PlayerListUpdateEvent {
                entries: Arc::from([PlayerListEntry::Remove { uuid }]),
            }),
        );
        for _ in 0..3 {
            store.sync_local_player(1, -100, &feed);
            assert_eq!(profile_skin(&store, 1), Some(server_skin.clone()));
            assert_eq!(store.retained_player_skin_bytes, retained);
            assert_eq!(store.player_count(), 0);
            assert!(store.unlisted_players.contains_key(&uuid));
        }
        store.reset_dimension(1, 3, 2);
        assert_eq!(store.retained_player_skin_bytes, 0);
        assert!(store.unlisted_players.is_empty());
    }
}

#[test]
fn authoritative_echo_releases_unlisted_synthetic_appearance_after_adoption() {
    let mut store = ActorStore::new(1, 0);
    let feed = local_feed(0.0, 0.0);
    store.sync_local_player(1, -100, &feed);
    store.apply(
        1,
        1,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Remove { uuid: feed.uuid }]),
        }),
    );
    store.sync_local_player(1, -100, &feed);
    assert!(store.unlisted_players.contains_key(&feed.uuid));
    assert_eq!(profile_skin(&store, 1), Some(feed.skin.clone()));

    let server_uuid = [7; 16];
    let server_skin = cape_skin(6);
    store.apply(1, 2, list_add(server_uuid, -100, server_skin.clone()));
    store.sync_local_player(1, -100, &feed);
    assert_eq!(profile_skin(&store, 1), Some(server_skin.clone()));
    assert!(matches!(&store.get(1).unwrap().kind,
        ActorKind::Player { uuid, .. } if *uuid == server_uuid));
    assert!(!store.unlisted_players.contains_key(&feed.uuid));
    assert_eq!(store.player_count(), 1);
    assert_eq!(
        store.retained_player_skin_bytes,
        super::retained_skin_bytes(&server_skin)
    );
}

#[test]
fn rejected_synthetic_feed_does_not_retain_uncharged_raster() {
    let mut store = ActorStore::with_limits(1, 0, 4, 4, 0);
    let feed = local_feed(0.0, 0.0);
    let PlayerSkin::Standard(skin) = &feed.skin else {
        unreachable!()
    };
    let pixels = Arc::downgrade(skin.rgba8.pixels());
    store.sync_local_player(1, -100, &feed);
    drop(feed);
    assert_eq!(store.retained_player_skin_bytes, 0);
    assert!(pixels.upgrade().is_none());
}

#[test]
fn server_replacement_releases_previous_client_fed_raster() {
    let mut store = ActorStore::new(1, 0);
    let feed = local_feed(0.0, 0.0);
    let PlayerSkin::Standard(skin) = &feed.skin else {
        unreachable!()
    };
    let pixels = Arc::downgrade(skin.rgba8.pixels());
    store.sync_local_player(1, -100, &feed);
    let server_skin = cape_skin(2);
    store.apply(
        1,
        1,
        ActorEvent::Skin {
            uuid: feed.uuid,
            skin: server_skin.clone(),
        },
    );
    drop(feed);
    assert!(pixels.upgrade().is_none());
    assert_eq!(
        store.retained_player_skin_bytes,
        super::retained_skin_bytes(&server_skin)
    );
}

#[test]
fn rejected_local_skin_change_retries_when_skin_budget_is_freed() {
    let base_bytes = super::retained_skin_bytes(&fed_skin());
    let mut store = ActorStore::with_limits(1, 0, 4, 4, base_bytes * 2);
    let mut feed = local_feed(0.0, 0.0);
    store.sync_local_player(1, -100, &feed);
    let other_uuid = [7; 16];
    store.apply(1, 1, list_add(other_uuid, -200, standard_skin(3)));
    feed.skin = cape_skin(4);
    store.sync_local_player(1, -100, &feed);
    assert_eq!(profile_skin(&store, 1), Some(fed_skin()));
    assert_eq!(store.retained_player_skin_bytes, base_bytes * 2);
    store.apply(
        1,
        2,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Remove { uuid: other_uuid }]),
        }),
    );
    store.sync_local_player(1, -100, &feed);
    assert_eq!(profile_skin(&store, 1), Some(feed.skin.clone()));
    assert_eq!(
        store.retained_player_skin_bytes,
        super::retained_skin_bytes(&feed.skin)
    );
}

#[test]
fn server_skin_update_supersedes_a_pending_rejected_local_skin() {
    let base_bytes = super::retained_skin_bytes(&fed_skin());
    let mut store = ActorStore::with_limits(1, 0, 4, 4, base_bytes * 2);
    let mut feed = local_feed(0.0, 0.0);
    store.sync_local_player(1, -100, &feed);
    let other_uuid = [7; 16];
    store.apply(1, 1, list_add(other_uuid, -200, standard_skin(3)));
    feed.skin = cape_skin(4);
    store.sync_local_player(1, -100, &feed);
    assert_eq!(profile_skin(&store, 1), Some(fed_skin()));
    let server_skin = standard_skin(2);
    store.apply(
        1,
        2,
        ActorEvent::Skin {
            uuid: feed.uuid,
            skin: server_skin.clone(),
        },
    );
    store.apply(
        1,
        3,
        ActorEvent::PlayerList(PlayerListUpdateEvent {
            entries: Arc::from([PlayerListEntry::Remove { uuid: other_uuid }]),
        }),
    );
    store.sync_local_player(1, -100, &feed);
    assert_eq!(profile_skin(&store, 1), Some(server_skin));
    assert_eq!(store.retained_player_skin_bytes, base_bytes);
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

#[test]
fn client_skin_override_restores_the_retained_server_appearance() {
    for server_uuid in [[5; 16], [7; 16]] {
        for unlist_at in [None, Some(0), Some(1)] {
            let listed = unlist_at.is_none();
            let mut store = ActorStore::new(1, 0);
            let mut feed = local_feed(0.0, 0.0);
            let server_skin = cape_skin(3);
            store.exclude_remote_state_for(1);
            store.apply(1, 1, list_add(server_uuid, -100, server_skin.clone()));
            store.sync_local_player(1, -100, &feed);
            feed.skin = cape_skin(7);
            feed.prefer_client_skin = true;
            for step in 0..3 {
                if unlist_at == Some(step) {
                    store.apply(
                        1,
                        2,
                        ActorEvent::PlayerList(PlayerListUpdateEvent {
                            entries: Arc::from([PlayerListEntry::Remove { uuid: server_uuid }]),
                        }),
                    );
                }
                store.sync_local_player(1, -100, &feed);
                store.prune_unlisted_players();
                assert!(profile_skin(&store, 1).as_ref() == Some(&feed.skin));
                let retained = store
                    .players
                    .get(&server_uuid)
                    .or_else(|| store.unlisted_players.get(&server_uuid))
                    .unwrap();
                assert!(retained.skin == server_skin);
                assert!(retained.verified);
            }
            let synthetic = store.synthetic_local_uuid.unwrap();
            assert_ne!(synthetic, server_uuid);
            assert_eq!(store.player_count(), usize::from(listed) + 1);
            feed.prefer_client_skin = false;
            store.sync_local_player(1, -100, &feed);
            assert!(profile_skin(&store, 1).as_ref() == Some(&server_skin));
            assert!(store.player_profile(1).unwrap().verified);
            assert!(!store.players.contains_key(&synthetic));
            assert!(!store.unlisted_players.contains_key(&synthetic));
            assert_eq!(store.player_count(), usize::from(listed));
            assert_eq!(
                store.retained_player_skin_bytes,
                super::retained_skin_bytes(&server_skin)
            );
        }
    }
}

/// The predicted glide owns the local gliding flag and the ticks that ease in its body tilt.
#[test]
fn local_glide_prediction_sets_the_gliding_flag_and_its_ticks() {
    let mut store = ActorStore::new(1, 0);
    store.exclude_remote_state_for(1);
    let mut feed = local_feed(0.0, 0.0);
    feed.gliding = true;
    feed.fall_fly_ticks = 1;
    store.sync_local_player(1, -100, &feed);
    let actor = store.get(1).unwrap();
    assert!(actor.is_gliding());
    assert_eq!(actor.status.fall_fly_ticks, 1);
    feed.fall_fly_ticks = 7;
    store.sync_local_player(1, -100, &feed);
    assert_eq!(store.get(1).unwrap().status.fall_fly_ticks, 7);
    feed.gliding = false;
    feed.fall_fly_ticks = 0;
    store.sync_local_player(1, -100, &feed);
    let actor = store.get(1).unwrap();
    assert!(!actor.is_gliding());
    assert_eq!(actor.status.fall_fly_ticks, 0);
}
