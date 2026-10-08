//! Custom player-skin geometry: the geometry JSON a serialized skin carries, resolved through
//! the skin's resource patch into one model. Parsing is lenient (remote data): unknown fields are
//! ignored and malformed cubes are dropped; only an unusable model is rejected.
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

mod bounds;
mod poly_mesh;
pub use bounds::SkinGeometryBounds;
pub use poly_mesh::{SkinPolyMesh, SkinPolyVertex};

use crate::{
    EntityGeometryBone, EntityGeometryCube, EntityGeometryFaceUv, EntityGeometryFaceUvs,
    EntityGeometryScalar, EntityGeometryUv, MAX_ENTITY_TEXTURE_DIMENSION,
};

/// Bones one skin model may have; the actor renderer's per-rig bone bound.
pub const MAX_SKIN_GEOMETRY_BONES: usize = 96;
/// Cubes one skin model may have.
pub const MAX_SKIN_GEOMETRY_CUBES: usize = 2048;
/// Vertices accepted by a skin model and the actor geometry catalog.
pub const MAX_SKIN_GEOMETRY_VERTICES: usize = 1_048_576;
/// Inheritance links followed before a chain is treated as cyclic.
const MAX_INHERITANCE_DEPTH: usize = 8;

/// One resolved skin model; UVs address a `texture_width` x `texture_height` image.
#[derive(Clone, Debug, PartialEq)]
pub struct SkinGeometry {
    pub identifier: Box<str>,
    pub texture_width: u16,
    pub texture_height: u16,
    /// Bones after inheritance, parents resolved by name (an unknown parent becomes a root).
    pub bones: Box<[EntityGeometryBone]>,
    /// Optional polygon meshes in the same order as `bones`.
    pub poly_meshes: Box<[Option<SkinPolyMesh>]>,
    /// Authored visibility box in the actor coordinate frame, when supplied and finite.
    pub visible_bounds: Option<SkinGeometryBounds>,
    /// Digest of the model inputs, for caching built meshes.
    pub digest: [u8; 32],
}

impl SkinGeometry {
    /// Resolves a classic model from the catalog when a skin sends only its resource patch.
    pub fn from_catalog(geometry: &crate::EntityGeometry) -> Option<Self> {
        if geometry.inherits.is_some() {
            return None;
        }
        if geometry.bones.len() > MAX_SKIN_GEOMETRY_BONES
            || geometry
                .bones
                .iter()
                .map(|bone| bone.cubes.len())
                .sum::<usize>()
                > MAX_SKIN_GEOMETRY_CUBES
        {
            return None;
        }
        let source = serde_json::to_string(geometry).ok()?;
        let mut skin = finish(
            geometry.identifier.to_string(),
            Some(geometry.texture_width),
            Some(geometry.texture_height),
            geometry.bones.to_vec(),
            vec![None; geometry.bones.len()],
            "",
            &source,
        )
        .ok()?;
        skin.visible_bounds = geometry.visible_bounds.map(|bounds| bounds.as_bounds());
        Some(skin)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SkinGeometryError {
    Json,
    MissingGeometry,
    NoBones,
    TooManyBones,
    TooManyCubes,
    CyclicHierarchy,
}

/// The geometry the resource patch names as the skin's default model.
#[must_use]
pub fn skin_geometry_name(resource_patch: &str) -> Option<String> {
    geometry_name(resource_patch, "default")
}

/// Reads one geometry alias from a serialized skin's patch.
fn geometry_name(resource_patch: &str, key: &str) -> Option<String> {
    let patch: Value = serde_json::from_str(resource_patch).ok()?;
    patch
        .get("geometry")?
        .get(key)?
        .as_str()
        .map(str::to_ascii_lowercase)
}

/// Resolves the skin's model. `Ok(None)` means the default geometry applies: no geometry data
/// (vanilla then uses its skin pack's classic model) or no named geometry in the patch.
/// Inheritance resolves only within the skin's own geometries, as vanilla's per-skin group does.
pub fn parse_skin_geometry(
    resource_patch: &str,
    geometry_data: &str,
) -> Result<Option<SkinGeometry>, SkinGeometryError> {
    parse_skin_geometry_layer(resource_patch, geometry_data, "default")
}

/// Resolves an additional named geometry used by a persona animation layer.
pub fn parse_skin_geometry_layer(
    resource_patch: &str,
    geometry_data: &str,
    key: &str,
) -> Result<Option<SkinGeometry>, SkinGeometryError> {
    let Some(name) = geometry_name(resource_patch, key) else {
        return Ok(None);
    };
    let data = geometry_data.trim();
    if data.is_empty() || data == "null" {
        return Ok(None);
    }
    let root: Value = serde_json::from_str(data).map_err(|_| SkinGeometryError::Json)?;
    let geometries = parse_geometries(&root).ok_or(SkinGeometryError::Json)?;
    let own = |identifier: &str| {
        geometries
            .iter()
            .find(|entry| entry.identifier.eq_ignore_ascii_case(identifier))
    };
    // Walk the inheritance chain from the named geometry to its root.
    let mut chain = Vec::new();
    let mut current = Some(name.clone());
    while let Some(identifier) = current.take() {
        if chain.len() > MAX_INHERITANCE_DEPTH {
            return Err(SkinGeometryError::CyclicHierarchy);
        }
        let entry = own(&identifier).ok_or(SkinGeometryError::MissingGeometry)?;
        chain.push(entry);
        current.clone_from(&entry.inherits);
    }
    let (mut texture_width, mut texture_height) = (None, None);
    let mut visible_bounds = None;
    let mut bones: Vec<EntityGeometryBone> = Vec::new();
    let mut poly_meshes = Vec::new();
    let mut cube_count = 0;
    for entry in chain.iter().rev() {
        texture_width = entry.texture_width.or(texture_width);
        texture_height = entry.texture_height.or(texture_height);
        visible_bounds = entry.visible_bounds.or(visible_bounds);
        for (child, mesh) in entry.bones.iter().zip(&entry.poly_meshes) {
            match bones
                .iter()
                .position(|bone| bone.name.eq_ignore_ascii_case(&child.name))
            {
                Some(index) => {
                    let other_cubes = cube_count - bones[index].cubes.len();
                    cube_count = other_cubes
                        + overlay_bone(
                            &mut bones[index],
                            child,
                            MAX_SKIN_GEOMETRY_CUBES - other_cubes,
                        )?;
                    if mesh.is_some() || child.reset == Some(true) {
                        poly_meshes[index] = mesh.clone();
                    }
                }
                None => {
                    cube_count = cube_count
                        .checked_add(child.cubes.len())
                        .filter(|count| *count <= MAX_SKIN_GEOMETRY_CUBES)
                        .ok_or(SkinGeometryError::TooManyCubes)?;
                    if bones.len() >= MAX_SKIN_GEOMETRY_BONES {
                        return Err(SkinGeometryError::TooManyBones);
                    }
                    bones.push(child.clone());
                    poly_meshes.push(mesh.clone());
                }
            }
        }
    }
    finish(
        name,
        texture_width,
        texture_height,
        bones,
        poly_meshes,
        resource_patch,
        geometry_data,
    )
    .map(|mut geometry| {
        geometry.visible_bounds = visible_bounds;
        Some(geometry)
    })
}

fn finish(
    identifier: String,
    texture_width: Option<u16>,
    texture_height: Option<u16>,
    mut bones: Vec<EntityGeometryBone>,
    poly_meshes: Vec<Option<SkinPolyMesh>>,
    resource_patch: &str,
    geometry_data: &str,
) -> Result<SkinGeometry, SkinGeometryError> {
    if bones.is_empty() {
        return Err(SkinGeometryError::NoBones);
    }
    if bones.len() > MAX_SKIN_GEOMETRY_BONES {
        return Err(SkinGeometryError::TooManyBones);
    }
    if bones.iter().map(|bone| bone.cubes.len()).sum::<usize>() > MAX_SKIN_GEOMETRY_CUBES {
        return Err(SkinGeometryError::TooManyCubes);
    }
    let names: Vec<Box<str>> = bones.iter().map(|bone| bone.name.clone()).collect();
    for bone in &mut bones {
        let known = bone.parent.as_deref().is_some_and(|parent| {
            !parent.eq_ignore_ascii_case(&bone.name)
                && names.iter().any(|name| name.eq_ignore_ascii_case(parent))
        });
        if !known {
            bone.parent = None;
        }
    }
    for start in 0..bones.len() {
        let mut current = Some(start);
        for _ in 0..=bones.len() {
            current = current.and_then(|index| {
                let parent = bones[index].parent.as_deref()?;
                bones
                    .iter()
                    .position(|bone| bone.name.eq_ignore_ascii_case(parent))
            });
        }
        if current.is_some() {
            return Err(SkinGeometryError::CyclicHierarchy);
        }
    }
    let mut digest = Sha256::new();
    digest.update(identifier.as_bytes());
    digest.update([0]);
    digest.update(resource_patch.as_bytes());
    digest.update([0]);
    digest.update(geometry_data.as_bytes());
    Ok(SkinGeometry {
        identifier: identifier.into(),
        texture_width: texture_width.unwrap_or(64),
        texture_height: texture_height.unwrap_or(64),
        bones: bones.into(),
        poly_meshes: poly_meshes.into(),
        visible_bounds: None,
        digest: digest.finalize().into(),
    })
}

struct ParsedGeometry {
    identifier: String,
    inherits: Option<String>,
    texture_width: Option<u16>,
    texture_height: Option<u16>,
    bones: Vec<EntityGeometryBone>,
    poly_meshes: Vec<Option<SkinPolyMesh>>,
    visible_bounds: Option<SkinGeometryBounds>,
}

fn parse_geometries(root: &Value) -> Option<Vec<ParsedGeometry>> {
    let root = root.as_object()?;
    let mut parsed = Vec::new();
    if let Some(modern) = root.get("minecraft:geometry").and_then(Value::as_array) {
        for geometry in modern {
            let Some(description) = geometry.get("description") else {
                continue;
            };
            let Some(identifier) = description.get("identifier").and_then(Value::as_str) else {
                continue;
            };
            // Modern formats dropped inheritance; vanilla skips such an entry.
            if identifier.contains(':') {
                continue;
            }
            parsed.push(ParsedGeometry {
                identifier: identifier.to_owned(),
                inherits: None,
                texture_width: dimension(description.get("texture_width")),
                texture_height: dimension(description.get("texture_height")),
                bones: bones(geometry.get("bones")),
                poly_meshes: poly_mesh::bone_meshes(geometry.get("bones")),
                visible_bounds: bounds::parse(description),
            });
        }
    }
    for (key, geometry) in root {
        if !key.starts_with("geometry.") || !geometry.is_object() {
            continue;
        }
        let (identifier, inherits) = match key.split_once(':') {
            Some((identifier, inherits)) => (identifier, Some(inherits.to_owned())),
            None => (key.as_str(), None),
        };
        parsed.push(ParsedGeometry {
            identifier: identifier.to_owned(),
            inherits,
            texture_width: dimension(geometry.get("texturewidth")),
            texture_height: dimension(geometry.get("textureheight")),
            bones: bones(geometry.get("bones")),
            poly_meshes: poly_mesh::bone_meshes(geometry.get("bones")),
            visible_bounds: bounds::parse(geometry),
        });
    }
    Some(parsed)
}

fn dimension(value: Option<&Value>) -> Option<u16> {
    let value = value?.as_f64()?;
    (value.is_finite() && value >= 1.0 && value <= f64::from(MAX_ENTITY_TEXTURE_DIMENSION))
        .then_some(value as u16)
}

fn scalar(value: &Value) -> Option<EntityGeometryScalar> {
    EntityGeometryScalar::new(value.as_f64()? as f32)
}

fn vector<const N: usize>(value: Option<&Value>) -> Option<[EntityGeometryScalar; N]> {
    let values = value?.as_array()?;
    if values.len() != N {
        return None;
    }
    let parsed = values.iter().map(scalar).collect::<Option<Vec<_>>>()?;
    parsed.try_into().ok()
}

fn bones(value: Option<&Value>) -> Vec<EntityGeometryBone> {
    let Some(bones) = value.and_then(Value::as_array) else {
        return Vec::new();
    };
    bones
        .iter()
        .filter_map(|bone| {
            let bone = bone.as_object()?;
            let name = bone.get("name")?.as_str()?;
            if name.is_empty() || name.chars().any(char::is_control) {
                return None;
            }
            let mirror = bone.get("mirror").and_then(Value::as_bool);
            let inflate = bone.get("inflate").and_then(scalar);
            let cubes = bone
                .get("cubes")
                .and_then(Value::as_array)
                .map(|cubes| {
                    cubes
                        .iter()
                        .filter_map(|cube| {
                            cube_from(
                                cube.as_object()?,
                                mirror.unwrap_or(false),
                                inflate.unwrap_or(EntityGeometryScalar::ZERO),
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            Some(EntityGeometryBone {
                name: name.into(),
                parent: bone
                    .get("parent")
                    .and_then(Value::as_str)
                    .filter(|parent| !parent.is_empty())
                    .map(Into::into),
                pivot: vector(bone.get("pivot")),
                rotation: vector(bone.get("rotation")),
                bind_pose_rotation: vector(bone.get("bind_pose_rotation")),
                // Cubes carry the resolved mirror and inflate; the bone keeps none of its own.
                mirror: None,
                inflate: None,
                never_render: bone.get("neverRender").and_then(Value::as_bool),
                reset: bone.get("reset").and_then(Value::as_bool),
                binding: None,
                texture_meshes: Box::new([]),
                cubes: cubes.into(),
            })
        })
        .collect()
}

/// A drawable cube, or `None` when it is malformed or degenerate.
fn cube_from(
    cube: &Map<String, Value>,
    bone_mirror: bool,
    bone_inflate: EntityGeometryScalar,
) -> Option<EntityGeometryCube> {
    let origin = vector(cube.get("origin"))?;
    let size: [EntityGeometryScalar; 3] = vector(cube.get("size"))?;
    let inflate = cube.get("inflate").and_then(scalar).unwrap_or(bone_inflate);
    let zero_axes = size
        .iter()
        .filter(|value| value.get() + 2.0 * inflate.get() == 0.0)
        .count();
    if size.iter().any(|value| value.get() < 0.0)
        || zero_axes > 1
        || size
            .iter()
            .any(|value| value.get() + 2.0 * inflate.get() < 0.0)
    {
        return None;
    }
    let zero = [EntityGeometryScalar::ZERO; 3];
    Some(EntityGeometryCube {
        origin,
        size,
        pivot: vector(cube.get("pivot"))
            .or_else(|| EntityGeometryCube::default_rotation_pivot(origin, size))?,
        rotation: vector(cube.get("rotation")).unwrap_or(zero),
        uv: match cube.get("uv") {
            Some(Value::Object(faces)) => face_uvs(faces)?,
            other => {
                EntityGeometryUv::Box(vector(other).unwrap_or([EntityGeometryScalar::ZERO; 2]))
            }
        },
        inflate,
        mirror: cube
            .get("mirror")
            .and_then(Value::as_bool)
            .unwrap_or(bone_mirror),
    })
}

fn face_uvs(faces: &Map<String, Value>) -> Option<EntityGeometryUv> {
    let face = |name: &str| {
        let face = faces.get(name)?;
        Some(EntityGeometryFaceUv {
            uv: vector(face.get("uv"))?,
            uv_size: vector(face.get("uv_size")),
        })
    };
    let uvs = EntityGeometryFaceUvs {
        north: face("north"),
        south: face("south"),
        east: face("east"),
        west: face("west"),
        up: face("up"),
        down: face("down"),
    };
    [
        &uvs.north, &uvs.south, &uvs.east, &uvs.west, &uvs.up, &uvs.down,
    ]
    .iter()
    .any(|face| face.is_some())
    .then_some(EntityGeometryUv::Faces(uvs))
}

fn overlay_bone(
    base: &mut EntityGeometryBone,
    child: &EntityGeometryBone,
    maximum_cubes: usize,
) -> Result<usize, SkinGeometryError> {
    let cube_count = base
        .append_inherited_cubes(child, maximum_cubes)
        .ok_or(SkinGeometryError::TooManyCubes)?;
    if child.parent.is_some() {
        base.parent.clone_from(&child.parent);
    }
    if child.pivot.is_some() {
        base.pivot = child.pivot;
    }
    if child.rotation.is_some() {
        base.rotation = child.rotation;
    }
    if child.never_render.is_some() {
        base.never_render = child.never_render;
    }
    Ok(cube_count)
}

#[cfg(test)]
mod tests;
