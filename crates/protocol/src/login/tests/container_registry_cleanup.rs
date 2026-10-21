use super::*;
use crate::{ContainerIdentity, InventoryEvent};

/// Builds cleanup bytes independently of the generated packet encoder.
fn cleanup_body(containers: &[(u8, Option<u32>)]) -> Vec<u8> {
    let mut body = Vec::new();
    let mut writer = wire::Writer::new(&mut body);
    writer.write_var_u32(containers.len() as u32).unwrap();
    for (name, dynamic_id) in containers {
        writer.write_u8(*name).unwrap();
        writer.write_u8(u8::from(dynamic_id.is_some())).unwrap();
        if let Some(id) = dynamic_id {
            writer.write_all(&id.to_le_bytes()).unwrap();
        }
    }
    body
}

#[test]
fn container_registry_cleanup_preserves_full_names_and_optional_ids() {
    let containers = [
        (crate::CONTAINER_NAME_DYNAMIC, Some(7)),
        (crate::CONTAINER_NAME_DYNAMIC, Some(0)),
        (crate::CONTAINER_NAME_DYNAMIC, None),
        (u8::MAX, Some(u32::MAX)),
        (crate::CONTAINER_NAME_INVENTORY, Some(7)),
    ];
    let raw = raw_packet(
        PacketId::ContainerRegistryCleanup,
        &cleanup_body(&containers),
    );
    let event = decode_world_raw_with(raw, 0, super::super::decode_test_raw)
        .unwrap()
        .expect("cleanup enters inventory ingress");
    let WorldEvent::Inventory(InventoryEvent::RegistryCleanup(removed)) = event else {
        panic!("one cleanup packet remains one inventory event");
    };
    assert_eq!(
        removed.as_ref(),
        containers.map(|(name, dynamic_id)| ContainerIdentity {
            window_id: None,
            slot_type: Some(name),
            dynamic_id,
        })
    );
}

#[test]
fn container_registry_cleanup_empty_list_is_admitted() {
    let raw = raw_packet(PacketId::ContainerRegistryCleanup, &[0]);
    assert!(matches!(
        decode_world_raw_with(raw, 0, super::super::decode_test_raw).unwrap(),
        Some(WorldEvent::Inventory(InventoryEvent::RegistryCleanup(removed))) if removed.is_empty()
    ));
}

#[test]
fn container_registry_cleanup_rejects_truncation_and_trailing_bytes() {
    let body = cleanup_body(&[(crate::CONTAINER_NAME_DYNAMIC, Some(7))]);
    for length in 0..body.len() {
        let raw = raw_packet(PacketId::ContainerRegistryCleanup, &body[..length]);
        let error = decode_world_raw_with(raw, 0, super::super::decode_test_raw)
            .expect_err("incomplete cleanup is a wire error");
        assert!(super::super::skip_semantic_world_error(error, &mut 0).is_err());
    }
    let mut trailing = body;
    trailing.push(0);
    let raw = raw_packet(PacketId::ContainerRegistryCleanup, &trailing);
    let error = decode_world_raw_with(raw, 0, super::super::decode_test_raw)
        .expect_err("cleanup must consume its whole payload");
    assert!(super::super::skip_semantic_world_error(error, &mut 0).is_err());
}
