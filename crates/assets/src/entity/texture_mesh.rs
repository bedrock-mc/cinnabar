//! Authored raster-extrusion geometry retained for held attachables.

use serde::{Deserialize, Serialize};

use super::{EntityGeometryBone, EntityGeometryScalar, validate_identifier, validate_scalars};
use crate::AssetError;

/// Raster extrusions one geometry may contribute, checked before carrier allocation.
pub const MAX_ENTITY_GEOMETRY_TEXTURE_MESHES: usize = 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EntityGeometryTextureMesh {
    pub local_pivot: [EntityGeometryScalar; 3],
    pub position: [EntityGeometryScalar; 3],
    pub rotation: [EntityGeometryScalar; 3],
    #[serde(default = "default_scale", skip_serializing_if = "is_default_scale")]
    pub scale: [EntityGeometryScalar; 3],
    #[serde(
        default = "default_use_pixel_depth",
        skip_serializing_if = "is_default_use_pixel_depth"
    )]
    pub use_pixel_depth: bool,
    /// Local texture alias in the attachable's description.
    pub texture: Box<str>,
}

impl EntityGeometryTextureMesh {
    pub const DEFAULT_SCALE: [EntityGeometryScalar; 3] =
        [EntityGeometryScalar(1.0_f32.to_bits()); 3];
    pub const DEFAULT_USE_PIXEL_DEPTH: bool = true;
}

fn default_scale() -> [EntityGeometryScalar; 3] {
    EntityGeometryTextureMesh::DEFAULT_SCALE
}

fn is_default_scale(value: &[EntityGeometryScalar; 3]) -> bool {
    *value == EntityGeometryTextureMesh::DEFAULT_SCALE
}

fn default_use_pixel_depth() -> bool {
    EntityGeometryTextureMesh::DEFAULT_USE_PIXEL_DEPTH
}

fn is_default_use_pixel_depth(value: &bool) -> bool {
    *value == EntityGeometryTextureMesh::DEFAULT_USE_PIXEL_DEPTH
}

pub(super) fn validate(bone: &EntityGeometryBone) -> Result<(), AssetError> {
    if let Some(binding) = &bone.binding {
        validate_identifier(binding)?;
    }
    for mesh in &bone.texture_meshes {
        validate_scalars(&mesh.local_pivot)?;
        validate_scalars(&mesh.position)?;
        validate_scalars(&mesh.rotation)?;
        validate_scalars(&mesh.scale)?;
        validate_identifier(&mesh.texture)?;
    }
    Ok(())
}
