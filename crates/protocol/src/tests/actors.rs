use bedrock_protocol_1_26_51::enums::{
    ActorLinkType as VendorActorLinkType, DataItemType, GameType,
};
use bedrock_protocol_1_26_51::packets::{
    AddActor as AddActorPacket, AddPlayer as AddPlayerPacket,
    MoveActorAbsolute as MoveActorAbsolutePacket, MoveActorDelta as MoveActorDeltaPacket,
    PlayerList as PlayerListPacket, RemoveActor as RemoveActorPacket,
    SetActorData as SetActorDataPacket, UpdateAttributes as UpdateAttributesPacket,
};
use bedrock_protocol_1_26_51::types::{
    ActorRuntimeID, ActorUniqueID, AttributeData, DataItemEntry, DataItemEntryValue, EntityLink,
    MoveActorAbsoluteData, MoveActorDeltaData, PlayerInputTick, PlayerListData, PropertySyncData,
    PropertySyncDataPropertySyncFloatEntry, PropertySyncDataPropertySyncIntEntry,
    SerializedAbilitiesData, SerializedSkinRef, SkinImage, SynchedActorDataCopyableDataList,
};
use bedrock_protocol_1_26_51::wire;
use protocol::{
    ActorEvent, ActorKind, ActorLinkType, ActorMetadataValue, ActorPositionOrigin, ActorProperty,
    PlayerListEntry, PlayerSkin, PlayerSkinUnavailable, WorldEvent, into_world_event,
};
use render_api::StandardSkin;

/// Builds the actor-data entry for a string value, keyed by raw id (4 is the name tag).
fn string_actor_data(id: i32, value: &str) -> DataItemEntry {
    DataItemEntry {
        id: wire::VarUInt(id as u32),
        payload: DataItemEntryValue::DataItemString {
            type_: DataItemType::String,
            value: value.into(),
        },
    }
}

fn float_actor_data(id: i32, value: f32) -> DataItemEntry {
    DataItemEntry {
        id: wire::VarUInt(id as u32),
        payload: DataItemEntryValue::DataItemFloat {
            type_: DataItemType::Float,
            value: wire::F32LE(value),
        },
    }
}

/// One player-list add record with every other field left at its default.
fn add_entry(unique_id: i64, name: &str, serialized_skin: SerializedSkinRef) -> PlayerListData {
    PlayerListData::AddEntry {
        action: Default::default(),
        uuid: Default::default(),
        actor_unique_id: ActorUniqueID(unique_id),
        player_name: name.into(),
        xbl_xuid: Default::default(),
        platform_online_id: Default::default(),
        build_platform: Default::default(),
        serialized_skin: Box::new(serialized_skin),
        is_teacher: false,
        is_host: false,
        is_sub_client: false,
        player_color: Default::default(),
    }
}

#[test]
fn add_entity_normalizes_to_a_vendor_neutral_actor_spawn() {
    let packet = protocol::Packet::new(AddActorPacket {
        target_actor_id: ActorUniqueID(-17),
        target_runtime_id: ActorRuntimeID(42),
        actor_type: "minecraft:bee".into(),
        position: [1.25, 70.5, -8.75].into(),
        velocity: [0.1, -0.2, 0.3].into(),
        // 1.26.40 packs pitch and yaw into a single Vec2, in that order.
        rotation: [15.0, 90.0].into(),
        y_head_rotation: wire::F32LE(80.0),
        y_body_rotation: wire::F32LE(70.0),
        actor_links: vec![
            EntityLink {
                target_a: ActorUniqueID(90),
                target_b: ActorUniqueID(-17),
                type_: VendorActorLinkType::Riding,
                immediate: true,
                passenger_initiated: false,
                vehicle_angular_velocity: wire::F32LE(0.0),
            },
            EntityLink {
                target_a: ActorUniqueID(91),
                target_b: ActorUniqueID(-18),
                type_: VendorActorLinkType::Unknown(9),
                immediate: false,
                passenger_initiated: true,
                vehicle_angular_velocity: wire::F32LE(0.0),
            },
        ],
        ..Default::default()
    });

    let Some(WorldEvent::Actor(ActorEvent::Spawn(spawn))) =
        into_world_event(packet, 2).expect("normalize add entity")
    else {
        panic!("expected actor spawn")
    };

    assert_eq!(spawn.dimension, 2);
    assert_eq!(spawn.unique_id, -17);
    assert_eq!(spawn.runtime_id, 42);
    assert_eq!(
        spawn.kind,
        ActorKind::Entity {
            identifier: "minecraft:bee".into()
        }
    );
    assert_eq!(spawn.position, [1.25, 70.5, -8.75]);
    assert_eq!(spawn.velocity, [0.1, -0.2, 0.3]);
    assert_eq!(spawn.pitch, 15.0);
    assert_eq!(spawn.yaw, 90.0);
    assert_eq!(spawn.head_yaw, 80.0);
    assert_eq!(spawn.body_yaw, 70.0);
    assert!(spawn.metadata.is_empty());
    assert!(spawn.attributes.is_empty());
    assert!(spawn.properties.is_empty());
    assert_eq!(spawn.links.len(), 2);
    assert_eq!(spawn.links[0].dimension, 2);
    assert_eq!(spawn.links[0].link_type, ActorLinkType::Rider);
    assert_eq!(spawn.links[1].link_type, ActorLinkType::Unknown(9));
    assert!(spawn.links[1].rider_initiated);
}

#[test]
fn add_player_and_remove_entity_preserve_both_actor_id_domains() {
    let uuid = Default::default();
    let add = protocol::Packet::new(AddPlayerPacket {
        uuid,
        player_name: "Alex".into(),
        player_game_type: GameType::Spectator,
        // AddPlayer has no standalone unique ID in 1.26.40; the spawned player's
        // unique ID is the first field of the embedded ability data.
        abilities_data: SerializedAbilitiesData {
            target_player_raw_id: wire::I64LE(-9),
            ..Default::default()
        },
        target_runtime_id: ActorRuntimeID(55),
        position: [1.0, 2.0, 3.0].into(),
        actor_links: vec![EntityLink {
            target_a: ActorUniqueID(77),
            target_b: ActorUniqueID(-9),
            type_: VendorActorLinkType::Passenger,
            immediate: false,
            passenger_initiated: true,
            vehicle_angular_velocity: wire::F32LE(0.0),
        }],
        ..Default::default()
    });
    let remove = protocol::Packet::new(RemoveActorPacket {
        target_actor_id: ActorUniqueID(-9),
    });

    let Some(WorldEvent::Actor(ActorEvent::PlayerSpawn { spawn, game_mode })) =
        into_world_event(add, 1).expect("normalize add player")
    else {
        panic!("expected player spawn")
    };
    assert_eq!(
        game_mode,
        protocol::GameModeUpdate::Explicit(protocol::PlayerGameMode::Spectator)
    );
    assert_eq!(spawn.unique_id, -9);
    assert_eq!(spawn.runtime_id, 55);
    assert_eq!(spawn.links[0].link_type, ActorLinkType::Passenger);
    assert_eq!(spawn.links[0].ridden_unique_id, 77);
    assert_eq!(
        spawn.kind,
        ActorKind::Player {
            uuid: [0; 16],
            username: "Alex".into(),
        }
    );

    let Some(WorldEvent::Actor(ActorEvent::Remove(remove))) =
        into_world_event(remove, 1).expect("normalize remove entity")
    else {
        panic!("expected actor removal")
    };
    assert_eq!(remove.dimension, 1);
    assert_eq!(remove.unique_id, -9);
}

#[test]
fn absolute_and_delta_actor_moves_normalize_to_partial_transform_updates() {
    let absolute = protocol::Packet::new(MoveActorAbsolutePacket {
        move_data: MoveActorAbsoluteData {
            actor_runtime_id: ActorRuntimeID(55),
            header: wire::U8(3),
            position: [4.0, 5.0, 6.0].into(),
            rotation_x: wire::U8(32),
            rotation_y: wire::U8(64),
            rotation_y_head: wire::U8(128),
        },
    });
    let delta = protocol::Packet::new(MoveActorDeltaPacket {
        move_data: MoveActorDeltaData {
            actor_runtime_id: ActorRuntimeID(55),
            new_position_x: Some(wire::F32LE(7.5)),
            new_position_y: Some(wire::F32LE(8.25)),
            // The signed byte keeps this quarter turn negative.
            rotation_y: Some(wire::I8(192_u8 as i8)),
            is_on_ground: true,
            ..Default::default()
        },
    });

    let Some(WorldEvent::Actor(ActorEvent::Move(absolute))) =
        into_world_event(absolute, 0).expect("normalize absolute move")
    else {
        panic!("expected absolute actor move")
    };
    assert_eq!(absolute.runtime_id, 55);
    assert_eq!(absolute.position, [Some(4.0), Some(5.0), Some(6.0)]);
    assert_eq!(absolute.position_origin, ActorPositionOrigin::NetworkOffset);
    assert_eq!(absolute.yaw, Some(90.0));
    assert_eq!(absolute.pitch, Some(45.0));
    assert_eq!(absolute.head_yaw, Some(-180.0));
    assert_eq!(absolute.on_ground, Some(true));
    assert!(absolute.teleported);

    let Some(WorldEvent::Actor(ActorEvent::Move(delta))) =
        into_world_event(delta, 0).expect("normalize delta move")
    else {
        panic!("expected delta actor move")
    };
    assert_eq!(delta.position, [Some(7.5), Some(8.25), None]);
    // Vanilla merges deltas into the previous absolute data,
    // so both forms share the absolute network origin.
    assert_eq!(delta.position_origin, ActorPositionOrigin::NetworkOffset);
    assert_eq!(delta.yaw, Some(-90.0));
    assert_eq!(delta.pitch, None);
    assert_eq!(delta.on_ground, Some(true));
    assert!(!delta.teleported);
}

#[test]
fn actor_rotation_bytes_decode_as_signed_turns_in_absolute_and_delta_packets() {
    for byte in 0..=u8::MAX {
        let expected = f32::from(byte as i8) * (360.0 / 256.0);
        let packets = [
            protocol::Packet::new(MoveActorAbsolutePacket {
                move_data: MoveActorAbsoluteData {
                    rotation_x: wire::U8(byte),
                    rotation_y: wire::U8(byte),
                    rotation_y_head: wire::U8(byte),
                    ..Default::default()
                },
            }),
            protocol::Packet::new(MoveActorDeltaPacket {
                move_data: MoveActorDeltaData {
                    rotation_x: Some(wire::I8(byte as i8)),
                    rotation_y: Some(wire::I8(byte as i8)),
                    rotation_y_head: Some(wire::I8(byte as i8)),
                    ..Default::default()
                },
            }),
        ];
        for packet in packets {
            let Some(WorldEvent::Actor(ActorEvent::Move(movement))) =
                into_world_event(packet, 0).unwrap()
            else {
                panic!("expected actor movement");
            };
            assert_eq!(movement.pitch, Some(expected), "byte {byte}");
            assert_eq!(movement.yaw, Some(expected), "byte {byte}");
            assert_eq!(movement.head_yaw, Some(expected), "byte {byte}");
        }
    }
}

#[test]
fn metadata_properties_and_attributes_are_normalized_without_generated_types() {
    let set_data = protocol::Packet::new(SetActorDataPacket {
        target_runtime_id: ActorRuntimeID(55),
        actor_data: SynchedActorDataCopyableDataList {
            data: vec![string_actor_data(4, "Beeatrice")],
        },
        synched_properties: PropertySyncData {
            int_entries_list: vec![PropertySyncDataPropertySyncIntEntry {
                property_index: wire::VarUInt(3),
                data: wire::ZigZag32(9),
            }],
            float_entries_list: vec![PropertySyncDataPropertySyncFloatEntry {
                property_index: wire::VarUInt(4),
                data: wire::F32LE(0.75),
            }],
        },
        tick: PlayerInputTick(10),
    });
    let attributes = protocol::Packet::new(UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeID(55),
        attribute_list: vec![AttributeData {
            min_value: wire::F32LE(0.0),
            max_value: wire::F32LE(20.0),
            current_value: wire::F32LE(17.5),
            default_min_value: wire::F32LE(0.0),
            default_max_value: wire::F32LE(20.0),
            default_value: wire::F32LE(20.0),
            name: "minecraft:health".into(),
            modifiers: vec![],
        }],
        tick: PlayerInputTick(11),
    });

    let Some(WorldEvent::Actor(ActorEvent::Metadata(update))) =
        into_world_event(set_data, 0).expect("normalize metadata")
    else {
        panic!("expected metadata update")
    };
    assert_eq!(update.runtime_id, 55);
    assert_eq!(update.tick, 10);
    assert_eq!(update.metadata[0].key, 4);
    assert_eq!(
        update.metadata[0].value,
        ActorMetadataValue::String("Beeatrice".into())
    );
    assert_eq!(
        update.properties.as_ref(),
        [
            ActorProperty::Int { index: 3, value: 9 },
            ActorProperty::Float {
                index: 4,
                value: 0.75
            }
        ]
    );

    let Some(WorldEvent::Actor(ActorEvent::Attributes(update))) =
        into_world_event(attributes, 0).expect("normalize attributes")
    else {
        panic!("expected attribute update")
    };
    assert_eq!(update.tick, 11);
    assert_eq!(update.attributes[0].name.as_ref(), "minecraft:health");
    assert_eq!(update.attributes[0].current, 17.5);
    assert_eq!(update.attributes[0].default, Some(20.0));
}

#[test]
fn the_actor_flag_words_keep_their_dedicated_metadata_values() {
    // 1.26.40 sends the flag words as ordinary Int64 payloads; only the
    // actor-data id (0, 92 and 139) distinguishes them from a plain long.
    let set_data = protocol::Packet::new(SetActorDataPacket {
        target_runtime_id: ActorRuntimeID(55),
        actor_data: SynchedActorDataCopyableDataList {
            data: vec![
                int64_actor_data(0, 0b1010),
                int64_actor_data(92, 0b0110),
                int64_actor_data(7, 42),
                int64_actor_data(139, 1),
            ],
        },
        ..Default::default()
    });

    let Some(WorldEvent::Actor(ActorEvent::Metadata(update))) =
        into_world_event(set_data, 0).expect("normalize flag metadata")
    else {
        panic!("expected metadata update")
    };
    assert_eq!(update.metadata[0].value, ActorMetadataValue::Flags(0b1010));
    assert_eq!(
        update.metadata[1].value,
        ActorMetadataValue::FlagsExtended(0b0110)
    );
    assert_eq!(update.metadata[2].value, ActorMetadataValue::Long(42));
    assert_eq!(
        update.metadata[3].value,
        ActorMetadataValue::FlagsExtended(1)
    );
}

fn int64_actor_data(id: i32, value: i64) -> DataItemEntry {
    DataItemEntry {
        id: wire::VarUInt(id as u32),
        payload: DataItemEntryValue::DataItemInt64 {
            type_: DataItemType::Int64,
            value: wire::ZigZag64(value),
        },
    }
}

#[test]
fn unmodelable_metadata_and_attributes_are_skipped_not_fatal() {
    // A metadata entry the client cannot model (non-finite float) is dropped,
    // and the surrounding well-formed entry is still retained.
    let set_data = protocol::Packet::new(SetActorDataPacket {
        target_runtime_id: ActorRuntimeID(55),
        actor_data: SynchedActorDataCopyableDataList {
            data: vec![
                float_actor_data(56, f32::NAN),
                string_actor_data(4, "Beeatrice"),
            ],
        },
        synched_properties: PropertySyncData::default(),
        tick: PlayerInputTick(10),
    });
    let Some(WorldEvent::Actor(ActorEvent::Metadata(update))) =
        into_world_event(set_data, 0).expect("metadata leniency is not fatal")
    else {
        panic!("expected metadata update")
    };
    assert_eq!(update.metadata.len(), 1);
    assert_eq!(
        update.metadata[0].value,
        ActorMetadataValue::String("Beeatrice".into())
    );

    // A non-finite attribute (servers send INFINITY for "unbounded") is dropped
    // while the finite attribute survives.
    let attributes = protocol::Packet::new(UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeID(55),
        attribute_list: vec![
            AttributeData {
                min_value: wire::F32LE(0.0),
                max_value: wire::F32LE(f32::INFINITY),
                current_value: wire::F32LE(1.0),
                default_min_value: wire::F32LE(0.0),
                default_max_value: wire::F32LE(0.0),
                default_value: wire::F32LE(0.0),
                name: "minecraft:luck".into(),
                modifiers: vec![],
            },
            AttributeData {
                min_value: wire::F32LE(0.0),
                max_value: wire::F32LE(20.0),
                current_value: wire::F32LE(17.5),
                default_min_value: wire::F32LE(0.0),
                default_max_value: wire::F32LE(20.0),
                default_value: wire::F32LE(20.0),
                name: "minecraft:health".into(),
                modifiers: vec![],
            },
        ],
        tick: PlayerInputTick(11),
    });
    let Some(WorldEvent::Actor(ActorEvent::Attributes(update))) =
        into_world_event(attributes, 0).expect("attribute leniency is not fatal")
    else {
        panic!("expected attribute update")
    };
    assert_eq!(update.attributes.len(), 1);
    assert_eq!(update.attributes[0].name.as_ref(), "minecraft:health");
}

#[test]
fn player_list_add_and_remove_normalize_to_fifo_roster_deltas() {
    let uuid = Default::default();
    // 1.26.40 tags every record individually, so an add list and a remove list
    // are just two different entry variants -- there is no packet-level action,
    // no shared record count, and no trailing verified array to cross-check.
    let mut steve = add_entry(
        77,
        "Steve",
        SerializedSkinRef {
            // The "verified" bit is the skin's own trusted flag, serialised as a string.
            trusted_skin_flag: "true".into(),
            ..Default::default()
        },
    );
    if let PlayerListData::AddEntry { xbl_xuid, .. } = &mut steve {
        *xbl_xuid = "2535400000000001".into();
    }
    let add = protocol::Packet::new(PlayerListPacket {
        entries: vec![steve],
    });
    let remove = protocol::Packet::new(PlayerListPacket {
        entries: vec![PlayerListData::RemoveEntry {
            action: Default::default(),
            uuid,
        }],
    });

    let Some(WorldEvent::Actor(ActorEvent::PlayerList(add))) =
        into_world_event(add, 0).expect("normalize player-list add")
    else {
        panic!("expected player-list add")
    };
    assert_eq!(
        add.entries.as_ref(),
        [PlayerListEntry::Add {
            uuid: [0; 16],
            unique_id: 77,
            username: "Steve".into(),
            verified: true,
            xuid: "2535400000000001".into(),
            skin: PlayerSkin::Unavailable(PlayerSkinUnavailable::InvalidDimensions),
        }]
    );

    let Some(WorldEvent::Actor(ActorEvent::PlayerList(remove))) =
        into_world_event(remove, 0).expect("normalize player-list remove")
    else {
        panic!("expected player-list remove")
    };
    assert_eq!(
        remove.entries.as_ref(),
        [PlayerListEntry::Remove { uuid: [0; 16] }]
    );
}

/// A malformed XUID is dropped without rejecting the roster entry.
#[test]
fn player_list_keeps_entries_with_malformed_xuids() {
    for xuid in ["12a", "123456789012345678901", ""] {
        let mut entry = add_entry(5, "Alex", SerializedSkinRef::default());
        if let PlayerListData::AddEntry { xbl_xuid, .. } = &mut entry {
            *xbl_xuid = xuid.into();
        }
        let packet = protocol::Packet::new(PlayerListPacket {
            entries: vec![entry],
        });
        let Some(WorldEvent::Actor(ActorEvent::PlayerList(update))) =
            into_world_event(packet, 0).expect("malformed XUIDs are not fatal")
        else {
            panic!("expected player-list add")
        };
        let PlayerListEntry::Add { xuid, .. } = &update.entries[0] else {
            panic!("expected add entry")
        };
        assert!(xuid.is_empty(), "{xuid:?} kept");
    }
}

#[test]
fn player_list_retains_bounded_standard_and_persona_baked_skins() {
    let rgba = [0x7f, 0x7f, 0x7f, 255].repeat(render_api::CLASSIC_SKIN_SIDE.pow(2));
    let classic = add_entry(
        0,
        "Classic",
        SerializedSkinRef {
            image_data: SkinImage {
                width: wire::U32LE(64),
                height: wire::U32LE(64),
                image_bytes: rgba.clone().into(),
            },
            trusted_skin_flag: "true".into(),
            ..Default::default()
        },
    );
    let persona = add_entry(
        0,
        "Persona",
        SerializedSkinRef {
            is_persona: true,
            image_data: SkinImage {
                width: wire::U32LE(64),
                height: wire::U32LE(64),
                image_bytes: rgba.into(),
            },
            ..Default::default()
        },
    );
    let packet = protocol::Packet::new(PlayerListPacket {
        entries: vec![classic, persona],
    });

    let Some(WorldEvent::Actor(ActorEvent::PlayerList(update))) =
        into_world_event(packet, 0).expect("normalize player-list skins")
    else {
        panic!("expected player-list update")
    };

    let PlayerListEntry::Add {
        skin,
        verified: true,
        ..
    } = &update.entries[0]
    else {
        panic!("expected trusted add entry")
    };
    assert_eq!(
        skin,
        &PlayerSkin::Standard(StandardSkin {
            geometry: None,
            cape: None,
            width: 64,
            height: 64,
            rgba8: [0x7f, 0x7f, 0x7f, 255]
                .repeat(render_api::CLASSIC_SKIN_SIDE.pow(2))
                .into(),
        })
    );
    let PlayerListEntry::Add {
        skin,
        verified: false,
        ..
    } = &update.entries[1]
    else {
        panic!("expected untrusted add entry")
    };
    assert!(
        matches!(skin, PlayerSkin::Standard(skin) if skin.width == 64),
        "a persona skin keeps its baked image"
    );
}

#[test]
fn actor_normalization_rejects_unbounded_or_non_finite_fields() {
    let too_long = protocol::Packet::new(AddActorPacket {
        actor_type: "x".repeat(protocol::MAX_ACTOR_IDENTIFIER_BYTES + 1).into(),
        ..Default::default()
    });
    let non_finite = protocol::Packet::new(AddActorPacket {
        actor_type: "minecraft:bee".into(),
        rotation: [0.0, f32::NAN].into(),
        ..Default::default()
    });
    let too_many_links = protocol::Packet::new(AddActorPacket {
        actor_type: "minecraft:bee".into(),
        actor_links: vec![EntityLink::default(); protocol::MAX_ACTOR_LINKS_PER_SPAWN + 1],
        ..Default::default()
    });
    let too_many_player_links = protocol::Packet::new(AddPlayerPacket {
        actor_links: vec![EntityLink::default(); protocol::MAX_ACTOR_LINKS_PER_SPAWN + 1],
        ..Default::default()
    });

    assert!(into_world_event(too_long, 0).is_err());
    assert!(into_world_event(non_finite, 0).is_err());
    assert!(into_world_event(too_many_links, 0).is_err());
    assert!(into_world_event(too_many_player_links, 0).is_err());
}
