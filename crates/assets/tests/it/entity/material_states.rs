use assets::{EntityRenderMaterialState, RuntimeEntityAssets, encode_entity_blob};

use super::suite::carrier_v4_fixture;

#[test]
fn additive_source_alpha_material_state_round_trips_through_entity_carrier() {
    let mut compiled = carrier_v4_fixture();
    for (additive_alpha, disable_overlay) in
        [(false, false), (false, true), (true, false), (true, true)]
    {
        let state = EntityRenderMaterialState {
            cull: false,
            blend: true,
            depth_write: false,
            emissive: true,
            additive: true,
            additive_alpha,
            disable_overlay,
            ..Default::default()
        };
        compiled.render.layers[0].material_state = Some(state);
        let encoded = encode_entity_blob(&compiled).unwrap();
        let runtime = RuntimeEntityAssets::decode(&encoded).unwrap();
        assert_eq!(runtime.render_data().layers[0].material_state, Some(state));
        assert_eq!(runtime.encode().unwrap().as_ref(), encoded.as_ref());
    }
}

/// Reads an explicitly named carrier fixture, skipping only when it is unavailable.
fn carrier_fixture(variable: &str) -> Option<Vec<u8>> {
    let Some(path) = std::env::var_os(variable) else {
        eprintln!("missing fixture {variable}: skipping entity carrier check");
        return None;
    };
    match std::fs::read(&path) {
        Ok(encoded) => Some(encoded),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("missing fixture {variable}={path:?}: skipping entity carrier check");
            None
        }
        Err(error) => panic!("read entity carrier {variable}={path:?}: {error}"),
    }
}

#[test]
fn existing_entity_carrier_keeps_encoded_identity() {
    let Some(encoded) = carrier_fixture("CINNABAR_ENTITY_CARRIER") else {
        return;
    };
    let runtime = RuntimeEntityAssets::decode(&encoded).expect("decode existing entity carrier");
    assert!(!runtime.render_data().layers.is_empty());
    assert_eq!(runtime.encode().unwrap().as_ref(), encoded.as_slice());
}

#[test]
fn legacy_entity_carrier_keeps_overlay_defaults_and_encoded_identity() {
    let Some(encoded) = carrier_fixture("CINNABAR_LEGACY_ENTITY_CARRIER") else {
        return;
    };
    let runtime = RuntimeEntityAssets::decode(&encoded).expect("decode legacy entity carrier");
    assert!(!runtime.render_data().layers.is_empty());
    assert!(
        runtime
            .render_data()
            .layers
            .iter()
            .filter_map(|layer| layer.material_state)
            .all(|state| !state.disable_overlay)
    );
    assert_eq!(runtime.encode().unwrap().as_ref(), encoded.as_slice());
}
