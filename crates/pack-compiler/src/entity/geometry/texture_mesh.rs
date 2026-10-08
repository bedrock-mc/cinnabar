//! Bounded authored raster-extrusion geometry; transforms are not baked into pixels.

use super::{
    invalid, optional_bool, optional_vec, required_string, validate_object_fields, zero_vec3,
};
use assets::{AssetError, EntityGeometryTextureMesh, MAX_ENTITY_GEOMETRY_TEXTURE_MESHES};
use serde_json::Value;
use std::path::Path;

pub(super) fn parse(
    value: Option<&Value>,
    path: &Path,
) -> Result<Box<[EntityGeometryTextureMesh]>, AssetError> {
    let Some(value) = value else {
        return Ok(Box::new([]));
    };
    let meshes = value
        .as_array()
        .filter(|meshes| meshes.len() <= MAX_ENTITY_GEOMETRY_TEXTURE_MESHES)
        .ok_or_else(|| {
            invalid("entity geometry texture meshes exceed bound or are not an array")
        })?;
    meshes
        .iter()
        .map(|mesh| {
            validate_object_fields(
                mesh,
                path,
                &[
                    "local_pivot",
                    "position",
                    "rotation",
                    "scale",
                    "use_pixel_depth",
                    "texture",
                ],
                &["texture"],
            )?;
            Ok(EntityGeometryTextureMesh {
                local_pivot: optional_vec(mesh, "local_pivot", path)?.unwrap_or_else(zero_vec3),
                position: optional_vec(mesh, "position", path)?.unwrap_or_else(zero_vec3),
                rotation: optional_vec(mesh, "rotation", path)?.unwrap_or_else(zero_vec3),
                scale: optional_vec(mesh, "scale", path)?
                    .unwrap_or(EntityGeometryTextureMesh::DEFAULT_SCALE),
                use_pixel_depth: optional_bool(mesh, "use_pixel_depth", path)?
                    .unwrap_or(EntityGeometryTextureMesh::DEFAULT_USE_PIXEL_DEPTH),
                texture: required_string(mesh, "texture", path)?.into(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn malformed_depth_and_unbounded_scale_reject_before_mesh_retention() {
        for mesh in [
            json!({"texture":"default", "use_pixel_depth":"false"}),
            json!({"texture":"default", "scale":[1e30,1,1]}),
        ] {
            assert!(parse(Some(&json!([mesh])), Path::new("synthetic.geo.json")).is_err());
        }
    }
}
