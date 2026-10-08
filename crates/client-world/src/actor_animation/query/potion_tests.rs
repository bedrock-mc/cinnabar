use super::potion::AUX_VALUE_METADATA_KEY as AUX;
use super::*;

/// Evaluates the appearance query with an unrelated held potion in the player context.
fn variant(actor: &ActorSnapshot, held_metadata: u32) -> f32 {
    let input = ActorTickInput::default();
    let context = ActorTickContext {
        main_hand: Some("minecraft:splash_potion".into()),
        main_hand_metadata: held_metadata,
        ..ActorTickContext::default()
    };
    query(
        &QueryInputs {
            actor,
            input: &input,
            context: &context,
            anim_tick: 0,
            anim_time: None,
            life_tick: 0,
            finished: (false, false),
            bones: &[],
            bone_names: &[],
        },
        "query.variant",
        &[],
    )
    .number()
}

#[test]
fn thrown_potion_variant_uses_its_aux_value_instead_of_generic_variant_or_held_item() {
    for identifier in ["minecraft:splash_potion", "minecraft:lingering_potion"] {
        for (aux, expected) in [
            (0, 30),
            (5, 15),
            (14, 1),
            (21, 5),
            (22, 5),
            (23, 6),
            (25, 18),
            (43, 26),
            (46, 29),
        ] {
            let mut actor = crate::actor_animation::tests::actor_with_metadata(HashMap::from([
                (AUX, ActorMetadataValue::Short(aux)),
                (
                    crate::actor_store::VARIANT_METADATA_KEY,
                    ActorMetadataValue::Int(12),
                ),
            ]));
            actor.kind = ActorKind::Entity {
                identifier: identifier.into(),
            };
            for held in [0, 21, 25] {
                assert_eq!(
                    variant(&actor, held),
                    expected as f32,
                    "{identifier} aux {aux}, held {held}"
                );
            }
        }
    }
}

#[test]
fn thrown_potion_variant_retains_normalized_add_actor_and_metadata_updates() {
    use protocol::wire::valentine::bedrock::version::v1_26_51::{
        ActorRuntimeId, ActorUniqueId, AddActorPacket, DataItemEntry, DataItemEntryPayload,
        DataItemShortPayload, EnumsDataItemType, SynchedActorDataCopyableDataList,
    };
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
    let mut actors = crate::actor_store::ActorStore::new(1, 0);
    actors.apply(1, 1, event);
    assert_eq!(variant(actors.get(7).unwrap(), 25), 5.0);
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
    assert_eq!(variant(actors.get(7).unwrap(), 21), 18.0);
}

#[test]
fn potion_variant_defaults_wrong_types_and_bounds_unknown_auxiliary_values() {
    let mut actor = crate::actor_animation::tests::actor_with_metadata(HashMap::new());
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:splash_potion".into(),
    };
    assert_eq!(variant(&actor, 21), 30.0);
    for value in [ActorMetadataValue::Int(21), ActorMetadataValue::Float(21.0)] {
        actor.metadata.insert(AUX, value);
        assert_eq!(variant(&actor, 21), 30.0);
    }
    for auxiliary in [-1, 47, 63, 64, i16::MAX] {
        actor
            .metadata
            .insert(AUX, ActorMetadataValue::Short(auxiliary));
        assert_eq!(variant(&actor, 21), 0.0);
    }
    actor.kind = ActorKind::Entity {
        identifier: "minecraft:bee".into(),
    };
    actor.metadata.insert(
        crate::actor_store::VARIANT_METADATA_KEY,
        ActorMetadataValue::Int(12),
    );
    assert_eq!(variant(&actor, 21), 12.0);
}

#[test]
fn pinned_potion_controller_selects_the_projectiles_own_effect_texture() {
    let Some(path) = std::env::var_os("CINNABAR_ENTITY_CARRIER") else {
        eprintln!("missing fixture: CINNABAR_ENTITY_CARRIER potion entity carrier");
        return;
    };
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!(
            "missing fixture: potion entity carrier {}",
            std::path::Path::new(&path).display()
        );
        return;
    };
    let assets = Arc::new(RuntimeEntityAssets::decode(&bytes).unwrap());
    let sprites: Vec<_> = assets
        .item_visuals()
        .iter()
        .filter_map(|visual| {
            if visual.key.identifier.as_ref() != "minecraft:splash_potion" {
                return None;
            }
            let assets::ItemVisualDefinitionRoute::Sprite { texture } = visual.route else {
                panic!("pinned potion must retain its reviewed sprite route");
            };
            Some((i16::try_from(visual.key.metadata).unwrap(), texture.source))
        })
        .collect();
    assert!(!sprites.is_empty(), "pinned potion item routes");
    for (auxiliary, texture_source) in sprites {
        let mut actor = crate::actor_animation::tests::actor_with_metadata(HashMap::from([(
            AUX,
            ActorMetadataValue::Short(auxiliary),
        )]));
        actor.kind = ActorKind::Entity {
            identifier: "minecraft:splash_potion".into(),
        };
        let mut store = ActorAnimationStore::with_assets(Arc::clone(&assets));
        store.insert(1, 0, &actor);
        store.advance_tick(
            &HashMap::from([(actor.runtime_id, actor.clone())]),
            None,
            None,
            true,
            true,
            |_| ActorTickContext {
                main_hand: Some("minecraft:splash_potion".into()),
                main_hand_metadata: 23,
                ..ActorTickContext::default()
            },
        );
        let rig = store.get(actor.runtime_id).expect("pinned potion rig");
        let layer = rig.render.first().expect("pinned potion texture layer");
        let path = &assets.sources()[layer.source as usize].path;
        assert_eq!(
            path,
            &assets.sources()[texture_source as usize].path,
            "auxiliary {auxiliary}"
        );
    }
}
