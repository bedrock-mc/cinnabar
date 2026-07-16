use protocol::{
    ActorEvent, ActorKind, ActorMetadataValue, ActorPositionOrigin, ActorProperty, PlayerListEntry,
    PlayerSkin, PlayerSkinUnavailable, StandardSkin, WorldEvent, into_world_event,
};
use valentine::bedrock::version::v1_26_30::{
    AddEntityPacket, AddPlayerPacket, DeltaMoveFlags, EntityProperties, EntityPropertiesFloatsItem,
    EntityPropertiesIntsItem, MetadataDictionaryItem, MetadataDictionaryItemKey,
    MetadataDictionaryItemType, MetadataDictionaryItemValue, MetadataDictionaryItemValueDefault,
    MoveEntityDeltaPacket, MoveEntityPacket, PlayerAttributesItem, PlayerListPacket, PlayerRecords,
    PlayerRecordsRecordsItem, PlayerRecordsRecordsItemAdd, PlayerRecordsRecordsItemRemove,
    PlayerRecordsType, RemoveEntityPacket, Rotation, SetEntityDataPacket, Skin, SkinImage,
    UpdateAttributesPacket, Vec3F,
};

#[test]
fn add_entity_normalizes_to_a_vendor_neutral_actor_spawn() {
    let packet = AddEntityPacket {
        unique_id: -17,
        runtime_id: 42,
        entity_type: "minecraft:bee".to_owned(),
        position: Vec3F {
            x: 1.25,
            y: 70.5,
            z: -8.75,
        },
        velocity: Vec3F {
            x: 0.1,
            y: -0.2,
            z: 0.3,
        },
        pitch: 15.0,
        yaw: 90.0,
        head_yaw: 80.0,
        body_yaw: 70.0,
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
}

#[test]
fn add_player_and_remove_entity_preserve_both_actor_id_domains() {
    let uuid = Default::default();
    let add = AddPlayerPacket {
        uuid,
        username: "Alex".to_owned(),
        unique_id: -9,
        runtime_id: 55,
        position: Vec3F {
            x: 1.0,
            y: 2.0,
            z: 3.0,
        },
        ..Default::default()
    }
    .into();
    let remove = RemoveEntityPacket { entity_id_self: -9 }.into();

    let Some(WorldEvent::Actor(ActorEvent::Spawn(spawn))) =
        into_world_event(add, 1).expect("normalize add player")
    else {
        panic!("expected player spawn")
    };
    assert_eq!(spawn.unique_id, -9);
    assert_eq!(spawn.runtime_id, 55);
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
    let absolute = MoveEntityPacket {
        runtime_entity_id: 55,
        flags: 3,
        position: Vec3F {
            x: 4.0,
            y: 5.0,
            z: 6.0,
        },
        rotation: Rotation {
            yaw: vec![64],
            pitch: vec![32],
            head_yaw: vec![128],
        },
    }
    .into();
    let delta = MoveEntityDeltaPacket {
        runtime_entity_id: 55,
        flags: DeltaMoveFlags::HAS_X
            | DeltaMoveFlags::HAS_Y
            | DeltaMoveFlags::HAS_ROT_Y
            | DeltaMoveFlags::ON_GROUND,
        x: Some(7.5),
        y: Some(8.25),
        rot_y: Some(192),
        ..Default::default()
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
    let metadata = MetadataDictionaryItem {
        key: MetadataDictionaryItemKey::Nametag,
        type_: MetadataDictionaryItemType::String,
        value: MetadataDictionaryItemValue::Default(Box::new(Some(
            MetadataDictionaryItemValueDefault::String("Beeatrice".to_owned()),
        ))),
    };
    let set_data = SetEntityDataPacket {
        runtime_entity_id: 55,
        metadata: vec![metadata],
        properties: EntityProperties {
            ints: vec![EntityPropertiesIntsItem { index: 3, value: 9 }],
            floats: vec![EntityPropertiesFloatsItem {
                index: 4,
                value: 0.75,
            }],
        },
        tick: 10,
    }
    .into();
    let attributes = UpdateAttributesPacket {
        runtime_entity_id: 55,
        attributes: vec![PlayerAttributesItem {
            min: 0.0,
            max: 20.0,
            current: 17.5,
            default_min: 0.0,
            default_max: 20.0,
            default: 20.0,
            name: "minecraft:health".to_owned(),
            modifiers: vec![],
        }],
        tick: 11,
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
fn player_list_add_and_remove_normalize_to_fifo_roster_deltas() {
    let uuid = Default::default();
    let add = PlayerListPacket {
        records: PlayerRecords {
            type_: PlayerRecordsType::Add,
            records_count: 1,
            records: vec![Some(PlayerRecordsRecordsItem::Add(Box::new(
                PlayerRecordsRecordsItemAdd {
                    uuid,
                    entity_unique_id: 77,
                    username: "Steve".to_owned(),
                    ..Default::default()
                },
            )))],
            verified: Some(vec![true]),
        },
    }
    .into();
    let remove = PlayerListPacket {
        records: PlayerRecords {
            type_: PlayerRecordsType::Remove,
            records_count: 1,
            records: vec![Some(PlayerRecordsRecordsItem::Remove(
                PlayerRecordsRecordsItemRemove { uuid },
            ))],
            verified: None,
        },
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
    let classic = PlayerRecordsRecordsItemAdd {
        username: "Classic".to_owned(),
        skin_data: Skin {
            skin_data: SkinImage {
                width: 64,
                height: 64,
                data: rgba.clone(),
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let persona = PlayerRecordsRecordsItemAdd {
        username: "Persona".to_owned(),
        skin_data: Skin {
            persona: true,
            skin_data: SkinImage {
                width: 64,
                height: 64,
                data: rgba,
            },
            ..Default::default()
        },
        ..Default::default()
    };
    let packet = PlayerListPacket {
        records: PlayerRecords {
            type_: PlayerRecordsType::Add,
            records_count: 2,
            records: vec![
                Some(PlayerRecordsRecordsItem::Add(Box::new(classic))),
                Some(PlayerRecordsRecordsItem::Add(Box::new(persona))),
            ],
            verified: Some(vec![true, false]),
        },
    }
    .into();

    let Some(WorldEvent::Actor(ActorEvent::PlayerList(update))) =
        into_world_event(packet, 0).expect("normalize player-list skins")
    else {
        panic!("expected player-list update")
    };

    let PlayerListEntry::Add { skin, .. } = &update.entries[0] else {
        panic!("expected add entry")
    };
    assert_eq!(
        skin,
        &PlayerSkin::Standard(StandardSkin {
            width: 64,
            height: 64,
            rgba8: vec![0x7f; 64 * 64 * 4].into(),
        })
    );
    let PlayerListEntry::Add { skin, .. } = &update.entries[1] else {
        panic!("expected add entry")
    };
    assert_eq!(
        skin,
        &PlayerSkin::Unavailable(PlayerSkinUnavailable::UnsupportedPersona)
    );
}

#[test]
fn actor_normalization_rejects_unbounded_or_non_finite_fields() {
    let too_long = AddEntityPacket {
        entity_type: "x".repeat(protocol::MAX_ACTOR_IDENTIFIER_BYTES + 1),
        ..Default::default()
    }
    .into();
    let non_finite = AddEntityPacket {
        entity_type: "minecraft:bee".to_owned(),
        yaw: f32::NAN,
        ..Default::default()
    }
    .into();

    assert!(into_world_event(too_long, 0).is_err());
    assert!(into_world_event(non_finite, 0).is_err());
}
