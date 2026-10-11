use assets::{EntityGeometryScalar, RuntimeEntityAssets, encode_entity_blob};

use super::suite::carrier_v4_fixture;

#[test]
fn carrier_v4_preflight_accepts_optional_cube_bind_pose_rotation() {
    let mut compiled = carrier_v4_fixture();
    let rotation = [12.0, -34.0, 56.0]
        .map(|value| EntityGeometryScalar::new(value).expect("finite bind-pose rotation"));
    for bind_pose_rotation in [None, Some(rotation)] {
        compiled.geometries[0].bones[0].bind_pose_rotation = bind_pose_rotation;
        let encoded = encode_entity_blob(&compiled).expect("encode bind-pose carrier");
        let runtime =
            RuntimeEntityAssets::decode(&encoded).expect("preflight and decode bind-pose carrier");
        assert_eq!(runtime.geometries(), compiled.geometries.as_ref());
        assert_eq!(runtime.encode().unwrap().as_ref(), encoded.as_ref());
    }
}

#[test]
fn rebuilt_entity_carrier_preserves_cube_bind_poses() {
    let Some(path) = std::env::var_os("CINNABAR_ENTITY_CARRIER").map(std::path::PathBuf::from)
    else {
        eprintln!("skipping missing fixture: CINNABAR_ENTITY_CARRIER is not set");
        return;
    };
    if !path.exists() {
        eprintln!(
            "skipping missing fixture: CINNABAR_ENTITY_CARRIER at {}",
            path.display()
        );
        return;
    }
    let encoded = std::fs::read(path).expect("read existing entity carrier");
    let runtime = RuntimeEntityAssets::decode(&encoded).expect("decode rebuilt entity carrier");
    let bind_poses = runtime
        .geometries()
        .iter()
        .flat_map(|geometry| geometry.bones.iter())
        .filter(|bone| bone.bind_pose_rotation.is_some())
        .count();
    assert!(
        bind_poses > 0,
        "rebuilt carrier must retain cube bind poses"
    );
    assert_eq!(runtime.encode().unwrap().as_ref(), encoded.as_slice());
    println!("decoded entity carrier with {bind_poses} retained cube bind poses");
}
