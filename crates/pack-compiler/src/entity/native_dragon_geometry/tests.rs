use super::*;

fn parse(bytes: &[u8]) -> BTreeMap<(Box<str>, Box<str>), PendingGeometry> {
    let mut geometries = BTreeMap::new();
    super::super::geometry::parse_geometry(
        PATH,
        std::path::Path::new(PATH),
        &serde_json::from_slice(bytes).unwrap(),
        &mut BTreeMap::new(),
        &mut geometries,
    )
    .unwrap();
    geometries
}

#[test]
fn similarly_named_custom_geometry_keeps_authored_bones() {
    let bytes = br#"{"format_version":"1.12.0","minecraft:geometry":[{
        "description":{"identifier":"geometry.dragon"},"bones":[
            {"name":"root"},{"name":"neck","parent":"root","pivot":[0,24,0]}
        ]}]}"#;
    let mut geometries = parse(bytes);
    let before = geometries[&(GEOMETRY.into(), PATH.into())].bones.clone();
    expand_sample(PATH, bytes, &mut geometries);
    assert_eq!(geometries[&(GEOMETRY.into(), PATH.into())].bones, before);
}

#[test]
fn dragon_segments_share_the_authored_neck_mesh_and_bind_to_the_complete_hierarchy() {
    let Some(bytes) = sample_bytes() else {
        return;
    };
    let mut geometries = parse(&bytes);
    let key = (GEOMETRY.into(), PATH.into());
    let template = geometries[&key]
        .bones
        .iter()
        .find(|bone| bone.name.as_ref() == "neck")
        .unwrap()
        .clone();
    expand_sample(PATH, &bytes, &mut geometries);
    let bones = &geometries[&key].bones;
    assert_eq!(bones.len(), 37);
    assert!(bones.iter().all(|bone| bone.name.as_ref() != "neck"));
    for (prefix, count) in [("neck", 5), ("tail", 12)] {
        for index in 1..=count {
            let name = format!("{prefix}{index}");
            let segment = bones
                .iter()
                .find(|bone| bone.name.as_ref() == name)
                .unwrap();
            assert_eq!(segment.parent.as_deref(), Some("root"));
            assert_eq!(segment.pivot, template.pivot);
            assert_eq!(segment.cubes, template.cubes);
            assert_eq!(segment.rotation, template.rotation);
            assert_eq!(segment.mirror, template.mirror);
        }
    }
    for (name, parent) in [
        ("root", None),
        ("head", Some("root")),
        ("jaw", Some("head")),
        ("wing", Some("root")),
        ("wingtip", Some("wing")),
        ("rearlegtip1", Some("rearleg1")),
        ("rearfoot1", Some("rearlegtip1")),
    ] {
        assert_eq!(
            bones
                .iter()
                .find(|bone| bone.name.as_ref() == name)
                .unwrap()
                .parent
                .as_deref(),
            parent
        );
    }
    let rotation = |name| {
        bones
            .iter()
            .find(|bone| bone.name.as_ref() == name)
            .unwrap()
            .rotation
            .unwrap()
            .map(|value| value.get())
    };
    assert_eq!(rotation("wing"), [0.0, 14.3, 0.0]);
    assert_eq!(rotation("wing1"), [0.0, 14.3, 180.0]);
    let expanded = bones.clone();
    expand_sample(PATH, &bytes, &mut geometries);
    assert_eq!(geometries[&key].bones, expanded);
}

fn sample_bytes() -> Option<Vec<u8>> {
    let manifest: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../../../../assets/vanilla-source.json"))
            .unwrap();
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(manifest["cache_dir"].as_str().unwrap())
        .join("resource_pack")
        .join(PATH);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!(
                "skipping dragon segment fixture: {} is absent",
                path.display()
            );
            return None;
        }
        Err(error) => panic!("read {}: {error}", path.display()),
    };
    Some(bytes)
}

#[test]
fn dragon_wing_tip_hinges_keep_membranes_attached_to_the_inner_spars() {
    let Some(bytes) = sample_bytes() else {
        return;
    };
    let mut geometries = parse(&bytes);
    let key = (GEOMETRY.into(), PATH.into());
    let authored = geometries[&key].bones.clone();
    expand_sample(PATH, &bytes, &mut geometries);
    let bones = &geometries[&key].bones;
    let find = |name| {
        bones
            .iter()
            .find(|bone| bone.name.as_ref() == name)
            .unwrap()
    };
    for (wing, tip) in [("wing", "wingtip"), ("wing1", "wingtip1")] {
        let wing = find(wing);
        let tip = find(tip);
        let hinge = tip.pivot.unwrap().map(|value| value.get());
        let shoulder = wing.pivot.unwrap().map(|value| value.get());
        let spar = &wing.cubes[0];
        assert_eq!(hinge, [spar.origin[0].get(), shoulder[1], shoulder[2]]);
        let inner = &wing.cubes[1];
        let outer = &tip.cubes[1];
        assert_eq!(
            inner.origin.map(|value| value.get()),
            [
                outer.origin[0].get() + outer.size[0].get(),
                outer.origin[1].get(),
                outer.origin[2].get(),
            ],
            "the two membrane halves meet at the hinge"
        );
        let original = authored.iter().find(|bone| bone.name == tip.name).unwrap();
        let original_pivot = original.pivot.unwrap().map(|value| value.get());
        for (cube, before) in tip.cubes.iter().zip(&original.cubes) {
            for axis in 0..3 {
                assert_eq!(
                    cube.origin[axis].get() - hinge[axis],
                    before.origin[axis].get() - original_pivot[axis]
                );
                assert_eq!(
                    cube.pivot[axis].get() - hinge[axis],
                    before.pivot[axis].get() - original_pivot[axis]
                );
            }
            assert_eq!(cube.size, before.size);
            assert_eq!(cube.uv, before.uv);
        }
    }
}
