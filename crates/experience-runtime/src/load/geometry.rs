//! A block's `.geo.json`: one geometry in the Experience's namespace, read for the names that
//! its visual and permutations may refer to.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result, bail, ensure};
use serde_json::Value;

use super::{FACES, is_name};
use crate::limits::{MAX_GEOMETRY_BYTES, MAX_NAME_BYTES};

/// The parts of a geometry that a block refers to by name.
#[derive(Debug, Clone)]
pub(super) struct Geometry {
    /// `geometry.<experience id>.<name>`.
    pub identifier: String,
    pub bones: BTreeSet<String>,
    /// The material instances its cube faces draw with: a face's `material_instance`, else the
    /// face's own name.
    pub instances: BTreeSet<String>,
}

/// Reads the geometry file at `path`, which must hold exactly one geometry, identified in the
/// namespace of the Experience `id`.
pub(super) fn read(path: &Path, id: &str) -> Result<Geometry> {
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| {
            file.take(MAX_GEOMETRY_BYTES as u64 + 1)
                .read_to_end(&mut bytes)
        })
        .context("reading it")?;
    ensure!(
        bytes.len() <= MAX_GEOMETRY_BYTES,
        "it exceeds {MAX_GEOMETRY_BYTES} bytes"
    );
    let file: Value = serde_json::from_slice(&bytes).context("parsing it")?;
    let geometries = file
        .get("minecraft:geometry")
        .and_then(Value::as_array)
        .context("it has no \"minecraft:geometry\" list")?;
    let [geometry] = geometries.as_slice() else {
        bail!("it holds {} geometries; it must hold one", geometries.len());
    };
    let identifier = geometry
        .pointer("/description/identifier")
        .and_then(Value::as_str)
        .context("its geometry has no description.identifier")?;
    let namespace = format!("geometry.{id}.");
    ensure!(
        identifier.strip_prefix(&namespace).is_some_and(is_name),
        "geometry \"{identifier}\" is outside namespace \"{namespace}\": it must be \
         {namespace}<name>, the name matching ^[a-z0-9_]{{1,{MAX_NAME_BYTES}}}$"
    );
    let mut bones = BTreeSet::new();
    let mut instances = BTreeSet::new();
    for bone in list(geometry, "bones") {
        let name = bone
            .get("name")
            .and_then(Value::as_str)
            .with_context(|| format!("geometry \"{identifier}\" has a bone without a name"))?;
        bones.insert(name.to_owned());
        for cube in list(bone, "cubes") {
            let per_face = cube.get("uv").and_then(Value::as_object);
            for face in FACES {
                let instance = match per_face {
                    // Box UV maps every face.
                    None => Some(face),
                    Some(faces) => faces.get(face).map(|uv| {
                        uv.get("material_instance")
                            .and_then(Value::as_str)
                            .unwrap_or(face)
                    }),
                };
                instances.extend(instance.map(str::to_owned));
            }
        }
    }
    Ok(Geometry {
        identifier: identifier.to_owned(),
        bones,
        instances,
    })
}

/// The list at `key` of `value`; none when it is missing or not a list.
fn list<'a>(value: &'a Value, key: &str) -> &'a [Value] {
    value
        .get(key)
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}
