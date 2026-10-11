//! Actor facts that animation reads, derived from packets and local feeds.
use std::{collections::HashMap, sync::Arc};

use protocol::{ActorKind, ActorMetadataValue};

use super::super::ActorStore;
use super::actor_with_metadata;
use actor_animation::{AnimatedActor, test_support};

#[test]
fn sleeping_player_metadata_does_not_spoof_sneaking() {
    let actor = actor_with_metadata(HashMap::from([(26, ActorMetadataValue::Byte(2))]));
    assert!(AnimatedActor::is_sleeping(&actor));
    assert!(!actor.is_sneaking());
}

#[test]
fn riding_flag_is_not_read_as_sleeping() {
    let actor = actor_with_metadata(HashMap::from([(0, ActorMetadataValue::Flags(1 << 2))]));
    assert!(!AnimatedActor::is_sleeping(&actor));
    let sleeping = actor_with_metadata(HashMap::from([(
        92,
        ActorMetadataValue::FlagsExtended(1 << (76 - 64)),
    )]));
    assert!(AnimatedActor::is_sleeping(&sleeping));
}

#[test]
fn creeper_ignition_advances_and_reverses_swelling_without_swell_metadata() {
    let mut store = ActorStore::new(1, 0);
    store.apply(
        1,
        1,
        protocol::ActorEvent::Spawn(protocol::ActorSpawnEvent {
            dimension: 0,
            unique_id: 1,
            runtime_id: 1,
            kind: ActorKind::Entity {
                identifier: "minecraft:creeper".into(),
            },
            position: [0.0; 3],
            velocity: [0.0; 3],
            pitch: 0.0,
            yaw: 0.0,
            head_yaw: 0.0,
            body_yaw: 0.0,
            held_item: Default::default(),
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags(1 << 10),
            }]),
            attributes: Arc::from([]),
            properties: Arc::from([]),
            links: Arc::from([]),
        }),
    );
    let swell = |store: &ActorStore, alpha: f32| {
        AnimatedActor::creeper_swell_amount(store.get(1).unwrap(), alpha)
    };
    let direction =
        |store: &ActorStore| AnimatedActor::creeper_swelling_direction(store.get(1).unwrap());
    store.advance_interpolation_ticks(14);
    assert_eq!(swell(&store, 0.0), 13.0 / 28.0);
    assert_eq!(direction(&store), 1.0);
    assert_eq!(swell(&store, 0.5), 13.5 / 28.0);
    store.apply(
        1,
        2,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            metadata: Arc::from([protocol::ActorMetadata {
                key: 0,
                value: ActorMetadataValue::Flags(0),
            }]),
            properties: Arc::from([]),
            tick: 14,
        }),
    );
    store.advance_interpolation_ticks(3);
    assert_eq!(swell(&store, 0.0), 12.0 / 28.0);
    assert_eq!(direction(&store), -1.0);
    store.advance_interpolation_ticks(100);
    assert_eq!(swell(&store, 0.0), 0.0);
    store.apply(
        1,
        3,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 1,
            metadata: Arc::from([
                protocol::ActorMetadata {
                    key: 0,
                    value: ActorMetadataValue::Flags(1 << 10),
                },
                protocol::ActorMetadata {
                    key: 19,
                    value: ActorMetadataValue::Int(5_000),
                },
                protocol::ActorMetadata {
                    key: 21,
                    value: ActorMetadataValue::Int(-5_000),
                },
            ]),
            properties: Arc::from([]),
            tick: 117,
        }),
    );
    store.advance_interpolation_ticks(100);
    assert_eq!(swell(&store, 0.0), 30.0 / 28.0);
    store.apply(
        1,
        4,
        protocol::ActorEvent::Status(protocol::ActorStatusEvent {
            runtime_id: 1,
            kind: protocol::ActorStatusKind::Death,
            data: 0,
        }),
    );
    store.advance_interpolation_ticks(2);
    assert_eq!(swell(&store, 0.0), 0.0);
}

#[test]
fn thrown_potion_variant_retains_normalized_add_actor_and_metadata_updates() {
    use protocol::wire::valentine::bedrock::version::v1_26_51::{
        ActorRuntimeId, ActorUniqueId, AddActorPacket, DataItemEntry, DataItemEntryPayload,
        DataItemShortPayload, EnumsDataItemType, SynchedActorDataCopyableDataList,
    };
    // The potion's auxiliary-value key, which its variant query reads.
    const AUX: u32 = actor_animation::test_support::POTION_AUX_VALUE_METADATA_KEY;
    let packet = AddActorPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: -7,
        },
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: 7,
        },
        actor_type: "minecraft:splash_potion".into(),
        actor_data: SynchedActorDataCopyableDataList {
            data: vec![DataItemEntry {
                id: AUX,
                payload: DataItemEntryPayload::DataItemShortPayload(DataItemShortPayload {
                    type_: EnumsDataItemType::Short,
                    value: 21,
                }),
            }],
        },
        ..Default::default()
    };
    let Some(protocol::WorldEvent::Actor(event)) =
        protocol::into_world_event(packet.into(), 0).unwrap()
    else {
        panic!("expected normalized potion spawn");
    };
    let mut actors = ActorStore::new(1, 0);
    actors.apply(1, 1, event);
    let aux = |actors: &ActorStore| actors.get(7).unwrap().metadata.get(&AUX).cloned();
    assert_eq!(aux(&actors), Some(ActorMetadataValue::Short(21)));
    actors.apply(
        1,
        2,
        protocol::ActorEvent::Metadata(protocol::ActorMetadataUpdateEvent {
            dimension: 0,
            runtime_id: 7,
            metadata: Arc::from([protocol::ActorMetadata {
                key: AUX,
                value: ActorMetadataValue::Short(25),
            }]),
            properties: Arc::from([]),
            tick: 0,
        }),
    );
    assert_eq!(aux(&actors), Some(ActorMetadataValue::Short(25)));
}

#[test]
fn local_flight_freezes_cape_walk_phase_but_keeps_chase_and_resumes_walking() {
    let mut compiled = test_support::compiled_fixture();
    compiled.sources[1].path = "entity/player.json".into();
    compiled.symbols[4].kind = assets::EntityAssetKind::Entity;
    compiled.symbols[4].identifier = "minecraft:player".into();
    compiled.symbols.rotate_right(1);
    compiled.rig_bindings[0].entity_symbol = 0;
    compiled.rig_bindings[0].render_controller = 3;
    compiled.animation_clips[0].symbol = 2;
    let assets = Arc::new(assets::RuntimeEntityAssets::from_compiled(compiled).unwrap());
    let mut store = ActorStore::new_with_entity_assets(1, 0, assets);
    store.exclude_remote_state_for(1);
    let mut feed = crate::LocalPlayerFeed {
        game_mode: None,
        prefer_client_skin: false,
        uuid: [1; 16],
        username: "Player".into(),
        skin: protocol::PlayerSkin::Unavailable(protocol::PlayerSkinUnavailable::InvalidDimensions),
        position: [0.0, 64.0, 0.0],
        velocity: [0.2, 0.0, 0.0],
        on_ground: true,
        flying: false,
        gliding: false,
        fall_fly_ticks: 0,
        yaw: 90.0,
        head_yaw: 90.0,
        pitch: 0.0,
        main_hand: None,
        off_hand: None,
        main_hand_metadata: 0,
        main_hand_stack_id: None,
        main_hand_slot: 0,
        bedrock_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        java_swing_ticks: actor_animation::ACTOR_SWING_TICKS,
        teleported: false,
        first_person: false,
        view_bobbing: true,
        sneaking: false,
        sprinting: false,
        item_use: Default::default(),
    };
    let step = |store: &mut ActorStore, feed: &crate::LocalPlayerFeed| {
        store.sync_local_player(1, -1, feed);
        store.advance_interpolation_ticks(1);
        store.actor_rig(1).unwrap().java
    };
    step(&mut store, &feed);
    feed.position[0] = 0.2;
    let walking = step(&mut store, &feed);
    assert!(walking.walked[1] > 0.0);
    feed.flying = true;
    feed.on_ground = false;
    feed.position[0] = 0.4;
    let flying = step(&mut store, &feed);
    assert_eq!(flying.walked, [walking.walked[1]; 2]);
    assert_ne!(
        flying.cape[1], walking.cape[1],
        "flight still advances inertia"
    );
    assert!(flying.limb_swing[1] > walking.limb_swing[1]);
    feed.position[0] = 0.6;
    let flying = step(&mut store, &feed);
    assert_eq!(flying.walked, [walking.walked[1]; 2]);
    feed.flying = false;
    feed.on_ground = true;
    feed.position[0] = 0.8;
    let resumed = step(&mut store, &feed);
    assert_eq!(resumed.walked[0], walking.walked[1]);
    assert!((resumed.walked[1] - walking.walked[1] - 0.12).abs() < 1e-6);
}
