use super::*;

#[test]
fn generation_changes_only_when_catalog_content_changes() {
    let asset = Asset {
        url: "fixture".into(),
        sha256: "a".repeat(64),
        bytes: 1,
    };
    let descriptor = Descriptor {
        id: "fixture:generation".into(),
        slot: "head".into(),
        display_name: "Fixture".into(),
        tags: Vec::new(),
        free: true,
        assets: Assets {
            geometry: asset.clone(),
            texture: asset,
            animation: None,
        },
        geometry_id: "geometry.fixture".into(),
        attachment_bone: "head".into(),
        offset: [0.0; 3],
        render_flags: RenderFlags {
            double_sided: false,
            emissive: false,
        },
    };
    let before = Item::generation();
    let item = Item::admit(descriptor.clone()).unwrap();
    let admitted = Item::generation();
    assert_ne!(before, admitted);
    assert_eq!(Item::admit(descriptor), Some(item));
    assert_eq!(Item::generation(), admitted);
    activate(&[item.id().into()]);
    let active = Item::generation();
    assert_ne!(admitted, active);
    activate(&[item.id().into()]);
    assert_eq!(Item::generation(), active);
    activate(&[]);
    assert_ne!(Item::generation(), active);
    assert!(Item::all().is_empty());
}
#[test]
fn cache_admission_checks_size_and_digest() {
    let bytes = b"original";
    let mut asset = Asset {
        url: String::new(),
        sha256: format!("{:x}", Sha256::digest(bytes)),
        bytes: bytes.len(),
    };
    assert!(asset.verifies(bytes));
    assert!(!asset.verifies(b"changed!"));
    asset.bytes += 1;
    assert!(!asset.verifies(bytes));
}
#[test]
fn slots_come_from_the_wire_schema() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../core/cinnabarapi/schema/v1.json")).unwrap();
    assert_eq!(
        schema["$defs"]["CosmeticSlot"]["x-known-slots"]
            .as_array()
            .unwrap()
            .len(),
        SLOT_COUNT
    );
    assert_eq!(
        schema["$defs"]["CosmeticsCatalog"]["properties"]["items"]["maxItems"],
        MAX_ITEMS
    );
    assert_eq!(
        schema["$defs"]["CosmeticAsset"]["properties"]["bytes"]["maximum"],
        MAX_ASSET_BYTES
    );
    assert!(known_slot("head"));
    assert!(!known_slot("future:slot"));
    assert_eq!(known_slots().len(), SLOT_COUNT);
    for (index, slot) in known_slots().iter().enumerate() {
        assert_eq!(slot_index(slot), Some(index));
    }
}
#[test]
fn emissive_flag_is_optional_and_omitted_when_disabled() {
    let old: RenderFlags = serde_json::from_str(r#"{"double_sided":true}"#).unwrap();
    assert!(!old.emissive);
    assert_eq!(
        serde_json::to_string(&old).unwrap(),
        r#"{"double_sided":true}"#
    );
    let glowing: RenderFlags =
        serde_json::from_str(r#"{"double_sided":false,"emissive":true}"#).unwrap();
    assert!(glowing.emissive);
    assert_eq!(
        serde_json::from_str::<RenderFlags>(&serde_json::to_string(&glowing).unwrap()).unwrap(),
        glowing
    );
}
