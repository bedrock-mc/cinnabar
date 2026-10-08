use assets::{RuntimeEntityAssets, encode_entity_blob};

use super::suite::inherited_geometry_fixture;

/// Both admission paths retain the selected parent graph and clones share it unchanged.
#[test]
fn admitted_geometry_parents_are_shared_and_match_the_decoded_catalog() {
    let compiled = inherited_geometry_fixture();
    let encoded = encode_entity_blob(&compiled).unwrap();
    let admitted = RuntimeEntityAssets::from_compiled(compiled).unwrap();
    let decoded = RuntimeEntityAssets::decode(&encoded).unwrap();
    assert_eq!(admitted.geometry_parents(), &[None, Some(0)]);
    assert_eq!(decoded.geometry_parents(), admitted.geometry_parents());
    let cloned = admitted.clone();
    assert!(std::ptr::eq(
        cloned.geometry_parents(),
        admitted.geometry_parents()
    ));
}

/// A compiled catalog and a decode of its carrier must be indistinguishable, identity included.
#[test]
fn encoded_admission_reports_the_identity_of_its_carrier() {
    let (admitted, blob) =
        RuntimeEntityAssets::from_compiled_encoded(inherited_geometry_fixture()).unwrap();
    let decoded = RuntimeEntityAssets::decode(&blob.unwrap()).unwrap();
    assert!(admitted.carrier_identity().is_some());
    assert_eq!(format!("{admitted:?}"), format!("{decoded:?}"));
}
