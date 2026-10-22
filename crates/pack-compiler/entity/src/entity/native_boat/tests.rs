use super::*;

/// Encodes original single-colour artwork.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::RgbaImage::from_pixel(width, height, image::Rgba([90, 60, 30, 255]))
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    bytes.into_inner()
}

/// A client boat in the shape vanilla ships: named textures and geometry, no controller.
fn boat_pack(entity: Value) -> Vec<(Box<str>, Vec<u8>)> {
    let mut files: Vec<(Box<str>, Vec<u8>)> = vec![(
        "entity/boat.entity.json".into(),
        serde_json::to_vec(&entity).unwrap(),
    )];
    for wood in WOODS {
        files.push((
            format!("textures/entity/boat/{wood}.png").into(),
            png(128, 64),
        ));
    }
    files
}

fn vanilla_shaped_boat() -> Value {
    boat_entity(assets::facts::VanillaEntity::Boat.identifier())
}

fn boat_entity(identifier: &str) -> Value {
    let textures: serde_json::Map<String, Value> = WOODS
        .iter()
        .map(|wood| {
            (
                wood.to_string(),
                json!(format!("textures/entity/boat/{wood}")),
            )
        })
        .chain([("default".into(), json!("textures/entity/boat/oak"))])
        .collect();
    json!({"format_version": "1.8.0", "minecraft:client_entity": {"description": {
        "identifier": identifier,
        "textures": textures,
        "geometry": {"default": "geometry.boat"},
    }}})
}

fn compile(files: Vec<(Box<str>, Vec<u8>)>) -> crate::ActorPackCompilation {
    crate::compile_actor_pack(files)
        .unwrap()
        .expect("boat pack")
}

/// Rig binding of the boat client entity, if one compiled.
fn boat_rig(runtime: &assets::RuntimeEntityAssets) -> Option<&assets::EntityRigBinding> {
    let symbol = runtime.symbols().iter().position(|symbol| {
        symbol.kind == EntityAssetKind::Entity
            && symbol.identifier.as_ref() == assets::facts::VanillaEntity::Boat.identifier()
    })?;
    runtime
        .rig_bindings()
        .iter()
        .find(|rig| rig.entity_symbol as usize == symbol)
}

#[test]
fn controllerless_boats_get_a_rig_with_hull_and_raft_models() {
    let compiled = compile(boat_pack(vanilla_shaped_boat()));
    let runtime = assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap();
    let rig = boat_rig(&runtime).expect("vanilla-shaped boats are drawable");
    let geometry = |index: u32| {
        let binding = &runtime.rig_geometries()[index as usize];
        &runtime.geometries()[binding.geometry as usize]
    };
    let names: Vec<_> = (rig.first_geometry..rig.first_geometry + u32::from(rig.geometry_count))
        .map(|index| geometry(index).identifier.to_string())
        .collect();
    assert!(names.contains(&"geometry.boat".to_string()), "{names:?}");
    assert!(
        names.contains(&"geometry.cinnabar.raft".to_string()),
        "{names:?}"
    );
}

#[test]
fn authored_render_controllers_keep_their_own_boat() {
    let mut entity = vanilla_shaped_boat();
    entity["minecraft:client_entity"]["description"]["render_controllers"] =
        json!(["controller.render.custom_boat"]);
    let compiled = compile(boat_pack(entity));
    let runtime = assets::RuntimeEntityAssets::from_compiled(compiled.entities).unwrap();
    assert!(
        runtime
            .sources()
            .iter()
            .all(|source| !source.path.contains("_cinnabar")),
        "authored boats are not replaced"
    );
}

/// Rotates a pixel point about `pivot` the way authored X rotations turn.
fn rotate_x(point: [f32; 3], pivot: [f32; 3], degrees: f32) -> [f32; 3] {
    let (sin, cos) = (-degrees).to_radians().sin_cos();
    let [y, z] = [point[1] - pivot[1], point[2] - pivot[2]];
    [
        point[0],
        pivot[1] + y * cos - z * sin,
        pivot[2] + y * sin + z * cos,
    ]
}

#[test]
fn hull_and_raft_floors_rest_on_the_feet() {
    for (raft, expected) in [(false, [0.0, 3.0]), (true, [0.0, 8.0])] {
        let (parts, size) = model(false, raft);
        let document = geometry("geometry.test", &parts, size, 6.0);
        let mut extent = [f32::MAX, f32::MIN];
        for bone in document["bones"].as_array().unwrap() {
            let name = bone["name"].as_str().unwrap();
            if !name.starts_with("bottom") {
                continue;
            }
            let pivot: [f32; 3] = serde_json::from_value(bone["pivot"].clone()).unwrap();
            let degrees = bone["rotation"][0].as_f64().unwrap() as f32;
            for cube in bone["cubes"].as_array().unwrap() {
                let origin: [f32; 3] = serde_json::from_value(cube["origin"].clone()).unwrap();
                let size: [f32; 3] = serde_json::from_value(cube["size"].clone()).unwrap();
                for corner in 0..8 {
                    let point = std::array::from_fn(|axis| {
                        origin[axis]
                            + if corner >> axis & 1 == 1 {
                                size[axis]
                            } else {
                                0.0
                            }
                    });
                    let y = rotate_x(point, pivot, degrees)[1];
                    extent = [extent[0].min(y), extent[1].max(y)];
                }
            }
        }
        assert!(
            (extent[0] - expected[0]).abs() < 1e-4 && (extent[1] - expected[1]).abs() < 1e-4,
            "raft={raft}: {extent:?}"
        );
    }
}

#[test]
fn each_boat_entity_selects_only_its_own_texture_aliases() {
    let (mut sources, mut payloads) = (Vec::new(), SourcePayloads::new());
    let (mut symbols, mut geometries) = (BTreeMap::new(), BTreeMap::new());
    for (path, identifier, missing) in [
        (
            "entity/boat.entity.json",
            assets::facts::VanillaEntity::Boat,
            None,
        ),
        (
            "entity/chest_boat.entity.json",
            assets::facts::VanillaEntity::ChestBoat,
            Some("cherry"),
        ),
    ] {
        let mut entity = boat_entity(identifier.identifier());
        if let Some(wood) = missing {
            entity["minecraft:client_entity"]["description"]["textures"]
                .as_object_mut()
                .unwrap()
                .remove(wood);
        }
        let bytes = serde_json::to_vec(&entity).unwrap();
        parse_source(path, Path::new(path), &bytes, &mut symbols, &mut geometries).unwrap();
        payloads.insert(path.into(), bytes.into());
    }
    complete_bindings(
        Path::new(""),
        &mut sources,
        &mut payloads,
        &mut symbols,
        &mut geometries,
        None,
    )
    .unwrap();
    let controllers: Value =
        serde_json::from_slice(&payloads["render_controllers/_cinnabar/boats.json"]).unwrap();
    let woods = |entity: &str| {
        controllers["render_controllers"][format!("{CONTROLLER}.{entity}")]["arrays"]["textures"]
            ["Array.woods"]
            .clone()
    };
    let cherry = WOODS.iter().position(|wood| *wood == "cherry").unwrap();
    assert_eq!(woods("boat")[cherry], "Texture.cherry");
    assert_eq!(woods("chest_boat")[cherry], "Texture.default");
}

/// The pinned pack's boat entities, when the local fixture is present.
fn pinned_boat_entities() -> Option<Vec<Value>> {
    let Some(root) = std::env::var_os("CINNABAR_VANILLA_ACTOR_ROOT") else {
        eprintln!("missing fixture: CINNABAR_VANILLA_ACTOR_ROOT (pinned vanilla boat entities)");
        return None;
    };
    let root = std::path::PathBuf::from(root);
    ["entity/boat.entity.json", "entity/chest_boat.entity.json"]
        .iter()
        .map(|path| {
            let Ok(bytes) = std::fs::read(root.join(path)) else {
                eprintln!(
                    "missing fixture: pinned vanilla {path} at {}",
                    root.display()
                );
                return None;
            };
            Some(serde_json::from_slice(&bytes).unwrap())
        })
        .collect()
}

#[test]
fn variant_textures_match_the_pinned_boat_catalog() {
    let Some(entities) = pinned_boat_entities() else {
        return;
    };
    for entity in entities {
        let keys: std::collections::BTreeSet<&str> =
            entity["minecraft:client_entity"]["description"]["textures"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .filter(|key| *key != "default")
                .collect();
        assert_eq!(keys, WOODS.into_iter().collect());
    }
}
