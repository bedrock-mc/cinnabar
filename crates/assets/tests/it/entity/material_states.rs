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

#[test]
fn existing_entity_carrier_keeps_material_defaults_and_encoded_identity() {
    let Some(path) = std::env::var_os("CINNABAR_ENTITY_CARRIER") else {
        eprintln!(
            "missing fixture CINNABAR_ENTITY_CARRIER: skipping existing entity carrier check"
        );
        return;
    };
    let encoded = match std::fs::read(&path) {
        Ok(encoded) => encoded,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "missing fixture CINNABAR_ENTITY_CARRIER={path:?}: skipping existing entity carrier check"
            );
            return;
        }
        Err(error) => panic!("read existing entity carrier: {error}"),
    };
    let runtime = RuntimeEntityAssets::decode(&encoded).expect("decode existing entity carrier");
    assert!(!runtime.render_data().layers.is_empty());
    assert!(
        runtime
            .render_data()
            .layers
            .iter()
            .filter_map(|layer| layer.material_state)
            .all(|state| !state.additive_alpha && !state.disable_overlay)
    );
    assert_eq!(runtime.encode().unwrap().as_ref(), encoded.as_slice());
}
