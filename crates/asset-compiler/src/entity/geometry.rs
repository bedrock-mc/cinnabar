use std::{collections::BTreeMap, path::Path};

use assets::{
    AssetError, EntityAssetKind, EntityGeometryBone, EntityGeometryCube, EntityGeometryFaceUv,
    EntityGeometryFaceUvs, EntityGeometryScalar, EntityGeometryUv, MAX_ENTITY_GEOMETRIES,
    MAX_ENTITY_GEOMETRY_BONES, MAX_ENTITY_GEOMETRY_CUBES, MAX_ENTITY_GEOMETRY_NAME_BYTES,
    MAX_ENTITY_TEXTURE_DIMENSION,
};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{PendingGeometry, PendingSymbol, insert_symbol, invalid};

pub(super) fn parse_geometry(
    relative_path: &str,
    path: &Path,
    value: &Value,
    symbols: &mut BTreeMap<(EntityAssetKind, Box<str>, Box<str>), PendingSymbol>,
    geometry_payloads: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
) -> Result<(), AssetError> {
    let root = value
        .as_object()
        .ok_or_else(|| invalid(format!("invalid geometry root in {}", path.display())))?;
    let is_modern = root.contains_key("minecraft:geometry");
    let format_version = required_string(value, "format_version", path)?;
    let supported_version = if is_modern {
        matches!(
            format_version,
            "1.12.0" | "1.16.0" | "1.21.0" | "1.21.120" | "1.26.10"
        )
    } else {
        matches!(format_version, "1.8.0" | "1.10.0")
    };
    if !supported_version {
        return Err(invalid(format!(
            "unsupported entity geometry format version or schema branch in {}",
            path.display()
        )));
    }
    if is_modern {
        if root.len() != 2 {
            return Err(invalid(format!(
                "unknown modern geometry root field in {}",
                path.display()
            )));
        }
        let geometries = root
            .get("minecraft:geometry")
            .ok_or_else(|| invalid("missing modern entity geometry payload"))?
            .as_array()
            .ok_or_else(|| invalid(format!("invalid geometry array in {}", path.display())))?;
        for geometry in geometries {
            validate_object_fields(geometry, path, &["description", "bones"], &["description"])?;
            let description = geometry.get("description").ok_or_else(|| {
                invalid(format!(
                    "missing geometry description in {}",
                    path.display()
                ))
            })?;
            validate_object_fields(
                description,
                path,
                &[
                    "identifier",
                    "texture_width",
                    "texture_height",
                    "visible_bounds_width",
                    "visible_bounds_height",
                    "visible_bounds_offset",
                ],
                &["identifier"],
            )?;
            let identifier = required_string(description, "identifier", path)?;
            let texture_width = optional_texture_dimension(description, "texture_width", path)?;
            let texture_height = optional_texture_dimension(description, "texture_height", path)?;
            let bones = parse_geometry_bones(geometry.get("bones"), path)?;
            insert_symbol(
                symbols,
                EntityAssetKind::Geometry,
                identifier,
                relative_path,
                Box::new([]),
            )?;
            insert_geometry(
                geometry_payloads,
                identifier,
                None,
                relative_path,
                geometry,
                texture_width,
                texture_height,
                bones,
            )?;
        }
    } else {
        let identifiers = root
            .keys()
            .filter(|field| field.as_str() != "format_version")
            .collect::<Vec<_>>();
        if identifiers.is_empty()
            || identifiers
                .iter()
                .any(|identifier| !identifier.starts_with("geometry."))
        {
            return Err(invalid(format!(
                "unknown legacy geometry root field in {}",
                path.display()
            )));
        }
        for raw_identifier in identifiers {
            let raw_identifier = raw_identifier.as_str();
            let (identifier, inherits) = match raw_identifier.split_once(':') {
                Some((identifier, inherits))
                    if !identifier.is_empty()
                        && !inherits.is_empty()
                        && !inherits.contains(':')
                        && identifier.starts_with("geometry.")
                        && inherits.starts_with("geometry.") =>
                {
                    (identifier, Some(inherits))
                }
                Some(_) => return Err(invalid("invalid legacy entity geometry inheritance key")),
                None => (raw_identifier, None),
            };
            let geometry = root
                .get(raw_identifier)
                .ok_or_else(|| invalid("missing legacy entity geometry"))?;
            validate_object_fields(
                geometry,
                path,
                &[
                    "texturewidth",
                    "textureheight",
                    "visible_bounds_width",
                    "visible_bounds_height",
                    "visible_bounds_offset",
                    "bones",
                ],
                &[],
            )?;
            let texture_width = optional_texture_dimension(geometry, "texturewidth", path)?;
            let texture_height = optional_texture_dimension(geometry, "textureheight", path)?;
            let bones = parse_geometry_bones_with_inheritance(
                geometry.get("bones"),
                path,
                inherits.is_some(),
            )?;
            insert_symbol(
                symbols,
                EntityAssetKind::Geometry,
                identifier,
                relative_path,
                Box::new([]),
            )?;
            insert_geometry(
                geometry_payloads,
                identifier,
                inherits,
                relative_path,
                geometry,
                texture_width,
                texture_height,
                bones,
            )?;
        }
    }
    Ok(())
}

#[allow(
    clippy::too_many_arguments,
    reason = "one bounded geometry record is assembled here"
)]
fn insert_geometry(
    geometries: &mut BTreeMap<(Box<str>, Box<str>), PendingGeometry>,
    identifier: &str,
    inherits: Option<&str>,
    source_path: &str,
    source_geometry: &Value,
    texture_width: Option<u16>,
    texture_height: Option<u16>,
    bones: Box<[EntityGeometryBone]>,
) -> Result<(), AssetError> {
    let identifier: Box<str> = identifier.into();
    let source_path: Box<str> = source_path.into();
    let geometry = PendingGeometry {
        identifier: identifier.clone(),
        semantic_sha256: Sha256::digest(
            serde_json::to_vec(source_geometry)
                .map_err(|_| invalid("failed to canonicalize entity geometry"))?,
        )
        .into(),
        inherits: inherits.map(Into::into),
        source_path: source_path.clone(),
        texture_width,
        texture_height,
        bones,
    };
    if geometries
        .insert((identifier.clone(), source_path), geometry)
        .is_some()
    {
        return Err(invalid(format!(
            "duplicate entity geometry payload `{identifier}` within one source"
        )));
    }
    if geometries.len() > MAX_ENTITY_GEOMETRIES {
        return Err(invalid("entity geometry count exceeds bound"));
    }
    Ok(())
}

fn parse_geometry_bones(
    value: Option<&Value>,
    path: &Path,
) -> Result<Box<[EntityGeometryBone]>, AssetError> {
    parse_geometry_bones_with_inheritance(value, path, false)
}

fn parse_geometry_bones_with_inheritance(
    value: Option<&Value>,
    path: &Path,
    allow_inherited_parent: bool,
) -> Result<Box<[EntityGeometryBone]>, AssetError> {
    let Some(value) = value else {
        return Ok(Box::new([]));
    };
    let bones = value.as_array().ok_or_else(|| {
        invalid(format!(
            "geometry bones must be an array in {}",
            path.display()
        ))
    })?;
    if bones.len() > MAX_ENTITY_GEOMETRY_BONES {
        return Err(invalid("entity geometry bone count exceeds bound"));
    }
    let mut parsed = Vec::with_capacity(bones.len());
    let mut total_cubes = 0usize;
    for bone in bones {
        validate_object_fields(
            bone,
            path,
            &[
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
            ],
            &["name"],
        )?;
        validate_known_deferred_bone_fields(bone, path)?;
        let name = required_string(bone, "name", path)?;
        let parent = optional_string(bone, "parent", path)?
            .filter(|parent| !parent.eq_ignore_ascii_case(name))
            .map(Into::into);
        let pivot = optional_vec(bone, "pivot", path)?;
        let rotation = optional_vec(bone, "rotation", path)?;
        let mirror = optional_bool(bone, "mirror", path)?;
        let inflate = optional_scalar(bone, "inflate", path)?;
        let never_render = optional_bool(bone, "neverRender", path)?;
        let reset = optional_bool(bone, "reset", path)?;
        let cubes = bone
            .get("cubes")
            .map(|cubes| {
                parse_geometry_cubes(
                    cubes,
                    path,
                    mirror.unwrap_or(false),
                    inflate.unwrap_or_else(zero_scalar),
                )
            })
            .transpose()?
            .unwrap_or_default();
        total_cubes = total_cubes
            .checked_add(cubes.len())
            .ok_or_else(|| invalid("entity geometry cube count overflow"))?;
        if total_cubes > MAX_ENTITY_GEOMETRY_CUBES {
            return Err(invalid("entity geometry cube count exceeds bound"));
        }
        parsed.push(EntityGeometryBone {
            name: name.into(),
            parent,
            pivot,
            rotation,
            mirror,
            inflate,
            never_render,
            reset,
            cubes,
        });
    }
    validate_bone_hierarchy(&parsed, path, allow_inherited_parent)?;
    Ok(parsed.into_boxed_slice())
}

fn validate_bone_hierarchy(
    bones: &[EntityGeometryBone],
    path: &Path,
    allow_inherited_parent: bool,
) -> Result<(), AssetError> {
    for bone in bones {
        if bone.name.is_empty()
            || bone.name.len() > MAX_ENTITY_GEOMETRY_NAME_BYTES
            || bone.name.chars().any(char::is_control)
        {
            return Err(invalid(format!(
                "invalid entity geometry bone name in {}",
                path.display()
            )));
        }
        if let Some(parent) = &bone.parent {
            let invalid_parent = parent.is_empty()
                || parent.len() > MAX_ENTITY_GEOMETRY_NAME_BYTES
                || parent.chars().any(char::is_control)
                || parent.eq_ignore_ascii_case(&bone.name)
                || (!allow_inherited_parent
                    && !bones
                        .iter()
                        .any(|candidate| candidate.name.eq_ignore_ascii_case(parent)));
            if invalid_parent {
                return Err(invalid(format!(
                    "invalid entity geometry bone parent in {}",
                    path.display()
                )));
            }
        }
    }
    for start in 0..bones.len() {
        let mut current = Some(start);
        for step in 0..=bones.len() {
            let Some(index) = current else {
                break;
            };
            if step == bones.len() {
                return Err(invalid(format!(
                    "entity geometry bone hierarchy contains a cycle in {}",
                    path.display()
                )));
            }
            current = bones[index].parent.as_ref().and_then(|parent| {
                bones
                    .iter()
                    .position(|candidate| candidate.name.eq_ignore_ascii_case(parent))
            });
        }
    }
    Ok(())
}

fn parse_geometry_cubes(
    value: &Value,
    path: &Path,
    bone_mirror: bool,
    bone_inflate: EntityGeometryScalar,
) -> Result<Box<[EntityGeometryCube]>, AssetError> {
    let cubes = value.as_array().ok_or_else(|| {
        invalid(format!(
            "geometry cubes must be an array in {}",
            path.display()
        ))
    })?;
    if cubes.len() > MAX_ENTITY_GEOMETRY_CUBES {
        return Err(invalid("entity geometry cube count exceeds bound"));
    }
    cubes
        .iter()
        .map(|cube| {
            validate_object_fields(
                cube,
                path,
                &[
                    "origin", "size", "pivot", "rotation", "uv", "inflate", "mirror",
                ],
                &["origin", "size"],
            )?;
            let size = required_vec(cube, "size", path)?;
            if size.iter().any(|value| value.get() < 0.0) {
                return Err(invalid("entity geometry cube size is negative"));
            }
            Ok(EntityGeometryCube {
                origin: required_vec(cube, "origin", path)?,
                size,
                pivot: optional_vec(cube, "pivot", path)?.unwrap_or_else(zero_vec3),
                rotation: optional_vec(cube, "rotation", path)?.unwrap_or_else(zero_vec3),
                uv: cube
                    .get("uv")
                    .map(|uv| parse_geometry_uv(uv, path))
                    .transpose()?
                    .unwrap_or_else(|| EntityGeometryUv::Box(zero_vec2())),
                inflate: optional_scalar(cube, "inflate", path)?.unwrap_or(bone_inflate),
                mirror: optional_bool(cube, "mirror", path)?.unwrap_or(bone_mirror),
            })
        })
        .collect::<Result<Vec<_>, AssetError>>()
        .map(Vec::into_boxed_slice)
}

fn parse_geometry_uv(value: &Value, path: &Path) -> Result<EntityGeometryUv, AssetError> {
    if value.is_array() {
        return Ok(EntityGeometryUv::Box(parse_vec(value, "uv", path)?));
    }
    let object = value.as_object().ok_or_else(|| {
        invalid(format!(
            "entity geometry UV must be an array or face object in {}",
            path.display()
        ))
    })?;
    const FACE_NAMES: [&str; 6] = ["north", "south", "east", "west", "up", "down"];
    if object.is_empty()
        || object
            .keys()
            .any(|field| !FACE_NAMES.contains(&field.as_str()))
    {
        return Err(invalid("unknown or empty entity geometry face UV map"));
    }
    let parse_face = |name: &str| -> Result<Option<EntityGeometryFaceUv>, AssetError> {
        object
            .get(name)
            .map(|face| {
                validate_object_fields(face, path, &["uv", "uv_size"], &["uv"])?;
                Ok(EntityGeometryFaceUv {
                    uv: required_vec(face, "uv", path)?,
                    uv_size: optional_vec(face, "uv_size", path)?,
                })
            })
            .transpose()
    };
    Ok(EntityGeometryUv::Faces(EntityGeometryFaceUvs {
        north: parse_face("north")?,
        south: parse_face("south")?,
        east: parse_face("east")?,
        west: parse_face("west")?,
        up: parse_face("up")?,
        down: parse_face("down")?,
    }))
}

fn validate_object_fields(
    value: &Value,
    path: &Path,
    allowed: &[&str],
    required: &[&str],
) -> Result<(), AssetError> {
    let object = value.as_object().ok_or_else(|| {
        invalid(format!(
            "entity geometry value must be an object in {}",
            path.display()
        ))
    })?;
    if object
        .keys()
        .any(|field| !allowed.contains(&field.as_str()))
        || required.iter().any(|field| !object.contains_key(*field))
    {
        return Err(invalid(format!(
            "unknown or missing entity geometry field in {}",
            path.display()
        )));
    }
    Ok(())
}

fn required_string<'a>(value: &'a Value, field: &str, path: &Path) -> Result<&'a str, AssetError> {
    value.get(field).and_then(Value::as_str).ok_or_else(|| {
        invalid(format!(
            "entity geometry field `{field}` must be a string in {}",
            path.display()
        ))
    })
}

fn optional_string<'a>(
    value: &'a Value,
    field: &str,
    path: &Path,
) -> Result<Option<&'a str>, AssetError> {
    value
        .get(field)
        .map(|value| {
            value.as_str().ok_or_else(|| {
                invalid(format!(
                    "entity geometry field `{field}` must be a string in {}",
                    path.display()
                ))
            })
        })
        .transpose()
}

fn optional_texture_dimension(
    value: &Value,
    field: &str,
    path: &Path,
) -> Result<Option<u16>, AssetError> {
    let Some(raw) = value.get(field) else {
        return Ok(None);
    };
    let number = raw.as_f64().ok_or_else(|| {
        invalid(format!(
            "entity geometry texture dimension `{field}` must be numeric in {}",
            path.display()
        ))
    })?;
    if !number.is_finite()
        || number.fract() != 0.0
        || number < 1.0
        || number > f64::from(MAX_ENTITY_TEXTURE_DIMENSION)
    {
        return Err(invalid("entity geometry texture dimension exceeds bound"));
    }
    Ok(Some(number as u16))
}

fn scalar(value: &Value, field: &str, path: &Path) -> Result<EntityGeometryScalar, AssetError> {
    let number = value.as_f64().ok_or_else(|| {
        invalid(format!(
            "entity geometry scalar `{field}` must be numeric in {}",
            path.display()
        ))
    })?;
    EntityGeometryScalar::new(number as f32)
        .ok_or_else(|| invalid(format!("entity geometry scalar `{field}` exceeds bound")))
}

fn required_vec<const N: usize>(
    value: &Value,
    field: &str,
    path: &Path,
) -> Result<[EntityGeometryScalar; N], AssetError> {
    let value = value.get(field).ok_or_else(|| {
        invalid(format!(
            "missing entity geometry vector `{field}` in {}",
            path.display()
        ))
    })?;
    parse_vec(value, field, path)
}

fn optional_vec<const N: usize>(
    value: &Value,
    field: &str,
    path: &Path,
) -> Result<Option<[EntityGeometryScalar; N]>, AssetError> {
    value
        .get(field)
        .map(|value| parse_vec(value, field, path))
        .transpose()
}

fn parse_vec<const N: usize>(
    value: &Value,
    field: &str,
    path: &Path,
) -> Result<[EntityGeometryScalar; N], AssetError> {
    let values = value.as_array().ok_or_else(|| {
        invalid(format!(
            "entity geometry vector `{field}` must be an array in {}",
            path.display()
        ))
    })?;
    if values.len() != N {
        return Err(invalid(format!(
            "entity geometry vector `{field}` has the wrong length"
        )));
    }
    let parsed = values
        .iter()
        .map(|value| scalar(value, field, path))
        .collect::<Result<Vec<_>, _>>()?;
    parsed
        .try_into()
        .map_err(|_| invalid("entity geometry vector has the wrong length"))
}

fn optional_scalar(
    value: &Value,
    field: &str,
    path: &Path,
) -> Result<Option<EntityGeometryScalar>, AssetError> {
    value
        .get(field)
        .map(|value| scalar(value, field, path))
        .transpose()
}

fn optional_bool(value: &Value, field: &str, path: &Path) -> Result<Option<bool>, AssetError> {
    value
        .get(field)
        .map(|value| {
            value.as_bool().ok_or_else(|| {
                invalid(format!(
                    "entity geometry field `{field}` must be boolean in {}",
                    path.display()
                ))
            })
        })
        .transpose()
}

fn zero_scalar() -> EntityGeometryScalar {
    EntityGeometryScalar::new(0.0).expect("zero is a canonical geometry scalar")
}

fn zero_vec3() -> [EntityGeometryScalar; 3] {
    [zero_scalar(); 3]
}

fn zero_vec2() -> [EntityGeometryScalar; 2] {
    [zero_scalar(); 2]
}

fn validate_known_deferred_bone_fields(value: &Value, path: &Path) -> Result<(), AssetError> {
    if value
        .get("locators")
        .is_some_and(|value| !value.is_object())
        || value
            .get("texture_meshes")
            .is_some_and(|value| !value.is_array())
    {
        return Err(invalid(format!(
            "invalid deferred entity geometry object in {}",
            path.display()
        )));
    }
    optional_string(value, "binding", path)?;
    optional_bool(value, "neverRender", path)?;
    optional_bool(value, "reset", path)?;
    let _: Option<[EntityGeometryScalar; 3]> = optional_vec(value, "bind_pose_rotation", path)?;
    Ok(())
}
