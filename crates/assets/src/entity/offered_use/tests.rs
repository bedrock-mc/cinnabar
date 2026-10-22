use super::*;

/// Creates an original behavior descriptor without assigning component-group activation.
fn descriptor(condition: UseCondition, selector: UseSelector) -> EntityUseAdmission {
    EntityUseAdmission::Components {
        source: 0,
        offerings: Box::from([ComponentUse {
            selector,
            component: "fixture:offering".into(),
            condition,
        }]),
    }
}

#[test]
fn an_excluded_item_rules_out_an_unresolved_group_without_activating_it() {
    let definition = descriptor(
        UseCondition::HeldItem {
            identifier: "fixture:food".into(),
            metadata: None,
        },
        UseSelector::UnresolvedComponentGroup { group: 0 },
    );
    assert_eq!(
        definition.for_throwable(Some("fixture:projectile"), 0),
        OfferedUse::Absent
    );
    assert_eq!(
        definition.for_throwable(Some("fixture:food"), 0),
        OfferedUse::Unresolved
    );
    assert_eq!(definition.for_throwable(None, 0), OfferedUse::Unresolved);
    assert_eq!(
        EntityUseAdmission::Unresolved.for_throwable(Some("fixture:projectile"), 0),
        OfferedUse::Unresolved
    );
}

#[test]
fn unknown_filters_preserve_possible_offerings_but_cannot_override_a_false_conjunct() {
    let held = UseCondition::HeldItem {
        identifier: "fixture:feed".into(),
        metadata: Some(2),
    };
    let definition = descriptor(
        UseCondition::All(vec![UseCondition::Unresolved, held].into()),
        UseSelector::Base,
    );
    assert_eq!(
        definition.for_throwable(Some("fixture:feed"), 2),
        OfferedUse::Unresolved
    );
    assert_eq!(
        definition.for_throwable(Some("fixture:feed"), 0),
        OfferedUse::Absent
    );
    let alternatives = descriptor(
        UseCondition::Any(vec![UseCondition::Never, UseCondition::Unresolved].into()),
        UseSelector::Base,
    );
    assert_eq!(
        alternatives.for_throwable(Some("fixture:projectile"), 0),
        OfferedUse::Unresolved
    );
}

#[test]
fn offered_use_carrier_retains_conditions_and_rejects_a_resource_source() {
    let mut compiled =
        crate::carriers::startup::test_carriers::synthetic_interaction_assets("minecraft:zombie");
    let mut sources = compiled.sources.into_vec();
    sources.insert(
        0,
        super::super::EntityAssetSource {
            path: "behavior/entities/fixture.json".into(),
            source_bytes: 1,
            source_sha256: [3; 32],
        },
    );
    compiled.symbols[0].source_index += 1;
    compiled.sources = sources.into();
    compiled
        .interaction_traits
        .iter_mut()
        .find(|entry| entry.identifier.as_ref() == "minecraft:zombie")
        .unwrap()
        .offered_use = EntityUseAdmission::Components {
        source: 0,
        offerings: Box::from([ComponentUse {
            selector: UseSelector::UnresolvedComponentGroup { group: 0 },
            component: "minecraft:rideable".into(),
            condition: UseCondition::Never,
        }]),
    };
    let runtime =
        super::super::RuntimeEntityAssets::decode(&crate::encode_entity_blob(&compiled).unwrap())
            .unwrap();
    assert_eq!(
        runtime
            .offered_use_for("minecraft:zombie")
            .unwrap()
            .for_throwable(Some("fixture:projectile"), 0),
        OfferedUse::Absent
    );
    let EntityUseAdmission::Components { source, .. } = &mut compiled
        .interaction_traits
        .iter_mut()
        .find(|entry| entry.identifier.as_ref() == "minecraft:zombie")
        .unwrap()
        .offered_use
    else {
        unreachable!()
    };
    *source = 1;
    assert!(crate::encode_entity_blob(&compiled).is_err());
}
