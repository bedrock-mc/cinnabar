use assets::{RuntimeEntityAssets, encode_entity_blob};

use super::suite::inherited_geometry_fixture;

#[test]
fn large_geometry_catalog_roundtrips_through_both_admission_paths() {
    let mut compiled = inherited_geometry_fixture();
    let mut geometries = compiled.geometries.to_vec();
    let mut symbols = compiled.symbols.to_vec();
    for index in geometries.len()..4_097 {
        let mut geometry = geometries[1].clone();
        geometry.identifier = format!("geometry.fixture_{index:04}").into();
        geometry.inherits = None;
        let mut symbol = symbols[2].clone();
        symbol.identifier = geometry.identifier.clone();
        geometries.push(geometry);
        symbols.push(symbol);
    }
    compiled.geometries = geometries.into();
    compiled.symbols = symbols.into();
    let admitted = RuntimeEntityAssets::from_compiled(compiled.clone()).unwrap();
    let encoded = encode_entity_blob(&compiled).unwrap();
    let decoded = RuntimeEntityAssets::decode(&encoded).unwrap();
    assert_eq!(decoded.geometries(), compiled.geometries.as_ref());
    assert_eq!(decoded.symbols(), compiled.symbols.as_ref());
    assert_eq!(admitted.geometries(), decoded.geometries());
    assert_eq!(decoded.encode().unwrap().as_ref(), encoded.as_ref());
}

#[test]
fn geometry_header_count_must_match_the_payload() {
    use sha2::{Digest, Sha256};

    let mut encoded = encode_entity_blob(&inherited_geometry_fixture()).unwrap();
    encoded[20..24].copy_from_slice(&u32::MAX.to_le_bytes());
    let payload_end = encoded.len() - 32;
    let digest = Sha256::digest(&encoded[..payload_end]);
    encoded[payload_end..].copy_from_slice(&digest);
    let error = RuntimeEntityAssets::decode(&encoded).unwrap_err();
    assert!(
        error.to_string().contains("counts do not match header"),
        "{error}"
    );
}

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

#[test]
fn optional_geometry_fields_roundtrip_through_strict_preflight_and_admission() {
    use assets::{EntityGeometryBounds, EntityGeometryScalar};

    let bounds = EntityGeometryBounds {
        center: [EntityGeometryScalar::ZERO; 3],
        half_extents: [EntityGeometryScalar::new(2.0).unwrap(); 3],
    };
    for animation_no_head_bob in [false, true] {
        for visible_bounds in [None, Some(bounds)] {
            let mut compiled = inherited_geometry_fixture();
            compiled.geometries[0].animation_no_head_bob = animation_no_head_bob;
            compiled.geometries[0].visible_bounds = visible_bounds;
            let admitted = RuntimeEntityAssets::from_compiled(compiled.clone()).unwrap();
            let encoded = encode_entity_blob(&compiled).unwrap();
            let decoded = RuntimeEntityAssets::decode(&encoded).unwrap();
            assert_eq!(decoded.geometries(), admitted.geometries());
            assert_eq!(decoded.encode().unwrap().as_ref(), encoded.as_ref());
        }
    }
}

#[test]
fn admitted_mixed_case_bone_parents_keep_head_and_full_model_boxes() {
    use assets::{EntityGeometryScalar, block_entity_geometry::heads::HeadModel};

    let scalar = |value| EntityGeometryScalar::new(value).unwrap();
    let mut compiled = inherited_geometry_fixture();
    let mut body = compiled.geometries[0].bones[0].clone();
    body.name = "body".into();
    body.parent = None;
    body.pivot = Some([scalar(0.0); 3]);
    body.rotation = Some([0.0, 90.0, 0.0].map(scalar));
    body.cubes[0].origin = [scalar(0.0); 3];
    body.cubes[0].size = [scalar(2.0); 3];
    body.cubes[0].rotation = [scalar(0.0); 3];
    body.cubes[0].inflate = scalar(0.0);
    let mut head = body.clone();
    head.name = "head".into();
    head.parent = Some("Body".into());
    head.rotation = None;
    head.cubes[0].origin = [1.0, 2.0, 3.0].map(scalar);
    let mut ear = head.clone();
    ear.name = "ear".into();
    ear.parent = Some("HeAd".into());
    ear.cubes[0].origin = [1.0, 5.0, 3.0].map(scalar);
    compiled.geometries[0].bones = [body, head, ear].into();
    let runtime = RuntimeEntityAssets::from_compiled(compiled).unwrap();
    let geometry = &runtime.geometries()[0];
    let model = HeadModel::build_tree(geometry, Some("hEaD"), 1.0, false).unwrap();
    assert_eq!(model.boxes.len(), 2);
    let corners = model.boxes[0].corners();
    for (axis, expected) in [(0, [-5.0, -3.0]), (1, [2.0, 4.0]), (2, [-3.0, -1.0])] {
        let min = corners
            .iter()
            .map(|corner| corner[axis])
            .fold(f32::INFINITY, f32::min);
        let max = corners
            .iter()
            .map(|corner| corner[axis])
            .fold(f32::NEG_INFINITY, f32::max);
        assert!((min - expected[0]).abs() < 1e-5);
        assert!((max - expected[1]).abs() < 1e-5);
    }
    assert_eq!(HeadModel::build(geometry, 1.0).unwrap().boxes.len(), 2);
    assert_eq!(
        HeadModel::build_tree(geometry, None, 1.0, false)
            .unwrap()
            .boxes
            .len(),
        3
    );
}
