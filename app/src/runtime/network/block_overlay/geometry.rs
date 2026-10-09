//! Block geometry from pack `.geo.json` files, reduced to axis-aligned cube faces.
//!
//! Geometry X is mirrored relative to world X, as authoring tools export it;
//! face names are world directions. Faces map `u` to the viewer's right and
//! `v` downward, with up textures top-north and down textures top-south.

use std::collections::{HashMap, HashSet};

use resource_pack::{LayeredPackView, normalize_jsonc};
use serde_json::Value;

const MAX_GEOMETRY_FILES: usize = 4096;
const MAX_GEOMETRY_FILE_BYTES: usize = 1024 * 1024;
const MAX_CUBES: usize = 256;
const MAX_BONES: usize = 256;

/// Face order matches `assets::BlockFace`: west, east, down, up, north, south.
pub(super) const FACE_NAMES: [&str; 6] = ["west", "east", "down", "up", "north", "south"];

#[derive(Clone, Debug, PartialEq)]
pub(super) struct FaceUv {
    pub(super) uv: [f32; 2],
    pub(super) size: [f32; 2],
    pub(super) material_instance: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Cube {
    /// The bone that owns the cube, which bone visibility hides by name.
    pub(super) bone: String,
    /// Block-local pixel bounds in world orientation (0..16 spans the block).
    pub(super) min: [f32; 3],
    pub(super) max: [f32; 3],
    pub(super) faces: [Option<FaceUv>; 6],
    /// Rotations to apply in order (cube, then each ancestor bone), in block-local pixels:
    /// a pivot and X/Y/Z angles in the world frame's authored convention.
    pub(super) rotations: Vec<([f32; 3], [f32; 3])>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Geometry {
    pub(super) texture_size: [f32; 2],
    pub(super) cubes: Vec<Cube>,
    /// Cubes dropped for non-finite or malformed values.
    pub(super) skipped_cubes: u32,
}

impl Geometry {
    /// Full-block faces for a component that needs position-dependent model displacement.
    pub(super) fn full_block() -> Self {
        Self {
            texture_size: [16.0; 2],
            cubes: vec![Cube {
                bone: String::new(),
                min: [0.0; 3],
                max: [16.0; 3],
                faces: std::array::from_fn(|_| {
                    Some(FaceUv {
                        uv: [0.0; 2],
                        size: [16.0; 2],
                        material_instance: None,
                    })
                }),
                rotations: Vec::new(),
            }],
            skipped_cubes: 0,
        }
    }
}

/// One face ready for quantization: corners in block-local pixels and texture
/// pixel UVs, in the carrier's counter-clockwise corner order.
pub(super) struct FaceQuad {
    pub(super) face: usize,
    pub(super) positions: [[f32; 3]; 4],
    pub(super) uvs: [[f32; 2]; 4],
    /// Turned off the block axes, so it has no cull face.
    pub(super) rotated: bool,
}

/// Finds the wanted identifiers under `models/`; a higher pack replaces one.
pub(super) fn geometry_catalog(
    view: &LayeredPackView,
    wanted: &HashSet<&str>,
) -> HashMap<String, Geometry> {
    let mut catalog = HashMap::new();
    if wanted.is_empty() {
        return catalog;
    }
    let files = view
        .list("models/")
        .into_iter()
        .filter(|path| path.ends_with(".json"))
        .take(MAX_GEOMETRY_FILES)
        .map(str::to_owned)
        .collect::<Vec<_>>();
    for layer in view.layers() {
        for path in &files {
            let Ok(Some(bytes)) = layer.read_file(path) else {
                continue;
            };
            if bytes.len() > MAX_GEOMETRY_FILE_BYTES
                || (!bytes.contains(&b'\\')
                    && !wanted.iter().any(|id| contains(&bytes, id.as_bytes())))
            {
                continue;
            }
            for (identifier, geometry) in parse_geometry_file(&bytes) {
                if wanted.contains(identifier.as_str()) {
                    catalog.insert(identifier, geometry);
                }
            }
        }
    }
    catalog
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

/// Parses both the `minecraft:geometry` array form and the legacy form keyed by
/// identifier (`"geometry.x"` or `"geometry.x:parent"`).
pub(super) fn parse_geometry_file(bytes: &[u8]) -> Vec<(String, Geometry)> {
    let Some(root) =
        normalize_jsonc(bytes).and_then(|json| serde_json::from_slice::<Value>(&json).ok())
    else {
        return Vec::new();
    };
    if let Some(entries) = root["minecraft:geometry"].as_array() {
        return entries
            .iter()
            .filter_map(|entry| {
                let description = &entry["description"];
                let identifier = description["identifier"].as_str()?.to_owned();
                let size = [
                    number(&description["texture_width"]).unwrap_or(16.0),
                    number(&description["texture_height"]).unwrap_or(16.0),
                ];
                Some((identifier, parse_bones(&entry["bones"], size)))
            })
            .collect();
    }
    let Some(object) = root.as_object() else {
        return Vec::new();
    };
    object
        .iter()
        .filter(|(key, _)| key.starts_with("geometry."))
        .map(|(key, entry)| {
            let identifier = key.split(':').next().unwrap_or(key).to_owned();
            let size = [
                number(&entry["texturewidth"]).unwrap_or(16.0),
                number(&entry["textureheight"]).unwrap_or(16.0),
            ];
            (identifier, parse_bones(&entry["bones"], size))
        })
        .collect()
}

fn number(value: &Value) -> Option<f32> {
    value
        .as_f64()
        .map(|value| value as f32)
        .filter(|value| value.is_finite())
}

fn vector(value: &Value) -> Option<[f32; 3]> {
    let parts = value.as_array()?;
    if parts.len() != 3 {
        return None;
    }
    Some([number(&parts[0])?, number(&parts[1])?, number(&parts[2])?])
}

fn is_rotated(value: &Value) -> bool {
    vector(value).is_some_and(|rotation| rotation.iter().any(|angle| angle.abs() > f32::EPSILON))
}

/// A geometry-frame pivot in block-local world pixels (X mirrored, Z shifted).
fn world_pivot(pivot: [f32; 3]) -> [f32; 3] {
    [8.0 - pivot[0], pivot[1], pivot[2] + 8.0]
}

/// Authored X/Y/Z rotation as the angles applied in the world frame, X then Y then Z.
fn world_angles(rotation: [f32; 3]) -> [f32; 3] {
    [-rotation[0], rotation[1], -rotation[2]]
}

fn parse_bones(bones: &Value, texture_size: [f32; 2]) -> Geometry {
    let mut geometry = Geometry {
        texture_size,
        ..Geometry::default()
    };
    let Some(bones) = bones.as_array() else {
        return geometry;
    };
    let bones = &bones[..bones.len().min(MAX_BONES)];
    let by_name = bones
        .iter()
        .filter_map(|bone| Some((bone["name"].as_str()?, bone)))
        .collect::<HashMap<_, _>>();
    for bone in bones {
        // Rotations from the bone up through its ancestors, in application order.
        let mut chain = Vec::new();
        let mut current = Some(bone);
        for _ in 0..=by_name.len() {
            let Some(bone) = current else { break };
            if is_rotated(&bone["rotation"]) {
                let pivot = vector(&bone["pivot"]).unwrap_or([0.0; 3]);
                if let Some(rotation) = vector(&bone["rotation"]) {
                    chain.push((world_pivot(pivot), world_angles(rotation)));
                }
            }
            current = bone["parent"]
                .as_str()
                .and_then(|name| by_name.get(name).copied());
        }
        for cube in bone["cubes"].as_array().map_or(&[][..], Vec::as_slice) {
            if geometry.cubes.len() >= MAX_CUBES {
                return geometry;
            }
            let Some(mut parsed) = parse_cube(cube) else {
                geometry.skipped_cubes = geometry.skipped_cubes.saturating_add(1);
                continue;
            };
            if is_rotated(&cube["rotation"])
                && let Some(rotation) = vector(&cube["rotation"])
            {
                let pivot = vector(&cube["pivot"])
                    .or_else(|| vector(&bone["pivot"]))
                    .unwrap_or([0.0; 3]);
                parsed
                    .rotations
                    .push((world_pivot(pivot), world_angles(rotation)));
            }
            parsed.rotations.extend(chain.iter().copied());
            bone["name"]
                .as_str()
                .unwrap_or_default()
                .clone_into(&mut parsed.bone);
            geometry.cubes.push(parsed);
        }
    }
    geometry
}

/// Turns `point` by `degrees` about `pivot`: X, then Y, then Z.
fn rotate_around(point: [f32; 3], pivot: [f32; 3], degrees: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = degrees.map(f32::to_radians);
    let (sx, cx) = x.sin_cos();
    let (sy, cy) = y.sin_cos();
    let (sz, cz) = z.sin_cos();
    let v = [
        point[0] - pivot[0],
        point[1] - pivot[1],
        point[2] - pivot[2],
    ];
    let v = [v[0], v[1] * cx - v[2] * sx, v[1] * sx + v[2] * cx];
    let v = [v[0] * cy + v[2] * sy, v[1], -v[0] * sy + v[2] * cy];
    let v = [v[0] * cz - v[1] * sz, v[0] * sz + v[1] * cz, v[2]];
    [v[0] + pivot[0], v[1] + pivot[1], v[2] + pivot[2]]
}

fn parse_cube(cube: &Value) -> Option<Cube> {
    let origin = vector(&cube["origin"])?;
    let size = vector(&cube["size"])?;
    let inflate = number(&cube["inflate"]).unwrap_or(0.0);
    let geo_max = [
        origin[0] + size[0],
        origin[1] + size[1],
        origin[2] + size[2],
    ];
    let min = [
        8.0 - geo_max[0] - inflate,
        origin[1] - inflate,
        origin[2] + 8.0 - inflate,
    ];
    let max = [
        8.0 - origin[0] + inflate,
        geo_max[1] + inflate,
        geo_max[2] + 8.0 + inflate,
    ];
    if !min.iter().chain(&max).all(|value| value.is_finite()) {
        return None;
    }
    let faces = match &cube["uv"] {
        Value::Object(faces) => FACE_NAMES.map(|name| {
            let face = faces.get(name)?;
            let uv = face["uv"].as_array()?;
            let uv = [number(uv.first()?)?, number(uv.get(1)?)?];
            let size = face["uv_size"]
                .as_array()
                .and_then(|size| Some([number(size.first()?)?, number(size.get(1)?)?]))
                .unwrap_or([0.0, 0.0]);
            Some(FaceUv {
                uv,
                size,
                material_instance: face["material_instance"].as_str().map(str::to_owned),
            })
        }),
        Value::Array(offset) => box_uv([number(offset.first()?)?, number(offset.get(1)?)?], size),
        _ => return None,
    };
    Some(Cube {
        bone: String::new(),
        min,
        max,
        faces,
        rotations: Vec::new(),
    })
}

/// The conventional box layout: a cross of faces starting at `offset`.
fn box_uv(offset: [f32; 2], size: [f32; 3]) -> [Option<FaceUv>; 6] {
    let [w, h, d] = size.map(f32::floor);
    let [u, v] = offset;
    let face = |uv: [f32; 2], size: [f32; 2]| {
        Some(FaceUv {
            uv,
            size,
            material_instance: None,
        })
    };
    [
        face([u + d + w, v + d], [d, h]),
        face([u, v + d], [d, h]),
        face([u + d + w, v], [w, d]),
        face([u + d, v], [w, d]),
        face([u + d, v + d], [w, h]),
        face([u + d + w + d, v + d], [w, h]),
    ]
}

impl Cube {
    /// Emits every face that has a UV entry and non-zero area.
    pub(super) fn quads(&self) -> Vec<(FaceQuad, Option<&str>)> {
        let ([x0, y0, z0], [x1, y1, z1]) = (self.min, self.max);
        let mut quads = Vec::with_capacity(6);
        for (face, uv) in self.faces.iter().enumerate() {
            let Some(uv) = uv else {
                continue;
            };
            let mut positions = match face {
                0 => [[x0, y0, z0], [x0, y0, z1], [x0, y1, z1], [x0, y1, z0]],
                1 => [[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]],
                2 => [[x0, y0, z0], [x1, y0, z0], [x1, y0, z1], [x0, y0, z1]],
                3 => [[x0, y1, z0], [x0, y1, z1], [x1, y1, z1], [x1, y1, z0]],
                4 => [[x0, y0, z0], [x0, y1, z0], [x1, y1, z0], [x1, y0, z0]],
                _ => [[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]],
            };
            let degenerate = match face {
                0 | 1 => y0 == y1 || z0 == z1,
                2 | 3 => x0 == x1 || z0 == z1,
                _ => x0 == x1 || y0 == y1,
            };
            if degenerate {
                continue;
            }
            for &(pivot, angles) in &self.rotations {
                for corner in &mut positions {
                    *corner = rotate_around(*corner, pivot, angles);
                }
            }
            let (u0, v0) = (uv.uv[0], uv.uv[1]);
            let (u1, v1) = (u0 + uv.size[0], v0 + uv.size[1]);
            // Corner UVs per face in the same order as `positions`.
            let uvs = match face {
                0 => [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
                1 => [[u1, v1], [u1, v0], [u0, v0], [u0, v1]],
                2 => [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
                3 => [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
                4 => [[u1, v1], [u1, v0], [u0, v0], [u0, v1]],
                _ => [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
            };
            quads.push((
                FaceQuad {
                    face,
                    positions,
                    uvs,
                    rotated: !self.rotations.is_empty(),
                },
                uv.material_instance.as_deref(),
            ));
        }
        quads
    }
}
