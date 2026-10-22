use assets::{
    facts::{BlockFacts, BlockProperty, StateValue},
    registry::{BlockTraits, SurvivalRule},
};

/// Loads actual version-pinned states rather than constructing property fixtures.
fn palette() -> Box<[assets::RegistryRecord]> {
    assets::read_registry_for_protocol(
        assets::pinned_block_registry_bytes(),
        assets::active_content_registry_protocol(),
    )
    .unwrap()
}

#[test]
fn every_torch_and_connection_state_has_compiled_traits() {
    let records = palette();
    let facts = BlockFacts::new(&records).unwrap();
    let mut torches = 0;
    let mut connections = 0;
    for record in &records {
        let fact = facts.get(record.sequential_id);
        assert!(fact.readable);
        assert_eq!(fact.traits, record.traits);
        assert_eq!(fact.family, record.model_family);
        assert_eq!(fact.survival, record.survival);
        if record
            .property(BlockProperty::TorchFacingDirection)
            .is_some()
        {
            torches += 1;
            assert!(fact.traits.contains(BlockTraits::TORCH), "{}", record.name);
            assert_eq!(fact.survival, SurvivalRule::Torch);
        }
        if record
            .property(BlockProperty::MinecraftConnectionNorth)
            .is_some()
        {
            connections += 1;
            assert!(
                fact.traits
                    .intersects(BlockTraits::CONNECTS_THIN | BlockTraits::CONNECTS_FENCE),
                "{}",
                record.name
            );
        }
    }
    assert!(torches > 0 && connections > 0);
    assert_eq!(facts.get(u32::MAX).survival, SurvivalRule::Unresolved);
    assert!(!facts.get(u32::MAX).readable);
    assert_eq!(facts.len(), records.len());
}

#[test]
fn placement_counterparts_join_real_block_identities() {
    use assets::facts::PlacementRule as R;
    let records = palette();
    let facts = BlockFacts::new(&records).unwrap();
    let mut slabs = 0;
    let mut signs = 0;
    for record in &records {
        let fact = facts.get(record.sequential_id);
        if matches!(fact.placement, R::Slab | R::StandingSign | R::WallSign) {
            let Some(alternate) = fact.alternate else {
                continue;
            };
            let target = facts.get(alternate);
            assert_ne!(fact.identity, target.identity);
            assert!(matches!(
                (fact.placement, target.placement),
                (R::Slab, R::DoubleSlab)
                    | (R::StandingSign, R::WallSign)
                    | (R::WallSign, R::StandingSign)
            ));
            if fact.placement == R::Slab {
                slabs += 1;
            } else {
                signs += 1;
            }
        }
    }
    assert!(slabs > 0 && signs > 0);
    assert_eq!(facts.get(u32::MAX).placement, R::Unresolved);
    assert!(facts.get(u32::MAX).identity.is_none());
}

#[test]
fn replacement_overrides_and_interaction_gates_share_compiled_traits() {
    let records = palette();
    let facts = BlockFacts::new(&records).unwrap();
    for record in &records {
        let fact = facts.get(record.sequential_id);
        if fact.family == assets::ModelFamily::FlowerBed
            || fact.traits.contains(BlockTraits::LIGHT_BLOCK)
        {
            assert!(
                fact.traits.contains(BlockTraits::REPLACEABLE),
                "{}",
                record.name
            );
        }
        if fact.traits.contains(BlockTraits::SNOW_LAYER) {
            assert_eq!(
                fact.traits.contains(BlockTraits::REPLACEABLE),
                record.property(BlockProperty::Height) == Some(&StateValue::Int(0))
            );
        }
    }
    for (name, trait_, admitted) in [
        ("minecraft:chest", BlockTraits::OPENS_CONTAINER, true),
        ("minecraft:lever", BlockTraits::TOGGLES_BY_HAND, true),
        ("minecraft:bed", BlockTraits::USES_LOCALLY, true),
        (
            "minecraft:iron_trapdoor",
            BlockTraits::TOGGLES_BY_HAND,
            false,
        ),
        ("minecraft:iron_door", BlockTraits::TOGGLES_BY_HAND, false),
    ] {
        let record = records.iter().find(|r| r.name.as_ref() == name).unwrap();
        assert_eq!(
            facts.get(record.sequential_id).traits.contains(trait_),
            admitted,
            "{name}"
        );
    }
}

#[test]
fn typed_lantern_properties_and_copper_bars_are_admitted_end_to_end() {
    let records = palette();
    let facts = BlockFacts::new(&records).unwrap();
    for name in ["minecraft:lantern", "minecraft:copper_lantern"] {
        let states: Vec<_> = records.iter().filter(|r| r.name.as_ref() == name).collect();
        assert!(!states.is_empty());
        assert!(
            states
                .iter()
                .any(|r| r.property(BlockProperty::Hanging) == Some(&StateValue::Bool(true)))
        );
        assert!(
            states
                .iter()
                .any(|r| r.property(BlockProperty::Hanging) == Some(&StateValue::Bool(false)))
        );
        for record in states {
            assert!(
                facts
                    .get(record.sequential_id)
                    .traits
                    .contains(BlockTraits::LANTERN_LIKE)
            );
        }
    }
    let bars: Vec<_> = records
        .iter()
        .filter(|r| r.name.ends_with("copper_bars"))
        .collect();
    assert!(!bars.is_empty());
    for record in bars {
        assert!(
            facts
                .get(record.sequential_id)
                .traits
                .contains(BlockTraits::CONNECTS_THIN)
        );
    }
}

#[test]
fn pinned_property_vocabulary_is_complete_and_domains_match_admitted_values() {
    let vocabulary = include_str!("../../data/vanilla-pack-vocabulary-1.26.50.tsv");
    let names: std::collections::BTreeSet<_> = vocabulary
        .lines()
        .filter_map(|row| row.strip_prefix("property\t"))
        .collect();
    assert_eq!(names.len(), BlockProperty::ALL.len());
    for key in BlockProperty::ALL {
        assert!(names.contains(key.name()));
        assert_eq!(BlockProperty::from_name(key.name()), Some(*key));
    }
    for record in palette() {
        for (key, value) in &record.properties {
            match (key.domain(), value) {
                (assets::facts::PropertyDomain::Bool, StateValue::Bool(_)) => {}
                (assets::facts::PropertyDomain::Int(domain), StateValue::Int(value)) => {
                    assert!(domain.contains(value))
                }
                (assets::facts::PropertyDomain::String(domain), StateValue::String(value)) => {
                    assert!(domain.contains(&value.as_ref()))
                }
                _ => panic!("property domain differs for {}", key.name()),
            }
        }
    }
}

/// A block's generated domain must equal the values its own palette states use.
#[test]
fn per_block_domains_match_each_blocks_palette_values() {
    use assets::facts::PropertyDomain;
    let records = palette();
    let facts = BlockFacts::new(&records).unwrap();
    let mut admitted = std::collections::BTreeMap::<_, Vec<&StateValue>>::new();
    for record in records.iter() {
        for (key, value) in &record.properties {
            assert_eq!(
                facts.get(record.sequential_id).domain(*key),
                assets::facts::block_property_domain(&record.name, *key)
            );
            admitted
                .entry((record.name.as_ref(), *key))
                .or_default()
                .push(value);
        }
    }
    for ((name, key), values) in admitted {
        let expected: Vec<StateValue> = match assets::facts::block_property_domain(name, key) {
            PropertyDomain::Bool => continue,
            PropertyDomain::Int(domain) => domain.iter().map(|v| StateValue::Int(*v)).collect(),
            PropertyDomain::String(domain) => domain
                .iter()
                .map(|v| StateValue::String((*v).into()))
                .collect(),
        };
        assert!(
            expected.iter().all(|v| values.contains(&v))
                && values.iter().all(|v| expected.contains(v)),
            "{name} {} domain differs from its palette values",
            key.name()
        );
    }
}

#[test]
fn compiled_support_and_block_entity_membership_have_one_input_owner() {
    let records = palette();
    let bytes = include_bytes!("../../data/block-physics-v2193.bin");
    let physics = assets::read_physics_registry_for_protocol(
        bytes,
        assets::pinned_block_registry_bytes(),
        &records,
        assets::active_content_registry_protocol(),
    )
    .unwrap();
    for record in &records {
        let shapes = physics
            .by_sequential_id(record.sequential_id)
            .unwrap()
            .shapes
            .as_ref()
            .unwrap();
        assert_eq!(
            record.traits.contains(BlockTraits::VINE_SUPPORT),
            shapes
                .flags
                .contains(assets::block_shapes::ShapeFlags::VINE_SUPPORT)
        );
        assert_eq!(
            record.traits.contains(BlockTraits::FULL_TOP_SUPPORT),
            shapes
                .flags
                .contains(assets::block_shapes::ShapeFlags::FULL_TOP_SUPPORT)
        );
    }
    let invisible = records
        .iter()
        .find(|r| r.name.as_ref() == "minecraft:invisible_bedrock")
        .unwrap();
    assert!(!invisible.traits.contains(BlockTraits::VINE_SUPPORT));
    let entities: serde_json::Value =
        serde_json::from_str(include_str!("../../data/block-entities-v1001.json")).unwrap();
    for entry in entities["entries"].as_array().unwrap() {
        for block in entry["backing_blocks"].as_array().unwrap() {
            let name = block["name"].as_str().unwrap();
            let matched: Vec<_> = records.iter().filter(|r| r.name.as_ref() == name).collect();
            if matched.is_empty() {
                continue;
            }
            assert!(
                matched
                    .iter()
                    .all(|r| r.traits.contains(BlockTraits::BLOCK_ENTITY_BACKING))
            );
        }
    }
}

#[test]
fn dense_facts_reject_an_out_of_order_palette() {
    let mut records = palette();
    records.swap(0, 1);
    assert!(BlockFacts::new(&records).is_err());
}

#[test]
fn pinned_block_components_are_compiled_without_runtime_json() {
    let records = palette();
    for row in include_str!("../../data/block-components-v2193.tsv")
        .lines()
        .skip(1)
    {
        let (name, components) = row.split_once('\t').unwrap();
        let matched: Vec<_> = records.iter().filter(|r| r.name.as_ref() == name).collect();
        if matched.is_empty() {
            let expansion = include_str!("../../data/facts-expansion-v2193.tsv")
                .lines()
                .find(|row| row.split('\t').next() == Some(name))
                .unwrap();
            let fields: Vec<_> = expansion.split('\t').collect();
            assert!(
                fields[3].split(',').any(|status| status == "unadmitted"),
                "{name}"
            );
            if components.split(',').any(|c| c == "minecraft:flammable") {
                assert!(
                    fields[1].split(',').any(|fact| fact == "FIRE_FLAMMABLE"),
                    "{name}"
                );
            }
            continue;
        }
        for record in matched {
            if components.split(',').any(|c| c == "minecraft:flammable") {
                assert!(
                    record.traits.contains(BlockTraits::FIRE_FLAMMABLE),
                    "{name}"
                );
            }
            if components.split(',').any(|c| c == "minecraft:replaceable") {
                assert!(record.traits.contains(BlockTraits::REPLACEABLE), "{name}");
            }
        }
    }
    for name in ["minecraft:furnace", "minecraft:barrel"] {
        assert!(
            records
                .iter()
                .filter(|r| r.name.as_ref() == name)
                .all(|r| !r.traits.contains(BlockTraits::BLOCK_ENTITY_DRAWN))
        );
    }
}
