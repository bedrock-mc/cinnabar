use super::*;

/// Builds a minimal original binding for catalog validation.
fn binding(identifier: &str) -> EquipmentBinding {
    let reference = |identifier: &str| EquipmentReference {
        identifier: identifier.into(),
        resolution: EntityDependencyResolution::Catalog,
    };
    EquipmentBinding {
        identifier: identifier.into(),
        attachable: None,
        category: EquipmentCategory::Held,
        geometry: reference("geometry.a"),
        texture: reference("textures/entity/a"),
        resources: Default::default(),
        material: "entity".into(),
        color_mask: EquipmentColorMask::NoMask,
        render_controller: "controller.render.a".into(),
        first_person: EquipmentTransform::NeedsMeasurement,
        third_person: EquipmentTransform::NeedsMeasurement,
        dropped: EquipmentTransform::NeedsMeasurement,
        poses: Box::new([]),
    }
}

// Bindings sort and dedupe by identifier; a zero identity is refused.
#[test]
fn from_parts_sorts_dedupes_and_requires_an_identity() {
    let catalog = RuntimeEquipmentCatalog::from_parts(
        [1; 32],
        vec![binding("b:item"), binding("a:item"), binding("b:item")],
        Vec::new(),
    )
    .unwrap();
    let identifiers = catalog
        .bindings()
        .iter()
        .map(|binding| binding.identifier.as_ref())
        .collect::<Vec<_>>();
    assert_eq!(identifiers, ["a:item", "b:item"]);
    assert!(catalog.binding("b:item").is_some());
    assert!(
        RuntimeEquipmentCatalog::from_parts([0; 32], vec![binding("a:item")], Vec::new()).is_err()
    );
}

#[test]
fn owner_specific_definitions_survive_catalog_round_trips() {
    let mut player = binding("test:armour");
    player.attachable = Some("test:armour.player".into());
    let mut stand = player.clone();
    stand.attachable = Some("test:armour.stand".into());
    let catalog =
        RuntimeEquipmentCatalog::from_parts([1; 32], vec![stand, player], Vec::new()).unwrap();
    let bytes = encode_equipment_catalog_with_textures(
        catalog.source_manifest_sha256(),
        catalog.entity_blob_sha256(),
        catalog.bindings(),
        catalog.textures(),
    )
    .unwrap();
    let restored = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
    assert_eq!(restored.bindings_for("test:armour").len(), 2);
    assert_eq!(restored.bindings(), catalog.bindings());
    assert!(restored.bindings_for("test:missing").is_empty());
}

#[test]
fn compiled_mask_capability_applies_dye_without_inspecting_the_material_name() {
    let red = Some(0x00b0_2e26);
    let worn = |material: &str, color_mask| EquipmentBinding {
        material: material.into(),
        color_mask,
        ..binding("custom:chestplate")
    };
    assert_eq!(
        worn("renamed", EquipmentColorMask::Dye).color_mask_rgb(red),
        red
    );
    assert_eq!(
        worn("renamed", EquipmentColorMask::Dye).color_mask_rgb(None),
        Some(DEFAULT_LEATHER_RGB)
    );
    assert_eq!(
        worn("leather_decoy", EquipmentColorMask::NoMask).color_mask_rgb(red),
        None
    );
    assert_eq!(
        worn("unknown", EquipmentColorMask::Unresolved).color_mask_rgb(red),
        None
    );
}

/// Builds a version-five carrier with the binding encoding used before resource aliases.
fn carrier_without_resource_aliases() -> Vec<u8> {
    let source = serde_json::to_string(&[1u8; 32]).unwrap();
    let entity = serde_json::to_string(&[2u8; 32]).unwrap();
    let binding = concat!(
        r#"{"identifier":"minecraft:diamond_helmet","category":{"kind":"armor","slot":"helmet"},"#,
        r#""geometry":{"identifier":"geometry.helmet","resolution":"Catalog"},"#,
        r#""texture":{"identifier":"textures/helmet","resolution":"Catalog"},"#,
        r#""material":"armor","color_mask":"no_mask","render_controller":"controller.render.armor","#,
        r#""first_person":{"source":"needs_measurement"},"#,
        r#""third_person":{"source":"needs_measurement"},"#,
        r#""dropped":{"source":"needs_measurement"},"poses":[]}"#,
    );
    let payload = format!(
        r#"{{"item_use_overlays":[],"source_manifest_sha256":{source},"entity_blob_sha256":{entity},"bindings":[{binding}],"item_use":[]}}"#
    );
    let mut bytes = EQUIPMENT_CARRIER_MAGIC.to_vec();
    bytes.extend_from_slice(&5u32.to_le_bytes());
    bytes.extend_from_slice(&(payload.len() as u64).to_le_bytes());
    bytes.extend_from_slice(payload.as_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    let hash = Sha256::digest(&bytes);
    bytes.extend_from_slice(&hash);
    bytes
}

#[test]
fn version_five_carrier_without_resources_decodes_and_keeps_identical_bytes() {
    let bytes = carrier_without_resource_aliases();
    let catalog = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
    let helmet = catalog.binding("minecraft:diamond_helmet").unwrap();
    assert_eq!(
        helmet.category,
        EquipmentCategory::Armor {
            slot: ArmorSlot::Helmet
        }
    );
    assert!(helmet.resources.is_empty());
    let encoded = encode_equipment_catalog(
        catalog.source_manifest_sha256(),
        catalog.entity_blob_sha256(),
        catalog.bindings(),
    )
    .unwrap();
    assert_eq!(encoded, bytes);
}

#[test]
fn nonempty_resource_aliases_round_trip_even_when_only_one_table_is_populated() {
    for (geometry, textures) in [(true, false), (false, true), (true, true)] {
        let mut binding = binding("minecraft:bow");
        if geometry {
            binding
                .resources
                .geometry
                .insert("default".into(), "geometry.a".into());
        }
        if textures {
            binding
                .resources
                .textures
                .insert("default".into(), "textures/entity/a".into());
        }
        let bytes = encode_equipment_catalog([1; 32], [2; 32], &[binding.clone()]).unwrap();
        let catalog = RuntimeEquipmentCatalog::decode(&bytes).unwrap();
        assert_eq!(catalog.binding("minecraft:bow"), Some(&binding));
    }
}
