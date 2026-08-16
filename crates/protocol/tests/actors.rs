use protocol::{
    ActorEvent, ActorKind, ActorLinkType, ActorMetadataValue, ActorPositionOrigin, ActorProperty,
    PlayerListEntry, PlayerSkin, PlayerSkinUnavailable, StandardSkin, WorldEvent, into_world_event,
};
use valentine::bedrock::version::v1_26_44::{
    ActorLink, ActorRuntimeId, ActorUniqueId, AddActorPacket, AddPlayerPacket, AttributeData,
    DataItemEntry, DataItemEntryPayload, DataItemFloatPayload, DataItemStringPayload,
    EnumsActorLinkType as VendorActorLinkType, EnumsDataItemType, MoveActorAbsoluteData,
    MoveActorAbsolutePacket, MoveActorDeltaData, MoveActorDeltaPacket, PlayerInputTick,
    PlayerListPacket, PlayerListPacketEntriesItem, PlayerListPacketPayloadAddEntry,
    PlayerListPacketPayloadRemoveEntry, PropertySyncData, PropertySyncDataPropertySyncFloatEntry,
    PropertySyncDataPropertySyncIntEntry, RemoveActorPacket, SerializedAbilitiesData,
    SerializedSkinRef, SetActorDataPacket, SkinImage, SynchedActorDataCopyableDataList,
    UpdateAttributesPacket, Vec2, Vec3,
};

/// Builds the actor-data entry 1.26.40 puts on the wire for a string value.
///
/// 1.26.40 keys actor data by raw id (4 is the name tag) and tags each payload
/// with its own value type, so there is no named key enum to spell out.
fn string_actor_data(id: i32, value: &str) -> DataItemEntry {
    DataItemEntry {
        id: id as u32,
        payload: DataItemEntryPayload::DataItemStringPayload(DataItemStringPayload {
            type_: EnumsDataItemType::String,
            value: value.to_owned(),
        }),
    }
}

fn float_actor_data(id: i32, value: f32) -> DataItemEntry {
    DataItemEntry {
        id: id as u32,
        payload: DataItemEntryPayload::DataItemFloatPayload(DataItemFloatPayload {
            type_: EnumsDataItemType::Float,
            value,
        }),
    }
}

#[test]
fn add_entity_normalizes_to_a_vendor_neutral_actor_spawn() {
    let packet = AddActorPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: -17,
        },
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: 42,
        },
        actor_type: "minecraft:bee".to_owned(),
        position: Vec3 {
            x: 1.25,
            y: 70.5,
            z: -8.75,
        },
        velocity: Vec3 {
            x: 0.1,
            y: -0.2,
            z: 0.3,
        },
        // 1.26.40 packs pitch and yaw into a single Vec2, in that order.
        rotation: Vec2 { x: 15.0, y: 90.0 },
        y_head_rotation: 80.0,
        y_body_rotation: 70.0,
        actor_links: vec![
            ActorLink {
                target_a: ActorUniqueId {
                    actor_unique_id: 90,
                },
                target_b: ActorUniqueId {
                    actor_unique_id: -17,
                },
                type_: VendorActorLinkType::Riding,
                immediate: true,
                passenger_initiated: false,
                vehicle_angular_velocity: 0.0,
            },
            ActorLink {
                target_a: ActorUniqueId {
                    actor_unique_id: 91,
                },
                target_b: ActorUniqueId {
                    actor_unique_id: -18,
                },
                type_: VendorActorLinkType::Unknown(9),
                immediate: false,
                passenger_initiated: true,
                vehicle_angular_velocity: 0.0,
            },
        ],
        ..Default::default()
    }
    .into();

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
    let add = AddPlayerPacket {
        uuid,
        player_name: "Alex".to_owned(),
        // AddPlayer has no standalone unique ID in 1.26.40; the spawned player's
        // unique ID is the first field of the embedded ability data.
        abilities_data: SerializedAbilitiesData {
            target_player_raw_id: -9,
            ..Default::default()
        },
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: 55,
        },
        position: Vec3 {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        actor_links: vec![ActorLink {
            target_a: ActorUniqueId {
                actor_unique_id: 77,
            },
            target_b: ActorUniqueId {
                actor_unique_id: -9,
            },
            type_: VendorActorLinkType::Passenger,
            immediate: false,
            passenger_initiated: true,
            vehicle_angular_velocity: 0.0,
        }],
        ..Default::default()
    }
    .into();
    let remove = RemoveActorPacket {
        target_actor_id: ActorUniqueId {
            actor_unique_id: -9,
        },
    }
    .into();

    let Some(WorldEvent::Actor(ActorEvent::Spawn(spawn))) =
        into_world_event(add, 1).expect("normalize add player")
    else {
        panic!("expected player spawn")
    };
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
    let absolute = MoveActorAbsolutePacket {
        move_data: MoveActorAbsoluteData {
            actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: 55,
            },
            header: 3,
            position: Vec3 {
                x: 4.0,
                y: 5.0,
                z: 6.0,
            },
            rotation_x: 32,
            rotation_y: 64,
            rotation_y_head: 128,
        },
    }
    .into();
    let delta = MoveActorDeltaPacket {
        move_data: MoveActorDeltaData {
            actor_runtime_id: ActorRuntimeId {
                actor_runtime_id: 55,
            },
            new_position_x: Some(7.5),
            new_position_y: Some(8.25),
            // Bedrock reads rotation bytes unsigned; the generator types them i8.
            rotation_y: Some(192_u8 as i8),
            is_on_ground: true,
            ..Default::default()
        },
    }
    .into();

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
    assert_eq!(absolute.head_yaw, Some(180.0));
    assert_eq!(absolute.on_ground, Some(true));
    assert!(absolute.teleported);

    let Some(WorldEvent::Actor(ActorEvent::Move(delta))) =
        into_world_event(delta, 0).expect("normalize delta move")
    else {
        panic!("expected delta actor move")
    };
    assert_eq!(delta.position, [Some(7.5), Some(8.25), None]);
    assert_eq!(delta.position_origin, ActorPositionOrigin::Feet);
    assert_eq!(delta.yaw, Some(270.0));
    assert_eq!(delta.pitch, None);
    assert_eq!(delta.on_ground, Some(true));
    assert!(!delta.teleported);
}

#[test]
fn metadata_properties_and_attributes_are_normalized_without_generated_types() {
    let set_data = SetActorDataPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: 55,
        },
        actor_data: SynchedActorDataCopyableDataList {
            data: vec![string_actor_data(4, "Beeatrice")],
        },
        synched_properties: PropertySyncData {
            int_entries_list: vec![PropertySyncDataPropertySyncIntEntry {
                property_index: 3,
                data: 9,
            }],
            float_entries_list: vec![PropertySyncDataPropertySyncFloatEntry {
                property_index: 4,
                data: 0.75,
            }],
        },
        tick: PlayerInputTick { inputtick: 10 },
    }
    .into();
    let attributes = UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: 55,
        },
        attribute_list: vec![AttributeData {
            min_value: 0.0,
            max_value: 20.0,
            current_value: 17.5,
            default_min_value: 0.0,
            default_max_value: 20.0,
            default_value: 20.0,
            name: "minecraft:health".to_owned(),
            modifiers: vec![],
        }],
        tick: PlayerInputTick { inputtick: 11 },
    }
    .into();

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
fn the_two_actor_flag_words_keep_their_dedicated_metadata_values() {
    // 1.26.40 sends both flag words as ordinary Int64 payloads; only the
    // actor-data id (0 and 92) distinguishes them from a plain long.
    let set_data = SetActorDataPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: 55,
        },
        actor_data: SynchedActorDataCopyableDataList {
            data: vec![
                int64_actor_data(0, 0b1010),
                int64_actor_data(92, 0b0110),
                int64_actor_data(7, 42),
            ],
        },
        ..Default::default()
    }
    .into();

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
}

fn int64_actor_data(id: i32, value: i64) -> DataItemEntry {
    use valentine::bedrock::version::v1_26_44::DataItemInt64Payload;

    DataItemEntry {
        id: id as u32,
        payload: DataItemEntryPayload::DataItemInt64Payload(DataItemInt64Payload {
            type_: EnumsDataItemType::Int64,
            value,
        }),
    }
}

#[test]
fn unmodelable_metadata_and_attributes_are_skipped_not_fatal() {
    // A metadata entry the client cannot model (non-finite float) is dropped,
    // and the surrounding well-formed entry is still retained.
    let set_data = SetActorDataPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: 55,
        },
        actor_data: SynchedActorDataCopyableDataList {
            data: vec![
                float_actor_data(56, f32::NAN),
                string_actor_data(4, "Beeatrice"),
            ],
        },
        synched_properties: PropertySyncData::default(),
        tick: PlayerInputTick { inputtick: 10 },
    }
    .into();
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
    let attributes = UpdateAttributesPacket {
        target_runtime_id: ActorRuntimeId {
            actor_runtime_id: 55,
        },
        attribute_list: vec![
            AttributeData {
                min_value: 0.0,
                max_value: f32::INFINITY,
                current_value: 1.0,
                default_min_value: 0.0,
                default_max_value: 0.0,
                default_value: 0.0,
                name: "minecraft:luck".to_owned(),
                modifiers: vec![],
            },
            AttributeData {
                min_value: 0.0,
                max_value: 20.0,
                current_value: 17.5,
                default_min_value: 0.0,
                default_max_value: 20.0,
                default_value: 20.0,
                name: "minecraft:health".to_owned(),
                modifiers: vec![],
            },
        ],
        tick: PlayerInputTick { inputtick: 11 },
    }
    .into();
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
    let add = PlayerListPacket {
        entries: vec![PlayerListPacketEntriesItem::AddEntry(Box::new(
            PlayerListPacketPayloadAddEntry {
                uuid,
                actor_unique_id: ActorUniqueId {
                    actor_unique_id: 77,
                },
                player_name: "Steve".to_owned(),
                serialized_skin: SerializedSkinRef {
                    // The old trailing "verified" bool is now the skin's own
                    // trusted flag, serialised as a string.
                    trusted_skin_flag: "true".to_owned(),
                    ..Default::default()
                },
                ..Default::default()
            },
        ))],
    }
    .into();
    let remove = PlayerListPacket {
        entries: vec![PlayerListPacketEntriesItem::RemoveEntry(
            PlayerListPacketPayloadRemoveEntry {
                uuid,
                ..Default::default()
            },
        )],
    }
    .into();

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

#[test]
fn player_list_retains_bounded_standard_skin_and_marks_persona_explicitly() {
    let rgba = vec![0x7f; 64 * 64 * 4];
    let classic = PlayerListPacketPayloadAddEntry {
        player_name: "Classic".to_owned(),
        serialized_skin: SerializedSkinRef {
            image_data: SkinImage {
                width: 64,
                height: 64,
                image_bytes: rgba.clone(),
            },
            trusted_skin_flag: "true".to_owned(),
            ..Default::default()
        },
        ..Default::default()
    };
    let persona = PlayerListPacketPayloadAddEntry {
        player_name: "Persona".to_owned(),
        serialized_skin: SerializedSkinRef {
            is_persona: true,
            image_data: SkinImage {
                width: 64,
                height: 64,
                image_bytes: rgba,
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let packet = PlayerListPacket {
        entries: vec![
            PlayerListPacketEntriesItem::AddEntry(Box::new(classic)),
            PlayerListPacketEntriesItem::AddEntry(Box::new(persona)),
        ],
    }
    .into();

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
            width: 64,
            height: 64,
            rgba8: vec![0x7f; 64 * 64 * 4].into(),
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
    assert_eq!(
        skin,
        &PlayerSkin::Unavailable(PlayerSkinUnavailable::UnsupportedPersona)
    );
}

#[test]
fn actor_normalization_rejects_unbounded_or_non_finite_fields() {
    let too_long = AddActorPacket {
        actor_type: "x".repeat(protocol::MAX_ACTOR_IDENTIFIER_BYTES + 1),
        ..Default::default()
    }
    .into();
    let non_finite = AddActorPacket {
        actor_type: "minecraft:bee".to_owned(),
        rotation: Vec2 {
            x: 0.0,
            y: f32::NAN,
        },
        ..Default::default()
    }
    .into();
    let too_many_links = AddActorPacket {
        actor_type: "minecraft:bee".to_owned(),
        actor_links: vec![ActorLink::default(); protocol::MAX_ACTOR_LINKS_PER_SPAWN + 1],
        ..Default::default()
    }
    .into();
    let too_many_player_links = AddPlayerPacket {
        actor_links: vec![ActorLink::default(); protocol::MAX_ACTOR_LINKS_PER_SPAWN + 1],
        ..Default::default()
    }
    .into();

    assert!(into_world_event(too_long, 0).is_err());
    assert!(into_world_event(non_finite, 0).is_err());
    assert!(into_world_event(too_many_links, 0).is_err());
    assert!(into_world_event(too_many_player_links, 0).is_err());
}
