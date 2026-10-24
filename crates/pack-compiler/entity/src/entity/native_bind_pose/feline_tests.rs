use super::*;

const ADULT: &[u8] = br#"{"format_version":"1.21.0","minecraft:geometry":[{
    "description":{"identifier":"IDENTIFIER"},"bones":[
        {"name":"body","pivot":[0,7,1],"cubes":[{"origin":[-2,-1,-2],"size":[4,16,6],"uv":[20,0]}]},
        {"name":"head","parent":"body","pivot":[0,9,-9]},
        {"name":"tail1","parent":"body","pivot":[0,9,8]},
        {"name":"tail2","parent":"tail1","pivot":[0,9,16]},
        {"name":"backLegL","parent":"body","pivot":[1.1,6,7]},
        {"name":"frontLegL","parent":"body","pivot":[1.2,10,-4]}
    ]}]}"#;

fn parse(path: &str, identifier: &str) -> BTreeMap<(Box<str>, Box<str>), PendingGeometry> {
    let json = String::from_utf8(ADULT.to_vec())
        .unwrap()
        .replace("IDENTIFIER", identifier);
    let mut geometries = BTreeMap::new();
    super::super::geometry::parse_geometry(
        path,
        std::path::Path::new(path),
        &serde_json::from_str(&json).unwrap(),
        &mut BTreeMap::new(),
        &mut geometries,
    )
    .unwrap();
    geometries
}

fn expected_bind(name: &str) -> Option<[EntityGeometryScalar; 3]> {
    matches!(name, "body" | "tail1" | "tail2")
        .then(|| [90.0, 0.0, 0.0].map(|v| EntityGeometryScalar::new(v).unwrap()))
}

#[test]
fn adult_cat_and_ocelot_lay_body_and_tail_cubes_along_z_without_moving_other_bones() {
    for (path, identifier) in [(CAT_PATH, CAT_GEOMETRY), (OCELOT_PATH, OCELOT_GEOMETRY)] {
        let mut geometries = parse(path, identifier);
        let key = (identifier.into(), path.into());
        let before = geometries[&key].bones.clone();
        restore_verified_geometry(&mut geometries, path, identifier);
        for (old, new) in before.iter().zip(geometries[&key].bones.iter()) {
            let mut expected = old.clone();
            expected.bind_pose_rotation = expected_bind(&old.name);
            assert_eq!(&expected, new, "{identifier} {}", old.name);
        }
    }
}

#[test]
fn unpinned_cat_sources_keep_their_authored_defaults() {
    let mut geometries = parse(CAT_PATH, CAT_GEOMETRY);
    let before = geometries.clone();
    restore_sample_defaults(CAT_PATH, ADULT, &mut geometries);
    for (key, geometry) in &geometries {
        assert_eq!(geometry.bones, before[key].bones);
    }
}

#[test]
fn pinned_cat_and_ocelot_samples_restore_native_body_and_tail_binds() {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ROOT").map(std::path::PathBuf::from) else {
        eprintln!("skipping missing fixture: CINNABAR_VANILLA_ROOT is not set");
        return;
    };
    if !root.exists() {
        eprintln!(
            "skipping missing fixture: CINNABAR_VANILLA_ROOT at {}",
            root.display()
        );
        return;
    }
    for (path, identifier) in [(CAT_PATH, CAT_GEOMETRY), (OCELOT_PATH, OCELOT_GEOMETRY)] {
        let bytes = std::fs::read(root.join(path)).unwrap();
        let mut geometries = BTreeMap::new();
        super::super::geometry::parse_geometry(
            path,
            &root.join(path),
            &serde_json::from_slice(&bytes).unwrap(),
            &mut BTreeMap::new(),
            &mut geometries,
        )
        .unwrap();
        restore_sample_defaults(path, &bytes, &mut geometries);
        let key = (identifier.into(), path.into());
        for bone in geometries[&key].bones.iter() {
            assert_eq!(
                bone.bind_pose_rotation,
                expected_bind(&bone.name),
                "{identifier} {}",
                bone.name
            );
        }
    }
}
