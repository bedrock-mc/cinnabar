use super::*;

fn parse(path: &str, bytes: &[u8]) -> BTreeMap<(Box<str>, Box<str>), PendingGeometry> {
    let mut geometries = BTreeMap::new();
    super::super::geometry::parse_geometry(
        path,
        std::path::Path::new(path),
        &serde_json::from_slice(bytes).unwrap(),
        &mut BTreeMap::new(),
        &mut geometries,
    )
    .unwrap();
    geometries
}

fn assert_same(
    actual: &BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
    expected: &BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) {
    assert!(actual.keys().eq(expected.keys()));
    for (key, geometry) in actual {
        assert_eq!(geometry.bones, expected[key].bones, "{key:?}");
    }
}

#[test]
fn custom_fox_geometry_keeps_its_authored_defaults() {
    let bytes = br#"{"format_version":"1.21.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.fox"},"bones":[
            {"name":"body","pivot":[0,3,0]},
            {"name":"tail","rotation":[20,0,0]}
        ]}]}"#;
    let mut geometries = parse(FOX_PATH, bytes);
    let before = geometries.clone();
    restore_sample_defaults(FOX_PATH, bytes, &mut geometries);
    assert_same(&geometries, &before);
}

#[test]
fn pinned_adult_fox_restores_native_cube_binds_without_moving_children_or_baby() {
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
    let bytes = std::fs::read(root.join(FOX_PATH)).unwrap();
    let mut geometries = parse(FOX_PATH, &bytes);
    let before = geometries.clone();
    let mut custom = bytes.clone();
    custom.push(b'\n');
    restore_sample_defaults(FOX_PATH, &custom, &mut geometries);
    assert_same(&geometries, &before);

    restore_sample_defaults(FOX_PATH, &bytes, &mut geometries);
    let key = (FOX_GEOMETRY.into(), FOX_PATH.into());
    let old = &before[&key];
    let new = &geometries[&key];
    assert_eq!(old.bones.len(), new.bones.len());
    for (old, new) in old.bones.iter().zip(new.bones.iter()) {
        let mut expected = old.clone();
        let native = match old.name.as_ref() {
            "body" => Some([90.0, 0.0, 0.0]),
            "tail" => Some([80.0, 0.0, 0.0]),
            _ => None,
        };
        expected.bind_pose_rotation =
            native.map(|angles| angles.map(|v| EntityGeometryScalar::new(v).unwrap()));
        assert_eq!(
            &expected, new,
            "{} native bind, all other fields preserved",
            old.name
        );
    }

    // Explicit cube binds remain authoritative even in a geometry supplied to the repair.
    for bone in &mut geometries.get_mut(&key).unwrap().bones {
        bone.bind_pose_rotation = Some([EntityGeometryScalar::ZERO; 3]);
    }
    let explicit = geometries.clone();
    restore_sample_defaults(FOX_PATH, &bytes, &mut geometries);
    assert_same(&geometries, &explicit);

    let baby_path = "models/entity/baby_fox.geo.json";
    let baby_bytes = std::fs::read(root.join(baby_path)).unwrap();
    let mut baby = parse(baby_path, &baby_bytes);
    let before = baby.clone();
    restore_sample_defaults(baby_path, &baby_bytes, &mut baby);
    assert_same(&baby, &before);
}

#[test]
fn verified_fox_bind_restoration_changes_only_missing_cube_orientations() {
    let bytes = br#"{"format_version":"1.21.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.fox"},"bones":[
            {"name":"body","pivot":[0,8,0]},
            {"name":"head","parent":"body","pivot":[0,8,-3]},
            {"name":"tail","parent":"body","pivot":[0,8,7],"rotation":[5,0,0]}
        ]}]}"#;
    let mut geometries = parse(FOX_PATH, bytes);
    let key = (FOX_GEOMETRY.into(), FOX_PATH.into());
    let before = geometries[&key].bones.clone();
    restore_verified_geometry(&mut geometries, FOX_PATH, FOX_GEOMETRY);
    for (old, new) in before.iter().zip(geometries[&key].bones.iter()) {
        let mut expected = old.clone();
        expected.bind_pose_rotation = match old.name.as_ref() {
            "body" => Some([90.0, 0.0, 0.0]),
            "tail" => Some([80.0, 0.0, 0.0]),
            _ => None,
        }
        .map(|angles| angles.map(|v| EntityGeometryScalar::new(v).unwrap()));
        assert_eq!(&expected, new);
    }
    let body = &mut geometries.get_mut(&key).unwrap().bones[0];
    body.bind_pose_rotation = Some([EntityGeometryScalar::ZERO; 3]);
    let explicit = geometries.clone();
    restore_verified_geometry(&mut geometries, FOX_PATH, FOX_GEOMETRY);
    assert_same(&geometries, &explicit);
}
