//! Persona polygons use independent position, normal and UV indices.
use assets::SkinPolyMesh;

use super::{ActorRigGeometryError, ActorRigVertex, MAX_ACTOR_RIG_VERTICES};

/// Converts authored skin polygons into the same actor frame used by cubes.
pub(super) fn append(
    vertices: &mut Vec<ActorRigVertex>,
    mesh: &SkinPolyMesh,
    bone_index: u32,
    texture_size: (u16, u16),
) -> Result<(), ActorRigGeometryError> {
    if vertices.len().saturating_add(mesh.vertices.len()) > MAX_ACTOR_RIG_VERTICES {
        return Err(ActorRigGeometryError::CatalogCapacity);
    }
    let scale = if mesh.normalized_uvs {
        [1.0, 1.0]
    } else {
        [
            1.0 / f32::from(texture_size.0),
            1.0 / f32::from(texture_size.1),
        ]
    };
    for vertex in &mesh.vertices {
        let [x, y, z] = vertex.position;
        let [nx, ny, nz] = vertex.normal;
        let uv = [
            (vertex.uv[0] * scale[0]).clamp(0.0, 1.0),
            ((1.0 - vertex.uv[1]) * scale[1]).clamp(0.0, 1.0),
        ];
        vertices.push(ActorRigVertex {
            position: [-x / 16.0, y / 16.0, z / 16.0],
            normal: [-nx, ny, nz],
            uv,
            back_uv: uv,
            bone_index,
            surface: super::ActorRigSurface::SINGLE_FACE,
        });
    }
    Ok(())
}
