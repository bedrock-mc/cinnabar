//! Lenient normalisation of a server pack's geometry JSON to the schema the entity compile
//! accepts: unknown fields and malformed optional values are dropped, negative cube sizes
//! are flipped, and unsupported format versions are read as the nearest supported one.

use serde_json::{Map, Value};

const MODERN_VERSIONS: [&str; 5] = ["1.12.0", "1.16.0", "1.21.0", "1.21.120", "1.26.10"];
const LEGACY_VERSIONS: [&str; 2] = ["1.8.0", "1.10.0"];
const BONE_FIELDS: [&str; 13] = [
    "name",
    "parent",
    "pivot",
    "rotation",
    "cubes",
    "mirror",
    "inflate",
    "locators",
    "binding",
    "texture_meshes",
    "neverRender",
    "reset",
    "bind_pose_rotation",
];
const CUBE_FIELDS: [&str; 7] = [
    "origin", "size", "pivot", "rotation", "uv", "inflate", "mirror",
];
const FACES: [&str; 6] = ["north", "south", "east", "west", "up", "down"];

fn keep_keys(object: &mut Map<String, Value>, allowed: &[&str]) {
    object.retain(|key, _| allowed.contains(&key.as_str()));
}

fn numbers<const N: usize>(value: &Value) -> Option<[f64; N]> {
    let items = value.as_array()?;
    if items.len() != N {
        return None;
    }
    let mut out = [0.0; N];
    for (slot, item) in out.iter_mut().zip(items) {
        *slot = item.as_f64().filter(|number| number.is_finite())?;
    }
    Some(out)
}

fn drop_unless(object: &mut Map<String, Value>, key: &str, valid: impl Fn(&Value) -> bool) {
    if object.get(key).is_some_and(|value| !valid(value)) {
        object.remove(key);
    }
}

fn is_dimension(value: &Value) -> bool {
    value
        .as_f64()
        .is_some_and(|number| number.fract() == 0.0 && (1.0..=16384.0).contains(&number))
}

fn sanitize_cube(cube: &mut Value) -> bool {
    let Some(object) = cube.as_object_mut() else {
        return false;
    };
    keep_keys(object, &CUBE_FIELDS);
    let (Some(mut origin), Some(mut size)) = (
        object.get("origin").and_then(numbers::<3>),
        object.get("size").and_then(numbers::<3>),
    ) else {
        return false;
    };
    for axis in 0..3 {
        if size[axis] < 0.0 {
            origin[axis] += size[axis];
            size[axis] = -size[axis];
        }
    }
    object.insert("origin".into(), origin.to_vec().into());
    object.insert("size".into(), size.to_vec().into());
    drop_unless(object, "pivot", |value| numbers::<3>(value).is_some());
    drop_unless(object, "rotation", |value| numbers::<3>(value).is_some());
    drop_unless(object, "inflate", Value::is_number);
    drop_unless(object, "mirror", Value::is_boolean);
    if let Some(uv) = object.get_mut("uv") {
        let keep = match uv {
            Value::Array(_) => numbers::<2>(uv).is_some(),
            Value::Object(faces) => {
                faces.retain(|name, face| FACES.contains(&name.as_str()) && sanitize_face(face));
                !faces.is_empty()
            }
            _ => false,
        };
        if !keep {
            object.remove("uv");
        }
    }
    true
}

fn sanitize_face(face: &mut Value) -> bool {
    let Some(object) = face.as_object_mut() else {
        return false;
    };
    keep_keys(object, &["uv", "uv_size"]);
    drop_unless(object, "uv_size", |value| numbers::<2>(value).is_some());
    object
        .get("uv")
        .is_some_and(|uv| numbers::<2>(uv).is_some())
}

/// `inherits` keeps parents that name bones of the parent geometry.
fn sanitize_bones(bones: &mut Value, inherits: bool) {
    let Some(list) = bones.as_array_mut() else {
        *bones = Value::Array(Vec::new());
        return;
    };
    list.retain_mut(|bone| {
        let Some(object) = bone.as_object_mut() else {
            return false;
        };
        keep_keys(object, &BONE_FIELDS);
        if object
            .get("name")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            return false;
        }
        drop_unless(object, "parent", Value::is_string);
        for vector in ["pivot", "rotation", "bind_pose_rotation"] {
            drop_unless(object, vector, |value| numbers::<3>(value).is_some());
        }
        for flag in ["mirror", "neverRender", "reset"] {
            drop_unless(object, flag, Value::is_boolean);
        }
        drop_unless(object, "inflate", Value::is_number);
        drop_unless(object, "binding", Value::is_string);
        drop_unless(object, "locators", Value::is_object);
        drop_unless(object, "texture_meshes", Value::is_array);
        if let Some(cubes) = object.get_mut("cubes") {
            match cubes.as_array_mut() {
                Some(cubes) => cubes.retain_mut(sanitize_cube),
                None => {
                    object.remove("cubes");
                }
            }
        }
        true
    });
    if !inherits {
        let names = list
            .iter()
            .filter_map(|bone| Some(bone["name"].as_str()?.to_ascii_lowercase()))
            .collect::<Vec<_>>();
        for bone in list.iter_mut().filter_map(Value::as_object_mut) {
            let orphan = bone
                .get("parent")
                .and_then(Value::as_str)
                .is_some_and(|parent| !names.contains(&parent.to_ascii_lowercase()));
            if orphan {
                bone.remove("parent");
            }
        }
    }
}

fn sanitize_dimensions(object: &mut Map<String, Value>, keys: &[&str]) {
    for key in keys {
        drop_unless(object, key, is_dimension);
    }
}

/// Rewrites `root` in place so `parse_geometry` accepts it; `false` if nothing usable is left.
pub(super) fn sanitize_geometry(root: &mut Value) -> bool {
    let Some(object) = root.as_object_mut() else {
        return false;
    };
    let modern = object.contains_key("minecraft:geometry");
    let supported: &[&str] = if modern {
        &MODERN_VERSIONS
    } else {
        &LEGACY_VERSIONS
    };
    let version_ok = object
        .get("format_version")
        .and_then(Value::as_str)
        .is_some_and(|version| supported.contains(&version));
    if !version_ok {
        object.insert("format_version".into(), supported[0].into());
    }
    if modern {
        keep_keys(object, &["format_version", "minecraft:geometry"]);
        let Some(entries) = object
            .get_mut("minecraft:geometry")
            .and_then(Value::as_array_mut)
        else {
            return false;
        };
        entries.retain_mut(|entry| {
            let Some(geometry) = entry.as_object_mut() else {
                return false;
            };
            keep_keys(geometry, &["description", "bones"]);
            let Some(description) = geometry
                .get_mut("description")
                .and_then(Value::as_object_mut)
            else {
                return false;
            };
            keep_keys(
                description,
                &[
                    "identifier",
                    "texture_width",
                    "texture_height",
                    "visible_bounds_width",
                    "visible_bounds_height",
                    "visible_bounds_offset",
                ],
            );
            sanitize_dimensions(description, &["texture_width", "texture_height"]);
            if description
                .get("identifier")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
            {
                return false;
            }
            if let Some(bones) = geometry.get_mut("bones") {
                sanitize_bones(bones, false);
            }
            true
        });
        return !entries.is_empty();
    }
    object.retain(|key, _| key == "format_version" || key.starts_with("geometry."));
    for (key, geometry) in object
        .iter_mut()
        .filter(|(key, _)| *key != "format_version")
    {
        let inherits = key.contains(':');
        let Some(geometry) = geometry.as_object_mut() else {
            return false;
        };
        keep_keys(
            geometry,
            &[
                "texturewidth",
                "textureheight",
                "visible_bounds_width",
                "visible_bounds_height",
                "visible_bounds_offset",
                "bones",
            ],
        );
        sanitize_dimensions(geometry, &["texturewidth", "textureheight"]);
        if let Some(bones) = geometry.get_mut("bones") {
            sanitize_bones(bones, inherits);
        }
    }
    object.len() > 1
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::sanitize_geometry;

    // Unknown fields drop, a negative cube flips, and an odd version reads as supported.
    #[test]
    fn geometry_is_normalised_to_the_accepted_schema() {
        let mut root = json!({
            "format_version": "1.14.0",
            "minecraft:geometry": [{
                "description": {"identifier": "geometry.a", "extra": 1},
                "bones": [{"name": "b", "poly_mesh": {}, "cubes": [
                    {"origin": [0, 0, 0], "size": [-2, 4, 4], "uv": {}},
                    {"origin": [0], "size": [1, 1, 1]}
                ]}],
                "unknown": true
            }],
            "other": 1
        });
        assert!(sanitize_geometry(&mut root));
        assert_eq!(root["format_version"], "1.12.0");
        let cubes = &root["minecraft:geometry"][0]["bones"][0]["cubes"];
        assert_eq!(cubes.as_array().unwrap().len(), 1);
        assert_eq!(cubes[0]["origin"], json!([-2.0, 0.0, 0.0]));
        assert_eq!(cubes[0]["size"], json!([2.0, 4.0, 4.0]));
        assert!(cubes[0].get("uv").is_none());
        assert!(root.get("other").is_none());
    }
}
