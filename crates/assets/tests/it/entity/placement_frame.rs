use assets::{CompiledEntityAssets, EntityAssetSource, RuntimeEntityAssets, placement};
use sha2::{Digest, Sha256};

/// Adds the frame source to an independently authored rig fixture and remaps its source indices.
fn fixture(identifier: &str) -> CompiledEntityAssets {
    let mut compiled = super::suite::carrier_v4_fixture();
    let mut sources = compiled.sources.to_vec();
    sources.insert(
        4,
        EntityAssetSource {
            path: placement::SOURCE_PATH.into(),
            source_bytes: placement::SOURCE_BYTES.len() as u32,
            source_sha256: Sha256::digest(placement::SOURCE_BYTES).into(),
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
    compiled.symbols[0].identifier = identifier.into();
    compiled.rig_bindings[0].placement_frame = placement::builtin_placement_frame(identifier);
    compiled.with_unresolved_admissions()
}

#[test]
fn placement_frame_admission_checks_source_identity_and_each_rig() {
    let compiled = fixture("minecraft:arrow");
    let runtime = RuntimeEntityAssets::from_compiled(compiled.clone()).unwrap();
    assert_eq!(
        runtime.rig_bindings()[0].placement_frame.placement(),
        Some(placement::ActorYawFrame::Absolute)
    );
    assert_eq!(
        runtime.rig_bindings()[0].placement_frame.target_rotation(),
        Some(placement::ActorYawFrame::Absolute)
    );
    let mut altered = compiled.clone();
    altered.sources[4].source_sha256[0] ^= 1;
    assert!(RuntimeEntityAssets::from_compiled(altered).is_err());
    let mut altered = compiled;
    altered.rig_bindings[0].placement_frame = placement::EntityPlacementFrame::Unresolved;
    assert!(RuntimeEntityAssets::from_compiled(altered).is_err());
}

#[test]
fn placement_frame_admission_keeps_unsupported_identities_unresolved() {
    let mut compiled = fixture("fixture:unsupported");
    assert!(RuntimeEntityAssets::from_compiled(compiled.clone()).is_ok());
    assert_eq!(compiled.rig_bindings[0].placement_frame.placement(), None);
    compiled.rig_bindings[0].placement_frame =
        placement::builtin_placement_frame("minecraft:arrow");
    assert!(RuntimeEntityAssets::from_compiled(compiled).is_err());
    let mut without_source = super::suite::carrier_v4_fixture();
    without_source.rig_bindings[0].placement_frame =
        placement::builtin_placement_frame("minecraft:arrow");
    assert!(RuntimeEntityAssets::from_compiled(without_source).is_err());
}
