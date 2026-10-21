use std::{fs, io::Cursor, path::Path};

use ::image::{ImageFormat, ImageReader};
use assets::{
    parse_skin_geometry,
    starter_skins::{
        CLASSIC_SKIN_GEOMETRY, MAX_STARTER_SKIN_GEOMETRY_BYTES, SLIM_SKIN_GEOMETRY,
        STARTER_SKIN_SIDE, STARTER_SKIN_SOURCES, StarterSkin, StarterSkins, encode_starter_skins,
    },
};
use serde_json::{Map, Value};

const MAX_TEXTURE_BYTES: u64 = 64 * 1024;
const MAX_MODELS_BYTES: u64 = 4 * 1024 * 1024;
const MODELS: &str = assets::LEGACY_ENTITY_GEOMETRY_PATH;
const MAX_INHERITANCE_DEPTH: usize = 8;
const SKIN_GEOMETRY_FORMAT_VERSION: &str = "1.21.0";

fn read_bounded(path: &Path, limit: u64) -> Result<Vec<u8>, String> {
    let size = fs::metadata(path)
        .map_err(|error| format!("{}: {error}", path.display()))?
        .len();
    if size > limit {
        return Err(format!("{} exceeds {limit} bytes", path.display()));
    }
    fs::read(path).map_err(|error| format!("{}: {error}", path.display()))
}

fn read_skin(path: &Path) -> Result<Box<[u8]>, String> {
    let decoded = ImageReader::with_format(
        Cursor::new(read_bounded(path, MAX_TEXTURE_BYTES)?),
        ImageFormat::Png,
    )
    .decode()
    .map_err(|error| format!("{}: {error}", path.display()))?
    .into_rgba8();
    if decoded.dimensions() != (STARTER_SKIN_SIDE, STARTER_SKIN_SIDE) {
        return Err(format!(
            "{} is {:?}, expected {STARTER_SKIN_SIDE}x{STARTER_SKIN_SIDE}",
            path.display(),
            decoded.dimensions()
        ));
    }
    Ok(decoded.into_raw().into_boxed_slice())
}

/// Emits both selected skin models in one modern schema, with legacy inheritance resolved.
fn skin_geometry(models: &Map<String, Value>) -> Result<String, String> {
    let entries = [CLASSIC_SKIN_GEOMETRY, SLIM_SKIN_GEOMETRY]
        .map(|model| modern_skin_model(models, model))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let document = serde_json::json!({
        "format_version": SKIN_GEOMETRY_FORMAT_VERSION,
        "minecraft:geometry": entries,
    });
    crate::entity::validate_geometry_document(&document).map_err(|error| error.to_string())?;
    let geometry = document.to_string();
    if geometry.len() > MAX_STARTER_SKIN_GEOMETRY_BYTES {
        return Err(format!(
            "skin geometry exceeds {MAX_STARTER_SKIN_GEOMETRY_BYTES} bytes"
        ));
    }
    for model in [CLASSIC_SKIN_GEOMETRY, SLIM_SKIN_GEOMETRY] {
        let patch = serde_json::json!({ "geometry": { "default": model } }).to_string();
        match parse_skin_geometry(&patch, &geometry) {
            Ok(Some(_)) => {}
            _ => return Err(format!("{model} does not resolve as skin geometry")),
        }
    }
    Ok(geometry)
}

/// Resolves a selected model's parents before moving legacy metadata into its description.
fn modern_skin_model(models: &Map<String, Value>, model: &str) -> Result<Value, String> {
    let mut chain = Vec::new();
    let mut legacy = false;
    let mut next = Some(model.to_owned());
    while let Some(wanted) = next.take() {
        if chain.len() >= MAX_INHERITANCE_DEPTH {
            return Err(format!("{model} inherits too deeply"));
        }
        if let Some(entry) = models
            .get("minecraft:geometry")
            .and_then(Value::as_array)
            .and_then(|entries| {
                entries
                    .iter()
                    .find(|entry| entry["description"]["identifier"].as_str() == Some(&wanted))
            })
        {
            chain.push(entry.clone());
            break;
        }
        let (key, value) = models
            .iter()
            .find(|(key, _)| {
                pack_compile_input::geometry_key::legacy_key(key)
                    .is_some_and(|(own, _)| own == wanted)
            })
            .ok_or_else(|| format!("{MODELS} has no {wanted}"))?;
        let mut description = value
            .as_object()
            .ok_or_else(|| format!("{wanted} is not a geometry object"))?
            .clone();
        legacy = true;
        let mut entry = Map::new();
        for field in ["bones", assets::entity::GEOMETRY_NO_HEAD_BOB_FIELD] {
            if let Some(value) = description.remove(field) {
                entry.insert(field.to_owned(), value);
            }
        }
        for (legacy, modern) in [
            ("texturewidth", "texture_width"),
            ("textureheight", "texture_height"),
        ] {
            if let Some(value) = description.remove(legacy) {
                description.insert(modern.to_owned(), value);
            }
        }
        description.insert("identifier".to_owned(), wanted.into());
        entry.insert("description".to_owned(), Value::Object(description));
        chain.push(Value::Object(entry));
        next = pack_compile_input::geometry_key::legacy_key(key)
            .and_then(|(_, parent)| parent.map(str::to_owned));
    }
    let mut resolved = chain.pop().expect("the selected model has a definition");
    if legacy {
        let mut base = serde_json::json!({"description":{}, "bones":[]});
        overlay_skin_model(&mut base, resolved)?;
        resolved = base;
    }
    while let Some(child) = chain.pop() {
        overlay_skin_model(&mut resolved, child)?;
    }
    resolved["description"]["identifier"] = model.into();
    if legacy && let Some(bones) = resolved["bones"].as_array_mut() {
        for bone in bones.iter_mut().filter_map(Value::as_object_mut) {
            bone.remove("reset");
        }
    }
    Ok(resolved)
}

/// Flattens a legacy child while preserving resolved cube defaults and inherited inflation.
fn overlay_skin_model(base: &mut Value, mut child: Value) -> Result<(), String> {
    let mut bones = base["bones"].as_array().cloned().unwrap_or_default();
    let child_bones = child["bones"].as_array().cloned().unwrap_or_default();
    for bone in bones.iter_mut() {
        resolve_cube_defaults(bone);
    }
    for mut bone in child_bones {
        resolve_cube_defaults(&mut bone);
        let Some(inherited) = bones.iter_mut().find(|inherited| {
            inherited["name"]
                .as_str()
                .zip(bone["name"].as_str())
                .is_some_and(|(parent, child)| parent.eq_ignore_ascii_case(child))
        }) else {
            bones.push(bone);
            continue;
        };
        let reset = bone["reset"].as_bool() == Some(true);
        let mut cubes = if reset {
            Vec::new()
        } else {
            inherited["cubes"].as_array().cloned().unwrap_or_default()
        };
        let inflate = bone["inflate"]
            .as_f64()
            .or_else(|| inherited["inflate"].as_f64());
        if let Some(inflate) = inflate.filter(|inflate| *inflate as f32 != 0.0) {
            for cube in &mut cubes {
                if let Some(cube) = cube.as_object_mut() {
                    cube.insert("inflate".to_owned(), inflate.into());
                }
            }
        }
        cubes.extend(bone["cubes"].as_array().into_iter().flatten().cloned());
        let inherited = inherited
            .as_object_mut()
            .ok_or_else(|| "invalid inherited skin bone".to_owned())?;
        // A redeclared bone defaults to visible, and reset also clears its mesh.
        inherited.remove("neverRender");
        if reset {
            inherited.remove("poly_mesh");
        }
        let mut properties = bone
            .as_object()
            .ok_or_else(|| "invalid child skin bone".to_owned())?
            .clone();
        // The skin reader keeps the first declaration's name and bind pose.
        properties.remove("name");
        properties.remove("bind_pose_rotation");
        inherited.extend(properties);
        inherited.insert("cubes".to_owned(), Value::Array(cubes));
    }
    let description = base["description"]
        .as_object_mut()
        .ok_or_else(|| "invalid skin geometry description".to_owned())?;
    let child_description = child["description"]
        .as_object()
        .ok_or_else(|| "invalid child skin geometry description".to_owned())?;
    // Bounds are one authored box; a child with partial bounds uses the box defaults.
    let bounds = [
        "visible_bounds_width",
        "visible_bounds_height",
        "visible_bounds_offset",
    ];
    if bounds
        .iter()
        .any(|field| child_description.contains_key(*field))
    {
        for field in bounds {
            description.remove(field);
        }
    }
    description.extend(child_description.clone());
    child
        .as_object_mut()
        .expect("model object")
        .remove("description");
    base.as_object_mut()
        .expect("model object")
        .extend(child.as_object().expect("model object").clone());
    base["bones"] = Value::Array(bones);
    Ok(())
}

/// Resolves authored cube defaults before inherited inflation is applied separately.
fn resolve_cube_defaults(bone: &mut Value) {
    let mirror = bone["mirror"].as_bool().unwrap_or(false);
    let inflate = bone["inflate"].as_f64().unwrap_or(0.0);
    if let Some(cubes) = bone.get_mut("cubes").and_then(Value::as_array_mut) {
        for cube in cubes {
            if let Some(cube) = cube.as_object_mut() {
                cube.entry("mirror").or_insert_with(|| mirror.into());
                cube.entry("inflate").or_insert_with(|| inflate.into());
            }
        }
    }
}

/// Uses the entity compiler's identifier selection, with standalone models above legacy models.
fn pack_skin_geometry(pack: &Path) -> Result<String, String> {
    let mut documents = Vec::new();
    let legacy = pack.join(MODELS);
    if legacy.is_file() {
        documents.push((MODELS.into(), read_bounded(&legacy, MAX_MODELS_BYTES)?));
    }
    let mut paths = Vec::new();
    crate::entity::collect::collect_optional_family(pack, "models/entity", &["json"], &mut paths)
        .map_err(|error| error.to_string())?;
    for (relative, path) in paths {
        documents.push((relative, read_bounded(&path, MAX_MODELS_BYTES)?));
    }
    let mut models = Map::new();
    let mut modern = Vec::new();
    for (_, bytes) in crate::select_entity_geometry(documents) {
        let mut document: Map<String, Value> =
            pack_json::parse_jsonc(&bytes).map_err(|error| error.to_string())?;
        if let Some(Value::Array(entries)) = document.remove("minecraft:geometry") {
            modern.extend(entries);
        } else {
            models.extend(document);
        }
    }
    if !modern.is_empty() {
        models.insert("minecraft:geometry".to_owned(), Value::Array(modern));
    }
    skin_geometry(&models)
}

/// Reads the starter skins and humanoid skin models from `pack` and encodes the MCBESKN1 carrier.
pub fn compile_starter_skins(pack: &Path) -> Result<Vec<u8>, String> {
    let skins = STARTER_SKIN_SOURCES
        .iter()
        .map(|source| {
            Ok(StarterSkin {
                name: source.name.into(),
                slim: source.slim,
                rgba8: read_skin(&pack.join(source.texture))?,
            })
        })
        .collect::<Result<_, String>>()?;
    let carrier = StarterSkins {
        geometry: pack_skin_geometry(pack)?.into(),
        skins,
    };
    encode_starter_skins(&carrier).map_err(|error| error.to_string())
}

/// Compiles the carrier and writes it to `out` through a temporary sibling file.
pub fn compile_starter_skins_to_file(pack: &Path, out: &Path) -> Result<(), String> {
    let blob = compile_starter_skins(pack)?;
    assets::write_blob_atomic(out, &blob).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Builds an original legacy fixture with one drawable body cube.
    fn cube_model() -> Value {
        serde_json::json!({
            "texturewidth": 64, "textureheight": 64,
            "bones": [{"name": "body", "pivot": [0, 24, 0],
                "cubes": [{"origin": [-4, 12, -2], "size": [8, 12, 4], "uv": [16, 16]}]}]
        })
    }

    /// Checks the serialized schema independently of the lenient skin geometry reader.
    fn assert_modern_skin_schema(data: &str) -> Vec<Value> {
        crate::entity::validate_geometry_document(&serde_json::from_str(data).unwrap()).unwrap();
        let root: Map<String, Value> = serde_json::from_str(data).unwrap();
        assert_eq!(root.len(), 2, "legacy models must not remain at the root");
        assert_eq!(root["format_version"], SKIN_GEOMETRY_FORMAT_VERSION);
        let entries = root["minecraft:geometry"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        for (entry, identifier) in entries
            .iter()
            .zip([CLASSIC_SKIN_GEOMETRY, SLIM_SKIN_GEOMETRY])
        {
            let description = entry["description"].as_object().unwrap();
            assert_eq!(description["identifier"], identifier);
            assert!(!description.contains_key("texturewidth"));
            assert!(!description.contains_key("textureheight"));
            for field in description.keys() {
                assert!(
                    entry.get(field).is_none(),
                    "metadata belongs in the description"
                );
            }
        }
        entries.clone()
    }

    #[test]
    fn legacy_skin_models_resolve_their_parents_into_one_modern_schema() {
        let models = serde_json::json!({
            "format_version": "1.8.0",
            "geometry.humanoid": cube_model(),
            "geometry.humanoid.custom:geometry.humanoid": {"bones": []},
            "geometry.humanoid.customSlim": cube_model(),
            "geometry.zombie": cube_model(),
        });
        let geometry = skin_geometry(models.as_object().unwrap()).unwrap();
        let entries = assert_modern_skin_schema(&geometry);
        for entry in entries {
            assert_eq!(entry["description"]["texture_width"], STARTER_SKIN_SIDE);
            assert_eq!(entry["description"]["texture_height"], STARTER_SKIN_SIDE);
            assert_eq!(entry["bones"][0]["cubes"].as_array().unwrap().len(), 1);
        }
    }

    #[test]
    fn a_missing_slim_model_fails_the_compile() {
        let models = serde_json::json!({
            "format_version": "1.8.0",
            "geometry.humanoid.custom": cube_model(),
        });
        assert!(skin_geometry(models.as_object().unwrap()).is_err());
    }

    #[test]
    fn inconsistent_schema_versions_fail_validation() {
        let document = serde_json::json!({
            "format_version":"1.8.0",
            "minecraft:geometry":[{
                "description":{"identifier":CLASSIC_SKIN_GEOMETRY},
                "bones":cube_model()["bones"],
            }],
            SLIM_SKIN_GEOMETRY:cube_model(),
        });
        assert!(crate::entity::validate_geometry_document(&document).is_err());
    }

    #[test]
    fn standalone_skin_geometry_overrides_the_legacy_inherited_model() {
        let directory = tempfile::tempdir().unwrap();
        let pack = directory.path();
        fs::create_dir_all(pack.join("models/entity")).unwrap();
        fs::create_dir_all(pack.join("textures/entity")).unwrap();
        let legacy = serde_json::json!({
            "format_version":"1.8.0",
            "geometry.parent":cube_model(),
            "geometry.humanoid.custom:geometry.parent":cube_model(),
            "geometry.humanoid.customSlim":cube_model()
        });
        fs::write(pack.join(MODELS), serde_json::to_vec(&legacy).unwrap()).unwrap();
        let modern = serde_json::json!({
            "format_version":"1.21.0",
            "minecraft:geometry":[{
                "description":{"identifier":CLASSIC_SKIN_GEOMETRY,"texture_width":64,"texture_height":64},
                "bones":[{"name":"body","cubes":[{
                    "origin":[-4,12,-2],"size":[8,12,4],"uv":[32,32],"inflate":0.25
                }]}]
            }]
        });
        fs::write(
            pack.join("models/entity/fixture.json"),
            serde_json::to_vec(&modern).unwrap(),
        )
        .unwrap();
        for skin in STARTER_SKIN_SOURCES {
            ::image::RgbaImage::from_pixel(64, 64, ::image::Rgba([33, 177, 91, 255]))
                .save(pack.join(skin.texture))
                .unwrap();
        }
        let out = pack.join("out.mcbeskn");
        let occupied = out.with_extension("tmp");
        fs::write(&occupied, b"another writer").unwrap();
        compile_starter_skins_to_file(pack, &out).unwrap();
        assert_eq!(fs::read(occupied).unwrap(), b"another writer");
        let carrier = assets::starter_skins::decode_starter_skins(&fs::read(out).unwrap()).unwrap();
        let entries = assert_modern_skin_schema(&carrier.geometry);
        assert_eq!(entries[0], modern["minecraft:geometry"][0]);
        assert_eq!(
            entries[1]["description"]["texture_width"],
            STARTER_SKIN_SIDE
        );
        assert_eq!(
            entries[1]["description"]["texture_height"],
            STARTER_SKIN_SIDE
        );
        let patch = serde_json::json!({"geometry":{"default":CLASSIC_SKIN_GEOMETRY}}).to_string();
        let model = parse_skin_geometry(&patch, &carrier.geometry)
            .unwrap()
            .unwrap();
        assert_eq!(
            model.bones[0].cubes.len(),
            1,
            "the selected modern model has no inherited coplanar cube"
        );
        assert_eq!(model.bones[0].cubes[0].inflate.get(), 0.25);
        assert_eq!(
            model.bones[0].cubes[0].uv,
            assets::EntityGeometryUv::Box([assets::EntityGeometryScalar::new(32.0).unwrap(); 2])
        );
        let patch = serde_json::json!({"geometry":{"default":SLIM_SKIN_GEOMETRY}}).to_string();
        assert_eq!(
            parse_skin_geometry(&patch, &carrier.geometry)
                .unwrap()
                .unwrap()
                .bones,
            parse_skin_geometry(&patch, &legacy.to_string())
                .unwrap()
                .unwrap()
                .bones,
        );
    }

    #[test]
    fn standalone_models_without_a_legacy_document_keep_a_supported_version() {
        let directory = tempfile::tempdir().unwrap();
        let pack = directory.path();
        fs::create_dir_all(pack.join("models/entity")).unwrap();
        let entries = [CLASSIC_SKIN_GEOMETRY, SLIM_SKIN_GEOMETRY].map(|identifier| {
            serde_json::json!({
                "description":{"identifier":identifier,"texture_width":64,"texture_height":64},
                "bones":cube_model()["bones"],
            })
        });
        let document = serde_json::json!({
            "format_version":"1.21.0", "minecraft:geometry":entries,
        });
        fs::write(
            pack.join("models/entity/fixture.json"),
            serde_json::to_vec(&document).unwrap(),
        )
        .unwrap();
        assert_eq!(
            assert_modern_skin_schema(&pack_skin_geometry(pack).unwrap()),
            document["minecraft:geometry"].as_array().unwrap().clone(),
        );
    }

    #[test]
    fn normalized_inheritance_preserves_cube_defaults_reset_and_metadata() {
        let mut parent = cube_model();
        parent["bones"][0]["mirror"] = true.into();
        parent["bones"][0]["inflate"] = 0.5.into();
        parent["visible_bounds_width"] = 3.into();
        parent["visible_bounds_height"] = 4.into();
        parent["visible_bounds_offset"] = serde_json::json!([0, 2, 0]);
        let mut child = cube_model();
        child["bones"][0]["name"] = "BODY".into();
        child["bones"][0]["locators"] = serde_json::json!({"fixture":[1,2,3]});
        child["visible_bounds_height"] = 2.into();
        let mut reset = cube_model();
        reset["bones"][0]["reset"] = true.into();
        let models = serde_json::json!({
            "format_version":"1.8.0",
            "geometry.parent":parent,
            "geometry.humanoid.custom:geometry.parent":child,
            "geometry.humanoid.customSlim:geometry.humanoid.custom":reset,
        });
        let normalized = skin_geometry(models.as_object().unwrap()).unwrap();
        let entries = assert_modern_skin_schema(&normalized);
        assert_eq!(
            entries[0]["bones"][0]["locators"],
            child["bones"][0]["locators"]
        );
        for name in [CLASSIC_SKIN_GEOMETRY, SLIM_SKIN_GEOMETRY] {
            let patch = serde_json::json!({"geometry":{"default":name}}).to_string();
            let before = parse_skin_geometry(&patch, &models.to_string())
                .unwrap()
                .unwrap();
            let after = parse_skin_geometry(&patch, &normalized).unwrap().unwrap();
            assert_eq!(after.bones, before.bones);
            assert_eq!(after.texture_width, before.texture_width);
            assert_eq!(after.texture_height, before.texture_height);
            assert_eq!(after.visible_bounds, before.visible_bounds);
        }
    }

    #[test]
    fn normalized_inheritance_preserves_effective_cube_inflation() {
        for (parent_inflate, cube_inflate, child_inflate, expected) in [
            (None, None, Some(1.0), [1.0, 0.125]),
            (None, Some(0.25), Some(1.0), [1.0, 0.125]),
            (Some(0.5), Some(0.25), None, [0.5, 0.125]),
            (Some(0.5), Some(0.25), Some(0.0), [0.25, 0.125]),
            (None, None, Some(-0.25), [-0.25, 0.125]),
        ] {
            for repeated in [false, true] {
                for reset in [false, true] {
                    let mut parent = cube_model();
                    let parent_bone = &mut parent["bones"][0];
                    if let Some(inflate) = cube_inflate {
                        parent_bone["cubes"][0]["inflate"] = inflate.into();
                    }
                    if let Some(inflate) = parent_inflate {
                        parent_bone["inflate"] = inflate.into();
                    }
                    let mut child = cube_model()["bones"][0].clone();
                    child["name"] = "BODY".into();
                    child["reset"] = reset.into();
                    child["cubes"][0]["inflate"] = 0.125.into();
                    if let Some(inflate) = child_inflate {
                        child["inflate"] = inflate.into();
                    }
                    let mut models = serde_json::json!({
                        "format_version":"1.8.0",
                        "geometry.humanoid.customSlim:geometry.humanoid.custom":{
                            "bones":[{"name":"body"}]
                        },
                    });
                    if repeated {
                        parent["bones"].as_array_mut().unwrap().push(child);
                        models[CLASSIC_SKIN_GEOMETRY] = parent;
                    } else {
                        models["geometry.parent"] = parent;
                        models["geometry.humanoid.custom:geometry.parent"] =
                            serde_json::json!({"bones":[child]});
                    }
                    let normalized = skin_geometry(models.as_object().unwrap()).unwrap();
                    assert_modern_skin_schema(&normalized);
                    for identifier in [CLASSIC_SKIN_GEOMETRY, SLIM_SKIN_GEOMETRY] {
                        let patch =
                            serde_json::json!({"geometry":{"default":identifier}}).to_string();
                        let before = parse_skin_geometry(&patch, &models.to_string())
                            .unwrap()
                            .unwrap();
                        let after = parse_skin_geometry(&patch, &normalized).unwrap().unwrap();
                        if identifier == CLASSIC_SKIN_GEOMETRY {
                            let inflation = before.bones[0]
                                .cubes
                                .iter()
                                .map(|cube| cube.inflate.get())
                                .collect::<Vec<_>>();
                            assert_eq!(inflation, expected[usize::from(reset)..]);
                        }
                        assert_eq!(
                            after.bones, before.bones,
                            "{identifier}, parent={parent_inflate:?}, child={child_inflate:?}, \
                             repeated={repeated}, reset={reset}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn repeated_legacy_bones_preserve_reset_before_normalization() {
        for reset in [false, true] {
            let mut first = cube_model()["bones"][0].clone();
            first["mirror"] = true.into();
            first["inflate"] = 0.5.into();
            let mut repeated = cube_model()["bones"][0].clone();
            repeated["name"] = "BODY".into();
            repeated["reset"] = reset.into();
            repeated["cubes"][0]["uv"] = serde_json::json!([32, 32]);
            let mut classic = cube_model();
            classic["bones"] = serde_json::json!([first, repeated]);
            let models = serde_json::json!({
                "format_version":"1.8.0",
                CLASSIC_SKIN_GEOMETRY:classic,
                "geometry.humanoid.customSlim:geometry.humanoid.custom":{"bones":[]},
            });
            let normalized = skin_geometry(models.as_object().unwrap()).unwrap();
            assert_modern_skin_schema(&normalized);
            for identifier in [CLASSIC_SKIN_GEOMETRY, SLIM_SKIN_GEOMETRY] {
                let patch = serde_json::json!({"geometry":{"default":identifier}}).to_string();
                let before = parse_skin_geometry(&patch, &models.to_string())
                    .unwrap()
                    .unwrap();
                let after = parse_skin_geometry(&patch, &normalized).unwrap().unwrap();
                assert_eq!(before.bones[0].cubes.len(), if reset { 1 } else { 2 });
                assert_eq!(after.bones, before.bones, "{identifier}, reset={reset}");
            }
        }
    }

    #[test]
    fn legacy_bones_without_cubes_remain_valid_after_normalization() {
        let mut model = cube_model();
        model["bones"].as_array_mut().unwrap().insert(
            0,
            serde_json::json!({
                "name":"root", "pivot":[0,0,0],
            }),
        );
        model["bones"][1]["parent"] = "root".into();
        let models = serde_json::json!({
            "format_version":"1.8.0",
            CLASSIC_SKIN_GEOMETRY:model,
            SLIM_SKIN_GEOMETRY:model,
        });
        let normalized = skin_geometry(models.as_object().unwrap()).unwrap();
        assert_modern_skin_schema(&normalized);
        for identifier in [CLASSIC_SKIN_GEOMETRY, SLIM_SKIN_GEOMETRY] {
            let patch = serde_json::json!({"geometry":{"default":identifier}}).to_string();
            let before = parse_skin_geometry(&patch, &models.to_string())
                .unwrap()
                .unwrap();
            let after = parse_skin_geometry(&patch, &normalized).unwrap().unwrap();
            assert!(after.bones[0].cubes.is_empty());
            assert_eq!(after.bones, before.bones);
        }
    }

    #[test]
    fn selected_skin_models_preserve_authored_coincident_cubes() {
        let mut model = cube_model();
        let mut second = model["bones"][0]["cubes"][0].clone();
        second["uv"] = serde_json::json!([32, 32]);
        model["bones"][0]["cubes"]
            .as_array_mut()
            .unwrap()
            .push(second);
        let models = Map::from_iter([
            (CLASSIC_SKIN_GEOMETRY.to_owned(), model),
            (SLIM_SKIN_GEOMETRY.to_owned(), cube_model()),
        ]);
        let data = skin_geometry(&models).unwrap();
        let patch = serde_json::json!({"geometry":{"default":CLASSIC_SKIN_GEOMETRY}}).to_string();
        let geometry = parse_skin_geometry(&patch, &data).unwrap().unwrap();
        let cubes = &geometry.bones[0].cubes;
        assert_eq!(cubes.len(), 2);
        assert_eq!(cubes[0].origin, cubes[1].origin);
        assert_eq!(cubes[0].size, cubes[1].size);
        assert_ne!(cubes[0].uv, cubes[1].uv);
    }

    #[test]
    fn pinned_pack_compiles_when_available() {
        let pack = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join(assets::vanilla_source().resource_pack_dir());
        if !pack.join(MODELS).is_file() {
            eprintln!(
                "skipping pinned_pack_compiles_when_available: fixture unavailable; requires the pinned vanilla resource pack under .local"
            );
            return;
        }
        let decoded =
            assets::starter_skins::decode_starter_skins(&compile_starter_skins(&pack).unwrap())
                .unwrap();
        assert_eq!(decoded.skins.len(), STARTER_SKIN_SOURCES.len());
        assert_modern_skin_schema(&decoded.geometry);
    }
}
