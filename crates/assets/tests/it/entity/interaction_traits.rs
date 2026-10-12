use assets::{CompiledEntityAssets, EntityAssetSource, RuntimeEntityAssets, interactions};

/// Binds an original rig inventory to the canonical interaction source.
fn fixture() -> CompiledEntityAssets {
    let mut compiled = super::suite::carrier_v4_fixture();
    let (length, hash) = interactions::source_identity();
    let mut sources = compiled.sources.to_vec();
    sources.insert(
        4,
        EntityAssetSource {
            path: interactions::SOURCE_PATH.into(),
            source_bytes: length,
            source_sha256: hash,
        },
    );
    compiled.sources = sources.into();
    for symbol in &mut compiled.symbols {
        if symbol.source_index >= 4 {
            symbol.source_index += 1;
        }
    }
    for geometry in &mut compiled.geometries {
        if geometry.source_index >= 4 {
            geometry.source_index += 1;
        }
    }
    for item in &mut compiled.item_visuals {
        if item.source >= 4 {
            item.source += 1;
        }
    }
    for candidate in &mut compiled.render.candidates {
        if candidate.source >= 4 {
            candidate.source += 1;
        }
    }
    compiled.symbols[0].identifier = "fixture:unsupported".into();
    compiled.interaction_traits = interactions::compile(&compiled.symbols);
    compiled.with_unresolved_seats()
}

#[test]
fn interaction_traits_admission_checks_the_independent_entity_inventory() {
    let compiled = fixture();
    let runtime = RuntimeEntityAssets::from_compiled(compiled.clone()).unwrap();
    assert_eq!(
        runtime.interaction_traits_for("fixture:unsupported"),
        interactions::EntityInteractionTraits::Unresolved
    );
    assert_eq!(
        runtime.interaction_traits_for("minecraft:arrow").pickable(),
        Some(false)
    );
    assert_eq!(
        runtime
            .interaction_traits_for("minecraft:painting")
            .aim_assist(),
        Some(false)
    );
    assert_eq!(
        runtime
            .interaction_traits_for("minecraft:painting")
            .pickable(),
        Some(true)
    );
    let mut omitted = compiled.clone();
    omitted.interaction_traits = omitted
        .interaction_traits
        .into_vec()
        .into_iter()
        .filter(|entry| entry.identifier.as_ref() != "fixture:unsupported")
        .collect();
    assert!(RuntimeEntityAssets::from_compiled(omitted).is_err());
    let mut forged = compiled;
    let arrow = forged
        .interaction_traits
        .iter_mut()
        .find(|entry| entry.identifier.as_ref() == "minecraft:arrow")
        .unwrap();
    assert_ne!(
        arrow.traits,
        interactions::EntityInteractionTraits::Unresolved
    );
    arrow.traits = interactions::EntityInteractionTraits::Unresolved;
    assert!(RuntimeEntityAssets::from_compiled(forged).is_err());
}

#[test]
fn interaction_traits_admission_rejects_altered_or_missing_sources() {
    let compiled = fixture();
    let encoded = assets::encode_entity_blob(&compiled).unwrap();
    let decoded = RuntimeEntityAssets::decode(&encoded).unwrap();
    assert_eq!(
        decoded
            .interaction_traits_for("minecraft:arrow")
            .obstructs_placement(),
        Some(false)
    );
    let mut altered = compiled.clone();
    altered.sources[4].source_sha256[0] ^= 1;
    assert!(RuntimeEntityAssets::from_compiled(altered).is_err());
    let mut absent = super::suite::carrier_v4_fixture();
    absent.interaction_traits = compiled.interaction_traits;
    assert!(RuntimeEntityAssets::from_compiled(absent).is_err());
}

#[test]
fn interaction_traits_require_inventory_entries_without_a_compatibility_source() {
    let mut compiled = super::suite::carrier_v4_fixture();
    assert!(RuntimeEntityAssets::from_compiled(compiled.clone()).is_ok());
    compiled.interaction_traits = Box::new([]);
    assert!(RuntimeEntityAssets::from_compiled(compiled).is_err());
}
