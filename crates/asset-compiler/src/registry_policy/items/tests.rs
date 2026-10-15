use super::*;

/// Locates the pinned compiler inputs without requiring a fetched pack.
fn root() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Copies only the real pinned inputs for source-validation regressions.
fn inputs() -> tempfile::TempDir {
    let scratch = tempfile::tempdir().unwrap();
    for input in INPUTS {
        let dest = scratch.path().join(input);
        std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
        std::fs::copy(root().join(input), dest).unwrap();
    }
    scratch
}

#[test]
fn every_retail_item_round_trips_with_its_existing_pinned_facts() {
    let (payload, _) = compile(&root()).unwrap();
    let bytes = encode_item_facts(payload.clone()).unwrap();
    assert_eq!(bytes, encode_item_facts(payload).unwrap());
    let facts = ItemFacts::decode(&bytes).unwrap();
    for record in &facts.payload().records {
        let name = record.identifier.as_ref();
        let fact = facts.get(record.network_id).unwrap();
        assert_eq!(facts.by_identifier(name), Some(fact));
        assert_eq!(fact.armour, assets::item::armour::builtin_item_armour(name));
        assert_eq!(
            fact.durability,
            assets::item::durability::builtin_item_durability(name)
        );
        assert_eq!(fact.grip, assets::item::grip::builtin_item_grip(name));
        assert_eq!(fact.glint, assets::item::glint::builtin_item_glint(name));
        assert_eq!(fact.fuel, assets::item::fuel::builtin_item_fuel(name));
        let profile = fact
            .use_profile
            .and_then(|index| facts.use_profile(index))
            .cloned()
            .unwrap_or_default();
        assert_eq!(profile, assets::item_use::diagnostic_profile(name));
    }
    assert!(facts.get(i32::MIN).is_none());
    assert!(facts.get(i32::MAX).is_none());
    let gap = facts
        .payload()
        .records
        .windows(2)
        .find(|pair| pair[1].network_id > pair[0].network_id + 1)
        .unwrap();
    assert!(facts.get(gap[0].network_id + 1).is_none());
}

#[test]
fn pinned_tags_cover_armour_smithing_tools_and_the_real_banner() {
    let (payload, _) = compile(&root()).unwrap();
    let facts = ItemFacts::from_payload(payload).unwrap();
    let armour = facts.tag("minecraft:is_armor").unwrap();
    for record in &facts.payload().records {
        if record.fact.tags.contains(armour) {
            assert!(record.fact.armour_slot.is_some(), "{}", record.identifier);
        }
        for (name, role) in [
            (
                "minecraft:trim_materials",
                ContainerRoles::SMITHING_MATERIAL,
            ),
            (
                "minecraft:transform_materials",
                ContainerRoles::SMITHING_MATERIAL,
            ),
            (
                "minecraft:trim_templates",
                ContainerRoles::SMITHING_TEMPLATE,
            ),
            (
                "minecraft:transform_templates",
                ContainerRoles::SMITHING_TEMPLATE,
            ),
            ("minecraft:trimmable_armors", ContainerRoles::SMITHING_INPUT),
            (
                "minecraft:transformable_items",
                ContainerRoles::SMITHING_INPUT,
            ),
        ] {
            if record.fact.tags.contains(facts.tag(name).unwrap()) {
                assert!(record.fact.roles.contains(role));
            }
        }
    }
    assert!(
        facts
            .by_identifier("minecraft:banner")
            .unwrap()
            .roles
            .contains(ContainerRoles::LOOM_BANNER)
    );
    assert!(
        facts
            .by_identifier("minecraft:carved_pumpkin")
            .unwrap()
            .traits
            .contains(ItemTraits::COSMETIC_HEAD)
    );
    let sword = facts.by_identifier("minecraft:copper_sword").unwrap();
    assert!(sword.traits.contains(ItemTraits::SWORD));
    assert_eq!(sword.tier, Some(ItemTier::Copper));
    assert!(
        facts
            .by_identifier("minecraft:mace")
            .unwrap()
            .traits
            .contains(ItemTraits::CREATIVE_NO_BREAK)
    );
}

#[test]
fn sealed_carrier_rejects_truncation_corruption_and_forged_fields() {
    let (payload, _) = compile(&root()).unwrap();
    let bytes = encode_item_facts(payload.clone()).unwrap();
    for end in [0, 7, bytes.len() - 1] {
        assert!(ItemFacts::decode(&bytes[..end]).is_err());
    }
    let mut corrupt = bytes;
    corrupt[20] ^= 1;
    assert!(ItemFacts::decode(&corrupt).is_err());
    let mut invalid = payload.clone();
    invalid.records[0].fact.tags.0 = u128::MAX;
    assert!(encode_item_facts(invalid).is_err());
    let mut invalid = payload.clone();
    invalid.records[0].fact.use_profile = Some(u16::MAX);
    assert!(encode_item_facts(invalid).is_err());
    let mut invalid = payload;
    invalid.records[0].network_id += 1;
    assert!(encode_item_facts(invalid).is_err());
}

#[test]
fn source_rows_and_rules_fail_closed_when_the_registry_no_longer_matches() {
    let scratch = inputs();
    let path = scratch.path().join(INPUTS[1]);
    let capacity = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        capacity.lines().skip(1).collect::<Vec<_>>().join("\n"),
    )
    .unwrap();
    assert!(compile(scratch.path()).is_err());
    std::fs::write(&path, capacity).unwrap();
    let path = scratch.path().join(INPUTS[9]);
    let mut policy: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    policy["item_rules"][0]["selector"] = serde_json::json!({"suffix": "_sword_removed"});
    std::fs::write(path, serde_json::to_vec(&policy).unwrap()).unwrap();
    assert!(compile(scratch.path()).is_err());
}

#[test]
fn committed_item_carrier_and_expansion_regenerate_byte_for_byte() {
    let root = root();
    let (payload, expansion) = compile(&root).unwrap();
    assert_eq!(
        encode_item_facts(payload).unwrap(),
        std::fs::read(root.join(ITEM_FACTS_SOURCE_PATH)).unwrap()
    );
    assert_eq!(
        expansion,
        std::fs::read_to_string(root.join(ITEM_FACTS_EXPANSION_PATH)).unwrap()
    );
}

#[test]
fn item_facts_source_path_and_preparation_output_agree() {
    assert_eq!(
        Path::new(ITEM_FACTS_SOURCE_PATH).file_name().unwrap(),
        ITEM_FACTS_FILENAME
    );
    assert_eq!(assets::carriers::ITEM_FACTS.output, ITEM_FACTS_FILENAME);
}

#[test]
fn beacon_roles_and_renderer_keys_have_one_complete_validated_source() {
    let (payload, _) = compile(&root()).unwrap();
    let facts = ItemFacts::from_payload(payload.clone()).unwrap();
    let payments: Vec<_> = facts
        .payload()
        .records
        .iter()
        .filter(|record| record.fact.roles.contains(ContainerRoles::BEACON_PAYMENT))
        .collect();
    assert!(!payments.is_empty());
    assert!(
        payments
            .iter()
            .all(|record| record.fact.beacon_payment_key.is_some())
    );
    assert!(
        facts
            .by_identifier(TOTEM_IDENTIFIER)
            .unwrap()
            .allow_off_hand
            == Some(true)
    );
    let index = payload
        .records
        .iter()
        .position(|record| record.fact.roles.contains(ContainerRoles::BEACON_PAYMENT))
        .unwrap();
    let mut incomplete = payload.clone();
    incomplete.records[index].fact.beacon_payment_key = None;
    assert!(ItemFacts::from_payload(incomplete).is_err());
    let mut extra = payload;
    let non_payment = extra
        .records
        .iter_mut()
        .find(|record| !record.fact.roles.contains(ContainerRoles::BEACON_PAYMENT))
        .unwrap();
    non_payment.fact.beacon_payment_key = payments[0].fact.beacon_payment_key;
    assert!(ItemFacts::from_payload(extra).is_err());
}
