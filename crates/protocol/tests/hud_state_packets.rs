//! Normalization coverage for the HUD-facing actor-state packets: mob effects,
//! armor equipment, runtime game-mode changes, and rider links.

use protocol::{
    ActorEffectAction, ActorLinkType, PlayerGameMode, UiEvent, WorldEvent, into_world_event,
};
use valentine::bedrock::version::v1_26_30::{
    GameMode, ItemV4, Link, MobArmorEquipmentPacket, MobEffectPacket, MobEffectPacketEventId,
    SetEntityLinkPacket, SetPlayerGameTypePacket,
};

#[test]
fn mob_effect_normalizes_to_a_bounded_actor_effect_event() {
    let packet = MobEffectPacket {
        runtime_entity_id: 42,
        event_id: MobEffectPacketEventId::Add,
        effect_id: 19,
        amplifier: 1,
        particles: true,
        duration: 600,
        tick: 100,
        ambient: false,
    }
    .into();

    let Some(WorldEvent::ActorEffect(effect)) =
        into_world_event(packet, 0).expect("normalize mob effect")
    else {
        panic!("expected an actor effect event")
    };
    assert_eq!(effect.dimension, 0);
    assert_eq!(effect.actor_runtime_id, 42);
    assert_eq!(effect.action, ActorEffectAction::Add);
    assert_eq!(effect.effect_id, 19);
    assert_eq!(effect.amplifier, 1);
    assert!(effect.particles);
    assert!(!effect.ambient);
    assert_eq!(effect.duration_ticks, 600);
    assert_eq!(effect.tick, 100);
}

#[test]
fn mob_effect_update_remove_and_unknown_actions_stay_typed() {
    for (wire, expected) in [
        (MobEffectPacketEventId::Update, ActorEffectAction::Update),
        (MobEffectPacketEventId::Remove, ActorEffectAction::Remove),
        (
            MobEffectPacketEventId::Unknown(9),
            ActorEffectAction::Unknown(9),
        ),
    ] {
        let packet = MobEffectPacket {
            runtime_entity_id: 7,
            event_id: wire,
            effect_id: 20,
            amplifier: 0,
            particles: false,
            duration: -1,
            tick: 0,
            ambient: true,
        }
        .into();
        let Some(WorldEvent::ActorEffect(effect)) =
            into_world_event(packet, 1).expect("normalize mob effect action")
        else {
            panic!("expected an actor effect event")
        };
        assert_eq!(effect.action, expected);
        assert_eq!(effect.duration_ticks, -1);
        assert!(effect.ambient);
    }
}

#[test]
fn mob_effect_negative_tick_fails_closed_as_a_semantic_error() {
    let packet = MobEffectPacket {
        runtime_entity_id: 42,
        event_id: MobEffectPacketEventId::Add,
        effect_id: 1,
        amplifier: 0,
        particles: false,
        duration: 20,
        tick: -5,
        ambient: false,
    }
    .into();
    assert!(into_world_event(packet, 0).is_err());
}

#[test]
fn mob_armor_equipment_normalizes_all_five_stacks() {
    let piece = |network_id: i16| ItemV4 {
        network_id,
        count: 1,
        metadata: 0,
        ..Default::default()
    };
    let packet = MobArmorEquipmentPacket {
        runtime_entity_id: 9,
        helmet: piece(100),
        chestplate: piece(101),
        leggings: piece(102),
        boots: ItemV4::default(),
        body: ItemV4::default(),
    }
    .into();

    let Some(WorldEvent::ArmorEquipment(armor)) =
        into_world_event(packet, 0).expect("normalize armor equipment")
    else {
        panic!("expected an armor equipment event")
    };
    assert_eq!(armor.actor_runtime_id, 9);
    assert_eq!(armor.helmet.network_id, 100);
    assert_eq!(armor.chestplate.network_id, 101);
    assert_eq!(armor.leggings.network_id, 102);
    assert!(armor.boots.is_empty());
    assert!(armor.body.is_empty());
}

#[test]
fn mob_armor_equipment_zero_runtime_id_fails_closed() {
    let packet = MobArmorEquipmentPacket {
        runtime_entity_id: 0,
        helmet: ItemV4::default(),
        chestplate: ItemV4::default(),
        leggings: ItemV4::default(),
        boots: ItemV4::default(),
        body: ItemV4::default(),
    }
    .into();
    assert!(into_world_event(packet, 0).is_err());
}

#[test]
fn set_player_game_type_normalizes_explicit_modes() {
    for (wire, expected) in [
        (GameMode::Survival, PlayerGameMode::Survival),
        (GameMode::Creative, PlayerGameMode::Creative),
        (GameMode::Adventure, PlayerGameMode::Adventure),
        (GameMode::Spectator, PlayerGameMode::Spectator),
        (GameMode::SurvivalSpectator, PlayerGameMode::Spectator),
        (GameMode::CreativeSpectator, PlayerGameMode::Spectator),
    ] {
        let packet = SetPlayerGameTypePacket { gamemode: wire }.into();
        let Some(WorldEvent::Ui(UiEvent::GameMode(event))) =
            into_world_event(packet, 0).expect("normalize game type")
        else {
            panic!("expected a game mode event")
        };
        assert_eq!(event.update, protocol::GameModeUpdate::Explicit(expected));
    }
}

#[test]
fn set_player_game_type_fallback_and_unknown_stay_typed_without_a_guess() {
    for (wire, expected) in [
        (GameMode::Fallback, protocol::GameModeUpdate::WorldDefault),
        (GameMode::Unknown(77), protocol::GameModeUpdate::Unknown(77)),
    ] {
        let packet = SetPlayerGameTypePacket { gamemode: wire }.into();
        let Some(WorldEvent::Ui(UiEvent::GameMode(event))) =
            into_world_event(packet, 0).expect("normalize odd game type")
        else {
            panic!("expected a game mode event")
        };
        assert_eq!(event.update, expected);
    }
}

#[test]
fn set_default_game_type_dispatches_as_a_default_mode_event() {
    use valentine::bedrock::version::v1_26_30::SetDefaultGameTypePacket;
    let packet = SetDefaultGameTypePacket {
        gamemode: GameMode::Adventure,
    }
    .into();
    let Some(WorldEvent::Ui(UiEvent::DefaultGameMode(event))) =
        into_world_event(packet, 0).expect("normalize default game type")
    else {
        panic!("expected a default game mode event")
    };
    assert_eq!(
        event.update,
        protocol::GameModeUpdate::Explicit(PlayerGameMode::Adventure)
    );
}

#[test]
fn set_entity_link_normalizes_typed_rider_links() {
    for (wire, expected) in [
        (0u8, ActorLinkType::Remove),
        (1u8, ActorLinkType::Rider),
        (2u8, ActorLinkType::Passenger),
        (9u8, ActorLinkType::Unknown(9)),
    ] {
        let packet = SetEntityLinkPacket {
            link: Link {
                ridden_entity_id: -55,
                rider_entity_id: -7,
                type_: wire,
                immediate: true,
                rider_initiated: false,
                angular_velocity: 0.25,
            },
        }
        .into();
        let Some(WorldEvent::ActorLink(link)) =
            into_world_event(packet, 0).expect("normalize entity link")
        else {
            panic!("expected an actor link event")
        };
        assert_eq!(link.ridden_unique_id, -55);
        assert_eq!(link.rider_unique_id, -7);
        assert_eq!(link.link_type, expected);
        assert!(link.immediate);
        assert!(!link.rider_initiated);
    }
}

#[test]
fn item_stack_damage_reads_the_root_damage_tag_and_fails_closed_on_junk() {
    use valentine::bedrock::codec::{BedrockCodec, Nbt};
    use valentine::bedrock::version::v1_26_30::{
        ItemExtraDataWithoutBlockingTick, ItemExtraDataWithoutBlockingTickNbt,
    };

    // Root compound { "other": byte 1, "Damage": int 37, "deep": {..} }.
    let mut nbt = vec![0x0a, 0x00, 0x00];
    nbt.extend_from_slice(&[0x01, 0x05, 0x00]);
    nbt.extend_from_slice(b"other");
    nbt.push(0x01);
    nbt.extend_from_slice(&[0x03, 0x06, 0x00]);
    nbt.extend_from_slice(b"Damage");
    nbt.extend_from_slice(&37i32.to_le_bytes());
    nbt.extend_from_slice(&[0x0a, 0x04, 0x00]);
    nbt.extend_from_slice(b"deep");
    nbt.push(0x00);
    nbt.push(0x00);
    let extra = ItemExtraDataWithoutBlockingTick {
        nbt: Some(ItemExtraDataWithoutBlockingTickNbt {
            version: 1,
            nbt: Nbt(nbt.into()),
        }),
        can_place_on: Vec::new(),
        can_destroy: Vec::new(),
    };
    let mut encoded = bytes::BytesMut::new();
    extra.encode(&mut encoded).unwrap();
    let encoded: Vec<u8> = encoded.to_vec();

    let stack = protocol::NetworkItemStack {
        network_id: 5,
        metadata: 0,
        stack_network_id: -1,
        count: 1,
        nbt_digest: [0; 32],
        block_runtime_id: 0,
        extra_data: encoded.into(),
    };
    assert_eq!(protocol::item_stack_damage(&stack), Some(37));

    let empty = protocol::NetworkItemStack::empty();
    assert_eq!(protocol::item_stack_damage(&empty), None);

    let junk = protocol::NetworkItemStack {
        extra_data: vec![0xff, 0x13, 0x37].into(),
        ..stack
    };
    assert_eq!(protocol::item_stack_damage(&junk), None);
}

#[test]
fn world_bootstrap_carries_the_local_player_unique_id() {
    let mut game_data = protocol::GameData {
        start_game: Default::default(),
        item_registry: Default::default(),
        biome_definitions: None,
        entity_identifiers: None,
        creative_content: None,
    };
    game_data.start_game.entity_id = -3;
    game_data.start_game.runtime_entity_id = 3;
    let bootstrap = protocol::WorldBootstrap::from_game_data(&game_data);
    assert_eq!(bootstrap.local_player_unique_id, -3);
    assert_eq!(bootstrap.local_player_runtime_id, 3);
}
