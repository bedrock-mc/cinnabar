use std::sync::Arc;

use valentine::bedrock::version::v1_26_51::{
    ActorEventPacket, AddItemActorPacket, EnumsActorEvent, TakeItemActorPacket,
};

use super::{ITEM_ACTOR_NETWORK_OFFSET, normalize_metadata, validate_finite};
use crate::{ActorEvent, ActorKind, ActorPacketError, ActorSpawnEvent, item::normalize_item};

/// Server-announced actor events the client visualises; ids with no client-side visual are dropped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActorStatusKind {
    Hurt,
    /// Event 81: hurt animation and sound without the red damage flash.
    HurtWithoutDamage,
    Death,
    /// Failed taming: smoke particles.
    TamingFailed,
    /// Successful taming: heart particles.
    TamingSucceeded,
    /// Native event 39 assigns the signed payload to the actor's shake countdown.
    Shake,
    ShakeWetness,
    EatGrass,
    LoveHearts,
    VillagerAngry,
    VillagerHappy,
    WitchHatMagic,
    FireworksExplode,
    DrinkPotion,
    ThrowPotion,
    PrimeTntMinecart,
    PrimeCreeper,
    TotemActivate,
    SpawnAlive,
    LeashDestroyed,
    ZombieConverting,
    Puke,
    DrinkMilk,
    Feed,
    ActorGrowUp,
}

/// One actor status event, addressed by runtime id (the wire carries no dimension).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorStatusEvent {
    pub runtime_id: u64,
    pub kind: ActorStatusKind,
    /// Event-specific payload; its meaning depends on `kind` and is unused for most kinds.
    pub data: i32,
}

/// A dropped item was picked up: the item flies to `collector_runtime_id` and is then removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActorTakeItemEvent {
    pub item_runtime_id: u64,
    pub collector_runtime_id: u64,
}

pub(crate) fn normalize_take_item_actor(packet: TakeItemActorPacket) -> ActorEvent {
    ActorEvent::TakeItem(ActorTakeItemEvent {
        item_runtime_id: packet.item_runtime_id.actor_runtime_id,
        collector_runtime_id: packet.actor_runtime_id.actor_runtime_id,
    })
}

/// Dropped-item spawn; the stack rides in `held_item` and the identifier is fixed.
pub(crate) fn normalize_add_item_actor(
    packet: AddItemActorPacket,
    dimension: i32,
) -> Result<ActorEvent, ActorPacketError> {
    for (field, value) in [
        ("position.x", packet.position.x),
        ("position.y", packet.position.y),
        ("position.z", packet.position.z),
        ("velocity.x", packet.velocity.x),
        ("velocity.y", packet.velocity.y),
        ("velocity.z", packet.velocity.z),
    ] {
        validate_finite(field, value)?;
    }
    let held_item = normalize_item(packet.item)?;
    let metadata = normalize_metadata(packet.entity_data)?;
    Ok(ActorEvent::Spawn(ActorSpawnEvent {
        dimension,
        unique_id: packet.target_actor_id.actor_unique_id,
        runtime_id: packet.target_runtime_id.actor_runtime_id,
        kind: ActorKind::Entity {
            identifier: Arc::from("minecraft:item"),
        },
        // The vanilla constructor and handler pass StateVector origin
        // unchanged. Our store retains feet; dropped rendering restores it explicitly.
        position: [
            packet.position.x,
            packet.position.y - ITEM_ACTOR_NETWORK_OFFSET,
            packet.position.z,
        ],
        velocity: [packet.velocity.x, packet.velocity.y, packet.velocity.z],
        pitch: 0.0,
        yaw: 0.0,
        head_yaw: 0.0,
        body_yaw: 0.0,
        held_item,
        metadata,
        attributes: Arc::from([]),
        properties: Arc::from([]),
        links: Arc::from([]),
    }))
}

/// Maps an ActorEvent packet to a status event, or `None` for ids the client draws nothing for.
pub(crate) fn normalize_actor_event(packet: ActorEventPacket) -> Option<ActorEvent> {
    let kind = match packet.event_id {
        EnumsActorEvent::Hurt => ActorStatusKind::Hurt,
        EnumsActorEvent::HurtWithoutReceivingDamage => ActorStatusKind::HurtWithoutDamage,
        EnumsActorEvent::Death | EnumsActorEvent::InstantDeath => ActorStatusKind::Death,
        EnumsActorEvent::TamingFailed => ActorStatusKind::TamingFailed,
        EnumsActorEvent::TamingSucceeded => ActorStatusKind::TamingSucceeded,
        EnumsActorEvent::Shake => ActorStatusKind::Shake,
        EnumsActorEvent::ShakeWetness => ActorStatusKind::ShakeWetness,
        EnumsActorEvent::EatGrass => ActorStatusKind::EatGrass,
        EnumsActorEvent::LoveHearts | EnumsActorEvent::InLoveHearts => ActorStatusKind::LoveHearts,
        EnumsActorEvent::VillagerAngry => ActorStatusKind::VillagerAngry,
        EnumsActorEvent::VillagerHappy => ActorStatusKind::VillagerHappy,
        EnumsActorEvent::WitchHatMagic => ActorStatusKind::WitchHatMagic,
        EnumsActorEvent::FireworksExplode => ActorStatusKind::FireworksExplode,
        EnumsActorEvent::DrinkPotion => ActorStatusKind::DrinkPotion,
        EnumsActorEvent::ThrowPotion => ActorStatusKind::ThrowPotion,
        EnumsActorEvent::PrimeTntcart => ActorStatusKind::PrimeTntMinecart,
        EnumsActorEvent::PrimeCreeper => ActorStatusKind::PrimeCreeper,
        EnumsActorEvent::TalismanActivate => ActorStatusKind::TotemActivate,
        EnumsActorEvent::SpawnAlive => ActorStatusKind::SpawnAlive,
        EnumsActorEvent::LeashDestroyed => ActorStatusKind::LeashDestroyed,
        EnumsActorEvent::ZombieConverting => ActorStatusKind::ZombieConverting,
        EnumsActorEvent::Puke => ActorStatusKind::Puke,
        EnumsActorEvent::DrinkMilk => ActorStatusKind::DrinkMilk,
        EnumsActorEvent::Feed => ActorStatusKind::Feed,
        EnumsActorEvent::ActorGrowUp => ActorStatusKind::ActorGrowUp,
        _ => return None,
    };
    Some(ActorEvent::Status(ActorStatusEvent {
        runtime_id: packet.target_runtime_id.actor_runtime_id,
        kind,
        data: packet.data,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::BytesMut;
    use valentine::bedrock::codec::BedrockCodec;

    fn packet(event_id: EnumsActorEvent) -> ActorEventPacket {
        let mut packet = ActorEventPacket::default();
        packet.target_runtime_id.actor_runtime_id = 9;
        packet.event_id = event_id;
        packet.data = 4;
        packet
    }

    #[test]
    fn hurt_and_death_ids_normalize_by_runtime_id() {
        assert_eq!(
            normalize_actor_event(packet(EnumsActorEvent::Hurt)),
            Some(ActorEvent::Status(ActorStatusEvent {
                runtime_id: 9,
                kind: ActorStatusKind::Hurt,
                data: 4,
            }))
        );
        assert!(matches!(
            normalize_actor_event(packet(EnumsActorEvent::HurtWithoutReceivingDamage)),
            Some(ActorEvent::Status(ActorStatusEvent {
                kind: ActorStatusKind::HurtWithoutDamage,
                ..
            }))
        ));
        assert!(matches!(
            normalize_actor_event(packet(EnumsActorEvent::InstantDeath)),
            Some(ActorEvent::Status(ActorStatusEvent {
                kind: ActorStatusKind::Death,
                ..
            }))
        ));
    }

    #[test]
    fn unknown_and_invisible_ids_are_skipped() {
        assert_eq!(
            normalize_actor_event(packet(EnumsActorEvent::Unknown(200))),
            None
        );
        assert_eq!(normalize_actor_event(packet(EnumsActorEvent::Jump)), None);
    }

    #[test]
    fn shake_preserves_the_server_signed_countdown_without_a_default_duration() {
        for data in [i32::MIN, -1, 0, 1, 12, i32::MAX] {
            let mut packet = packet(EnumsActorEvent::Shake);
            packet.data = data;
            assert_eq!(
                normalize_actor_event(packet),
                Some(ActorEvent::Status(ActorStatusEvent {
                    runtime_id: 9,
                    kind: ActorStatusKind::Shake,
                    data,
                }))
            );
        }
    }

    #[test]
    fn shake_wire_roundtrips_signed_data_and_rejects_every_truncated_body() {
        for data in [i32::MIN, -1, 0, 12, i32::MAX] {
            let mut packet = packet(EnumsActorEvent::Shake);
            packet.data = data;
            let mut encoded = BytesMut::new();
            packet.encode(&mut encoded).unwrap();
            let encoded = encoded.freeze();
            let decoded = ActorEventPacket::decode(&mut encoded.clone(), ()).unwrap();
            assert_eq!(
                normalize_actor_event(decoded),
                normalize_actor_event(packet)
            );
            for end in 0..encoded.len() {
                assert!(ActorEventPacket::decode(&mut encoded.slice(..end), ()).is_err());
            }
        }
    }

    #[test]
    fn add_item_actor_spawns_a_fixed_identifier_item_entity() {
        let mut packet = valentine::bedrock::version::v1_26_51::AddItemActorPacket::default();
        packet.target_actor_id.actor_unique_id = -5;
        packet.target_runtime_id.actor_runtime_id = 12;
        packet.position.y = 64.0 + ITEM_ACTOR_NETWORK_OFFSET;
        let Ok(ActorEvent::Spawn(spawn)) = normalize_add_item_actor(packet, 0) else {
            panic!("item actor spawn");
        };
        assert_eq!((spawn.unique_id, spawn.runtime_id), (-5, 12));
        assert!(matches!(
            &spawn.kind,
            crate::ActorKind::Entity { identifier } if identifier.as_ref() == "minecraft:item"
        ));
        assert_eq!(spawn.position[1], 64.0);
    }

    #[test]
    fn add_item_wire_origin_is_normalized_to_collision_feet_once() {
        let mut packet = AddItemActorPacket::default();
        packet.position.x = 1.0;
        packet.position.y = 64.0 + ITEM_ACTOR_NETWORK_OFFSET;
        packet.position.z = 3.0;
        let mut bytes = BytesMut::new();
        packet.encode(&mut bytes).unwrap();
        let packet = AddItemActorPacket::decode(&mut bytes.freeze(), ()).unwrap();
        let ActorEvent::Spawn(spawn) = normalize_add_item_actor(packet, 0).unwrap() else {
            panic!("expected item spawn");
        };
        assert_eq!(spawn.position, [1.0, 64.0, 3.0]);
    }

    #[test]
    fn take_item_names_item_and_collector() {
        let mut packet = TakeItemActorPacket::default();
        packet.item_runtime_id.actor_runtime_id = 3;
        packet.actor_runtime_id.actor_runtime_id = 4;
        assert_eq!(
            normalize_take_item_actor(packet),
            ActorEvent::TakeItem(ActorTakeItemEvent {
                item_runtime_id: 3,
                collector_runtime_id: 4,
            })
        );
    }
}
