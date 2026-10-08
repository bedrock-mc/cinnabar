use assets::{
    EntityControllerState, EntityGeometryScalar, RuntimeEntityAssets, encode_entity_blob,
};

use super::suite::carrier_v4_fixture;

#[test]
fn controller_blend_defaults_remain_compatible_and_durations_are_validated() {
    let fixture = carrier_v4_fixture();
    let encoded = serde_json::to_value(fixture.controller_states[0]).unwrap();
    assert!(encoded.get("blend_transition").is_none());
    assert!(encoded.get("blend_via_shortest_path").is_none());
    let state: EntityControllerState = serde_json::from_value(encoded).unwrap();
    assert_eq!(state.blend_transition, EntityGeometryScalar::ZERO);
    assert!(!state.blend_via_shortest_path);
    for invalid_duration in [-0.1_f32, f32::INFINITY, f32::NAN, f32::MAX] {
        let mut fixture = carrier_v4_fixture();
        fixture.controller_states[0].blend_transition =
            serde_json::from_value(serde_json::json!(invalid_duration.to_bits())).unwrap();
        assert!(
            fixture.validate().is_err(),
            "invalid controller duration must not enter a carrier"
        );
    }
    let mut fixture = carrier_v4_fixture();
    fixture.controller_states[0].blend_transition = EntityGeometryScalar::new(0.2).unwrap();
    fixture.controller_states[0].blend_via_shortest_path = true;
    let bytes = encode_entity_blob(&fixture).unwrap();
    let runtime = RuntimeEntityAssets::decode(&bytes).unwrap();
    assert_eq!(runtime.controller_states()[0].blend_transition.get(), 0.2);
    assert!(runtime.controller_states()[0].blend_via_shortest_path);
}
