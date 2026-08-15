//! Normalization coverage for the HUD-facing actor-state packets: mob effects,
//! armor equipment, runtime game-mode changes, and rider links.

use protocol::{
    ActorEffectAction, ActorLinkType, PlayerGameMode, UiEvent, WorldEvent, into_world_event,
};
use valentine::bedrock::version::v1_26_40::{
    ActorLink, ActorRuntimeId, ActorUniqueId, CerealizerNetworkItemStackDescriptorSerializedData,
    EnumsActorLinkType as VendorActorLinkType, EnumsGameType,
    EnumsMobEffectPacketPayloadEvent as MobEffectPacketEventId, MobArmorEquipmentPacket,
    MobEffectPacket, PlayerInputTick, SetActorLinkPacket, SetPlayerGameTypePacket,
};

/// 1.26.40 wraps the runtime id in a named `ActorRuntimeId` newtype.
fn runtime_id(value: u64) -> ActorRuntimeId {
    ActorRuntimeId {
        actor_runtime_id: value,
    }
}

#[test]
fn mob_effect_normalizes_to_a_bounded_actor_effect_event() {
    let packet = MobEffectPacket {
        target_runtime_id: runtime_id(42),
        event_id: MobEffectPacketEventId::Add,
        effect_id: 19,
        effect_amplifier: 1,
        show_particles: true,
        effect_duration_ticks: 600,
        tick: PlayerInputTick { inputtick: 100 },
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
            target_runtime_id: runtime_id(7),
            event_id: wire,
            effect_id: 20,
            effect_amplifier: 0,
            show_particles: false,
            effect_duration_ticks: -1,
            tick: PlayerInputTick { inputtick: 0 },
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
fn mob_effect_preserves_the_full_unsigned_tick_domain() {
    let packet = MobEffectPacket {
        target_runtime_id: runtime_id(42),
        event_id: MobEffectPacketEventId::Add,
        effect_id: 1,
        effect_amplifier: 0,
        show_particles: false,
        effect_duration_ticks: 20,
        tick: PlayerInputTick {
            inputtick: u64::MAX,
        },
        ambient: false,
    }
    .into();
    let Some(WorldEvent::ActorEffect(effect)) =
        into_world_event(packet, 0).expect("normalize maximum unsigned tick")
    else {
        panic!("expected an actor effect event")
    };
    assert_eq!(effect.tick, u64::MAX);
}

#[test]
fn mob_armor_equipment_normalizes_all_five_stacks() {
    // 1.26.40 carries one item descriptor everywhere and names the slots
    // head/torso/legs/feet/body.
    let piece = |id: i16| CerealizerNetworkItemStackDescriptorSerializedData {
        id,
        stacksize: 1,
        auxvalue: 0,
        ..Default::default()
    };
    let packet = MobArmorEquipmentPacket {
        target_runtime_id: runtime_id(9),
        head: piece(100),
        torso: piece(101),
        legs: piece(102),
        feet: CerealizerNetworkItemStackDescriptorSerializedData::default(),
        body: CerealizerNetworkItemStackDescriptorSerializedData::default(),
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
        target_runtime_id: runtime_id(0),
        head: CerealizerNetworkItemStackDescriptorSerializedData::default(),
        torso: CerealizerNetworkItemStackDescriptorSerializedData::default(),
        legs: CerealizerNetworkItemStackDescriptorSerializedData::default(),
        feet: CerealizerNetworkItemStackDescriptorSerializedData::default(),
        body: CerealizerNetworkItemStackDescriptorSerializedData::default(),
    }
    .into();
    assert!(into_world_event(packet, 0).is_err());
}

#[test]
fn set_player_game_type_normalizes_explicit_modes() {
    // 1.26.40's generated enum names only the modes Mojang still spells out:
    // Undefined(-1), Survival(0), Creative(1), Adventure(2), Default(5),
    // Spectator(6). gophertunnel's `GameTypeSurvivalSpectator` (3) and
    // `GameTypeCreativeSpectator` (4) have no named variant here and arrive as
    // `Unknown(3)` / `Unknown(4)`.
    for (wire, expected) in [
        (EnumsGameType::Survival, PlayerGameMode::Survival),
        (EnumsGameType::Creative, PlayerGameMode::Creative),
        (EnumsGameType::Adventure, PlayerGameMode::Adventure),
        (EnumsGameType::Spectator, PlayerGameMode::Spectator),
    ] {
        let packet = SetPlayerGameTypePacket {
            player_game_type: wire,
        }
        .into();
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
    // `Default` is wire value 5, the level-default sentinel gophertunnel calls
    // `GameTypeDefault`.
    for (wire, expected) in [
        (
            EnumsGameType::Default,
            protocol::GameModeUpdate::WorldDefault,
        ),
        (
            EnumsGameType::Unknown(77),
            protocol::GameModeUpdate::Unknown(77),
        ),
    ] {
        let packet = SetPlayerGameTypePacket {
            player_game_type: wire,
        }
        .into();
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
    use valentine::bedrock::version::v1_26_40::{
        SetDefaultGameTypePacket, SetDefaultGameTypePacketDefaultGameType,
    };
    let packet = SetDefaultGameTypePacket {
        default_game_type: SetDefaultGameTypePacketDefaultGameType::Adventure,
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
    // 1.26.40 types the link verb: None(0) / Riding(1) / Passenger(2), with
    // `target_a` the ridden actor and `target_b` the rider.
    for (wire, expected) in [
        (VendorActorLinkType::None, ActorLinkType::Remove),
        (VendorActorLinkType::Riding, ActorLinkType::Rider),
        (VendorActorLinkType::Passenger, ActorLinkType::Passenger),
        (VendorActorLinkType::Unknown(9), ActorLinkType::Unknown(9)),
    ] {
        let packet = SetActorLinkPacket {
            link: ActorLink {
                target_a: ActorUniqueId {
                    actor_unique_id: -55,
                },
                target_b: ActorUniqueId {
                    actor_unique_id: -7,
                },
                type_: wire,
                immediate: true,
                passenger_initiated: false,
                vehicle_angular_velocity: 0.25,
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
    // 1.26.40 hands the item user data over as an opaque buffer instead of
    // modelling its interior, so the header is written by hand here. It is the
    // shape gophertunnel's `Writer.itemUserData` (`minecraft/protocol/writer.go`)
    // emits: an int16 of -1 when a compound follows, a uint8 version of 1, then
    // the compound in fixed little-endian NBT.
    let mut encoded = vec![0xff, 0xff, 0x01];

    // Root compound { "other": byte 1, "Damage": int 37, "deep": {..} }.
    encoded.extend_from_slice(&[0x0a, 0x00, 0x00]);
    encoded.extend_from_slice(&[0x01, 0x05, 0x00]);
    encoded.extend_from_slice(b"other");
    encoded.push(0x01);
    encoded.extend_from_slice(&[0x03, 0x06, 0x00]);
    encoded.extend_from_slice(b"Damage");
    encoded.extend_from_slice(&37i32.to_le_bytes());
    encoded.extend_from_slice(&[0x0a, 0x04, 0x00]);
    encoded.extend_from_slice(b"deep");
    encoded.push(0x00);
    encoded.push(0x00);

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
    // 1.26.40 wraps both StartGame ids in named newtypes.
    game_data.start_game.entity_id = ActorUniqueId {
        actor_unique_id: -3,
    };
    game_data.start_game.runtime_id = runtime_id(3);
    let bootstrap = protocol::WorldBootstrap::from_game_data(&game_data);
    assert_eq!(bootstrap.local_player_unique_id, -3);
    assert_eq!(bootstrap.local_player_runtime_id, 3);
}
