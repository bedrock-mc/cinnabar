//! Original remote-player packets admitted through wire decoding and world normalization.

use bedrock_protocol_1_26_51::{
    packets::{self, AddPlayer, MobArmorEquipment, MobEquipment, PlayerList, RemoveActor},
    types::{
        ActorRuntimeID, ActorUniqueID, NetworkItemStackDescriptorSerializedData, PlayerListData,
    },
    wire::{self, Decode, Encode},
};

/// Decodes an encoded packet body before applying the production world adapter.
fn event<T: Encode + Decode>(packet: T) -> crate::WorldEvent
where
    packets::Packet: From<T>,
{
    let bytes = packet.encode_to_vec().expect("fixture packet encodes");
    let decoded = T::decode_exact(&bytes).expect("fixture packet decodes");
    crate::into_world_event(crate::Packet::new(decoded), 0)
        .expect("fixture packet normalizes")
        .expect("fixture has a world event")
}

/// Builds a roster entry and spawn with deliberately independent list and spawn identities.
pub fn player(
    runtime_id: u64,
    list_id: i64,
    spawn_id: i64,
    uuid: [u8; 16],
    name: &str,
    feet: [f32; 3],
    skin: &render_api::StandardSkin,
) -> [crate::WorldEvent; 2] {
    let skin_packet = crate::player_skin_packet(uuid, skin, "wide", name, name);
    let packets::Packet::PlayerSkin(skin_packet) = *skin_packet.data else {
        unreachable!()
    };
    let roster = event(PlayerList {
        entries: vec![PlayerListData::AddEntry {
            action: Default::default(),
            uuid: uuid::Uuid::from_bytes(uuid),
            actor_unique_id: ActorUniqueID(list_id),
            player_name: name.into(),
            xbl_xuid: Default::default(),
            platform_online_id: Default::default(),
            build_platform: Default::default(),
            serialized_skin: Box::new(skin_packet.serialized_skin),
            is_teacher: false,
            is_host: false,
            is_sub_client: false,
            player_color: Default::default(),
        }],
    });
    let mut spawn = AddPlayer {
        uuid: uuid::Uuid::from_bytes(uuid),
        player_name: name.into(),
        target_runtime_id: ActorRuntimeID(runtime_id),
        position: feet.into(),
        ..Default::default()
    };
    spawn.abilities_data.target_player_raw_id = wire::I64LE(spawn_id);
    [roster, event(spawn)]
}

/// Encodes an ordinary stack or the canonical empty descriptor.
fn stack(network_id: i16) -> NetworkItemStackDescriptorSerializedData {
    NetworkItemStackDescriptorSerializedData {
        id: wire::I16LE(network_id),
        stack_size: wire::U16LE(u16::from(network_id != 0)),
        ..Default::default()
    }
}

/// Admits all four worn slots and the two hands through their actual packet adapters.
pub fn equipment(runtime_id: u64, armor: [i16; 4], hands: [i16; 2]) -> [crate::WorldEvent; 3] {
    let [head, torso, legs, feet] = armor.map(stack);
    let armor = event(MobArmorEquipment {
        target_runtime_id: ActorRuntimeID(runtime_id),
        head,
        torso,
        legs,
        feet,
        ..Default::default()
    });
    let [main, off] = std::array::from_fn(|index| {
        event(MobEquipment {
            target_runtime_id: ActorRuntimeID(runtime_id),
            item: stack(hands[index]),
            container_id: wire::U8(if index == 0 { 0 } else { 119 }),
            ..Default::default()
        })
    });
    [armor, main, off]
}

/// Removes a spawned lifetime through the wire's unique actor identity.
pub fn remove(spawn_id: i64) -> crate::WorldEvent {
    event(RemoveActor {
        target_actor_id: ActorUniqueID(spawn_id),
    })
}

/// Admits a component item with its server-declared wearable slot through registry decoding.
pub fn wearable_registry(
    identifier: &str,
    network_id: i16,
    slot: &str,
) -> crate::ItemRegistryEvent {
    let mut bytes = Vec::new();
    let mut writer = wire::Writer::new(&mut bytes);
    for (tag, name) in [
        (10, ""),
        (10, "components"),
        (10, "minecraft:wearable"),
        (8, "slot"),
    ] {
        writer.write_u8(tag).unwrap();
        writer.write_len_var_u32(name.len()).unwrap();
        writer.write_all(name.as_bytes()).unwrap();
    }
    writer.write_len_var_u32(slot.len()).unwrap();
    writer.write_all(slot.as_bytes()).unwrap();
    writer.write_all(&[0, 0, 0]).unwrap();
    super::items::component_item_registry_payload(identifier, network_id, bytes)
}
