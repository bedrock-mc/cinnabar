//! Content-pinned parity correction for an omission in the downloaded vanilla samples.
use std::collections::BTreeMap;

use assets::EntityGeometryScalar;
use sha2::{Digest, Sha256};

use super::PendingGeometry;

#[cfg(test)]
#[path = "native_bind_pose/fox_tests.rs"]
mod fox_tests;

const POLAR_BEAR_PATH: &str = "models/entity/polar_bear.geo.json";
const POLAR_BEAR_GEOMETRY: &str = "geometry.polarbear";
const LLAMA_PATH: &str = "models/entity/llama.geo.json";
const LLAMA_GEOMETRY: &str = "geometry.llama.v1.8";
const FOX_PATH: &str = "models/entity/fox.geo.json";
const FOX_GEOMETRY: &str = "geometry.fox";
// Canonical pinned sample source, not a blanket default for similarly named custom models.
const SAMPLE_SHA256: [u8; 32] = [
    0x54, 0x11, 0xea, 0x78, 0xae, 0x01, 0xce, 0x1f, 0xa1, 0xbe, 0x30, 0xa5, 0x18, 0x59, 0xe5, 0x1e,
    0x3b, 0xab, 0x38, 0xc4, 0x3c, 0x72, 0x02, 0xd8, 0x7d, 0xca, 0x85, 0x95, 0xe5, 0x42, 0xf4, 0xb0,
];
const LLAMA_SAMPLE_SHA256: [u8; 32] = [
    0x0e, 0x8b, 0xa9, 0x21, 0x7f, 0x87, 0x7f, 0x37, 0xbd, 0x65, 0x86, 0xca, 0xd1, 0x5b, 0x07, 0x5d,
    0xe3, 0x9d, 0xc0, 0x6e, 0xad, 0x94, 0x29, 0x75, 0x5c, 0x18, 0xe9, 0x55, 0x7d, 0xf1, 0x6d, 0x6b,
];
const FOX_SAMPLE_SHA256: [u8; 32] = [
    0x55, 0xfb, 0x6b, 0x80, 0xe3, 0x10, 0x64, 0xb2, 0x3c, 0x2a, 0xa6, 0x0e, 0xf2, 0x82, 0xb2, 0xa3,
    0x30, 0x70, 0xdd, 0xd7, 0xc2, 0xf5, 0x8f, 0x2d, 0xff, 0x68, 0x0e, 0x7e, 0x92, 0xaa, 0xaf, 0x57,
];
// Shipped iOS vanilla/__brarchive/models/entity.brarchive, polar_bear.geo.json:
// the body cube bind pose is +90 X, independently of its bone's default rotation.
// llama.geo.json has the same bind. Native GeometryGroup keeps same-identifier history;
// Geometry::_parseBones reads missing bind fields through
// JsonValueHierarchy::get, retaining the older shipped bind under the modern sample.
const NATIVE_BODY_BIND_ROTATION: [f32; 3] = [90.0, 0.0, 0.0];
// The same native base model retains these adult fox cube binds beneath its modern
// replacements. The baby model is independently authored. See docs/reference/fox-rendering.md.
const FOX_BINDS: &[(&str, [f32; 3])] = &[
    ("body", NATIVE_BODY_BIND_ROTATION),
    ("tail", [80.0, 0.0, 0.0]),
];

pub(super) fn restore_sample_defaults(
    path: &str,
    bytes: &[u8],
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) {
    let (geometry, expected) = match path {
        POLAR_BEAR_PATH => (POLAR_BEAR_GEOMETRY, SAMPLE_SHA256),
        LLAMA_PATH => (LLAMA_GEOMETRY, LLAMA_SAMPLE_SHA256),
        FOX_PATH => (FOX_GEOMETRY, FOX_SAMPLE_SHA256),
        _ => return,
    };
    if <[u8; 32]>::from(Sha256::digest(bytes)) != expected {
        return;
    }
    restore_verified_geometry(geometries, path, geometry);
}

fn restore_verified_geometry(
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
    path: &str,
    identifier: &str,
) {
    let Some(geometry) = geometries.get_mut(&(identifier.into(), path.into())) else {
        return;
    };
    let binds = if identifier == FOX_GEOMETRY {
        FOX_BINDS
    } else {
        &[("body", NATIVE_BODY_BIND_ROTATION)]
    };
    for (name, rotation) in binds {
        if let Some(bone) = geometry
            .bones
            .iter_mut()
            .find(|bone| bone.name.as_ref() == *name)
            && bone.bind_pose_rotation.is_none()
        {
            bone.bind_pose_rotation = Some(rotation.map(|value| {
                EntityGeometryScalar::new(value).expect("native bind-pose constants are finite")
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use assets::EntityGeometryBone;

    fn fixture() -> BTreeMap<(Box<str>, Box<str>), PendingGeometry> {
        let geometry = PendingGeometry {
            identifier: POLAR_BEAR_GEOMETRY.into(),
            inherits: None,
            source_path: POLAR_BEAR_PATH.into(),
            texture_width: None,
            texture_height: None,
            bones: vec![EntityGeometryBone {
                name: "body".into(),
                parent: None,
                pivot: None,
                rotation: None,
                bind_pose_rotation: None,
                mirror: None,
                inflate: None,
                never_render: None,
                reset: None,
                binding: None,
                texture_meshes: Box::new([]),
                cubes: Box::new([]),
            }]
            .into(),
        };
        [(
            (POLAR_BEAR_GEOMETRY.into(), POLAR_BEAR_PATH.into()),
            geometry,
        )]
        .into()
    }

    #[test]
    fn native_bind_pose_does_not_rewrite_a_similarly_named_custom_source() {
        let mut geometries = fixture();
        restore_sample_defaults(
            POLAR_BEAR_PATH,
            b"synthetic custom geometry",
            &mut geometries,
        );
        assert!(
            geometries.values().next().unwrap().bones[0]
                .bind_pose_rotation
                .is_none()
        );
    }

    #[test]
    fn native_bind_pose_preserves_an_explicit_override_and_does_not_rotate_bones() {
        let mut geometries = fixture();
        restore_verified_geometry(&mut geometries, POLAR_BEAR_PATH, POLAR_BEAR_GEOMETRY);
        let body = &mut geometries.values_mut().next().unwrap().bones[0];
        assert_eq!(
            body.bind_pose_rotation.unwrap().map(|v| v.get()),
            NATIVE_BODY_BIND_ROTATION
        );
        assert!(body.rotation.is_none());
        body.bind_pose_rotation = Some([EntityGeometryScalar::ZERO; 3]);
        restore_verified_geometry(&mut geometries, POLAR_BEAR_PATH, POLAR_BEAR_GEOMETRY);
        assert_eq!(
            geometries.values().next().unwrap().bones[0].bind_pose_rotation,
            Some([EntityGeometryScalar::ZERO; 3])
        );
    }

    #[test]
    #[ignore = "requires CINNABAR_VANILLA_POLAR_BEAR_SOURCE pointing to the downloaded sample"]
    fn downloaded_native_bind_pose_repair_is_content_pinned() {
        let path =
            std::path::PathBuf::from(std::env::var("CINNABAR_VANILLA_POLAR_BEAR_SOURCE").unwrap());
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(<[u8; 32]>::from(Sha256::digest(&bytes)), SAMPLE_SHA256);
        let mut geometries = BTreeMap::new();
        super::super::geometry::parse_geometry(
            POLAR_BEAR_PATH,
            &path,
            &serde_json::from_slice(&bytes).unwrap(),
            &mut BTreeMap::new(),
            &mut geometries,
        )
        .unwrap();
        assert!(
            geometries
                .values()
                .next()
                .unwrap()
                .bones
                .iter()
                .find(|bone| bone.name.as_ref() == "body")
                .unwrap()
                .bind_pose_rotation
                .is_none()
        );
        let mut changed = bytes.clone();
        changed.push(b'\n');
        restore_sample_defaults(POLAR_BEAR_PATH, &changed, &mut geometries);
        assert!(
            geometries
                .values()
                .next()
                .unwrap()
                .bones
                .iter()
                .find(|bone| bone.name.as_ref() == "body")
                .unwrap()
                .bind_pose_rotation
                .is_none()
        );
        restore_sample_defaults(POLAR_BEAR_PATH, &bytes, &mut geometries);
        let body = geometries
            .values()
            .next()
            .unwrap()
            .bones
            .iter()
            .find(|bone| bone.name.as_ref() == "body")
            .unwrap();
        assert_eq!(
            body.bind_pose_rotation.unwrap().map(|v| v.get()),
            NATIVE_BODY_BIND_ROTATION
        );
    }

    #[test]
    #[ignore = "requires CINNABAR_VANILLA_ROOT pointing to the downloaded resource pack"]
    fn downloaded_llama_retains_the_inherited_native_body_bind_only_for_the_pinned_source() {
        let root = std::path::PathBuf::from(std::env::var_os("CINNABAR_VANILLA_ROOT").unwrap());
        let path = root.join(LLAMA_PATH);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(
            <[u8; 32]>::from(Sha256::digest(&bytes)),
            LLAMA_SAMPLE_SHA256
        );
        let mut geometries = BTreeMap::new();
        super::super::geometry::parse_geometry(
            LLAMA_PATH,
            &path,
            &serde_json::from_slice(&bytes).unwrap(),
            &mut BTreeMap::new(),
            &mut geometries,
        )
        .unwrap();
        let before = geometries.clone();
        let mut custom = bytes.clone();
        custom.push(b'\n');
        restore_sample_defaults(LLAMA_PATH, &custom, &mut geometries);
        assert!(
            geometries
                .values()
                .next()
                .unwrap()
                .bones
                .iter()
                .all(|bone| bone.bind_pose_rotation.is_none())
        );
        restore_sample_defaults(LLAMA_PATH, &bytes, &mut geometries);
        for (key, old) in before {
            let repaired = &geometries[&key];
            assert_eq!(old.bones.len(), repaired.bones.len());
            for (old, new) in old.bones.iter().zip(repaired.bones.iter()) {
                let mut expected = old.clone();
                if old.name.as_ref() == "body" {
                    expected.bind_pose_rotation = Some(
                        NATIVE_BODY_BIND_ROTATION
                            .map(|value| EntityGeometryScalar::new(value).unwrap()),
                    );
                }
                assert_eq!(
                    &expected, new,
                    "all other defaults and child pivots stay authored"
                );
            }
        }
    }
}
