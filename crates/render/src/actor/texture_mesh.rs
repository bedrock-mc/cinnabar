//! Native attachable raster extrusions (TextureMesh::compileQuads).
//!
//! Unlike cubes, these meshes start in the image's X/Z plane with Y-down depth.

use assets::{EquipmentTexture, RuntimeEntityAssets};
use bevy::math::{Mat4, Vec3};

use super::{
    ActorRigGeometry, ActorRigGeometryError, ActorRigVertex, EntityRigId, MAX_ACTOR_RIG_VERTICES,
    asset_geometry::{bone_bind_pivot, resolve_geometry_bones},
};

/// Builds cubes and pixel extrusions using the render controller's selected image. Every
/// raster mesh in this geometry must refer to that image's local alias.
pub fn attachable_geometry(
    assets: &RuntimeEntityAssets,
    geometry_index: usize,
    id: EntityRigId,
    texture: &EquipmentTexture,
) -> Result<ActorRigGeometry, ActorRigGeometryError> {
    let geometry = assets
        .geometries()
        .get(geometry_index)
        .ok_or(ActorRigGeometryError::InvalidAssetGeometry)?;
    let bones = resolve_geometry_bones(assets, geometry_index)?;
    let mut vertices = Vec::new();
    for (index, bone) in bones.iter().enumerate() {
        if bone.never_render == Some(true) {
            continue;
        }
        for cube in &bone.cubes {
            super::geometry::append_entity_cube_vertices(
                &mut vertices,
                cube,
                index as u32,
                (geometry.texture_width, geometry.texture_height),
                bone.mirror.unwrap_or(false),
                bone.inflate.map_or(0.0, |v| v.get()),
            )?;
        }
        for mesh in &bone.texture_meshes {
            let pivot = Vec3::from_array(mesh.local_pivot.map(|v| v.get()));
            let position = Vec3::from_array(mesh.position.map(|v| v.get()));
            let authored_bone_pivot =
                Vec3::from_array(bone.pivot.map_or([0.0; 3], |p| p.map(|v| v.get())));
            let [x, y, z] = mesh.rotation.map(|v| v.get().to_radians());
            let sx = f32::from(geometry.texture_width) / f32::from(texture.width);
            let sz = f32::from(geometry.texture_height) / f32::from(texture.height);
            // Bone matrices subtract the bind pivot; our vertices retain absolute model
            // coordinates, so native's bone-local subtraction is left to that matrix.
            let matrix = Mat4::from_translation(Vec3::from_array(bone_bind_pivot(bone)))
                * Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0) / 16.0)
                * Mat4::from_translation(position - authored_bone_pivot)
                * Mat4::from_rotation_z(z)
                * Mat4::from_rotation_y(y)
                * Mat4::from_rotation_x(x)
                * Mat4::from_translation(-pivot)
                * Mat4::from_scale(
                    Vec3::new(sx, sx.max(sz), sz) * Vec3::from_array(mesh.scale.map(|v| v.get())),
                );
            append_pixels(
                &mut vertices,
                texture,
                index as u32,
                matrix,
                mesh.use_pixel_depth,
            )?;
        }
        if vertices.len() > MAX_ACTOR_RIG_VERTICES {
            return Err(ActorRigGeometryError::CatalogCapacity);
        }
    }
    ActorRigGeometry::new(
        id,
        vertices,
        bones.iter().map(bone_bind_pivot).collect::<Vec<_>>(),
    )
}

fn append_pixels(
    vertices: &mut Vec<ActorRigVertex>,
    texture: &EquipmentTexture,
    bone_index: u32,
    matrix: Mat4,
    use_pixel_depth: bool,
) -> Result<(), ActorRigGeometryError> {
    let width = usize::from(texture.width);
    let height = usize::from(texture.height);
    if width == 0 || height == 0 || texture.rgba8.len() != width * height * 4 {
        return Err(ActorRigGeometryError::InvalidAssetGeometry);
    }
    let opaque = |x: isize, z: isize| {
        x >= 0
            && z >= 0
            && (x as usize) < width
            && (z as usize) < height
            && texture.rgba8[(z as usize * width + x as usize) * 4 + 3] >= 2
    };
    let depth = if use_pixel_depth {
        width.max(height) as f32 / 16.0
    } else {
        1.0
    };
    for z in 0..height {
        for x in 0..width {
            if !opaque(x as isize, z as isize) {
                continue;
            }
            let uv = [
                (x as f32 + 0.5) / width as f32,
                (z as f32 + 0.5) / height as f32,
            ];
            let (a, b, c, d) = (x as f32, x as f32 + 1.0, z as f32, z as f32 + 1.0);
            let faces = [
                (true, [[a, 0.0, c], [b, 0.0, c], [b, 0.0, d], [a, 0.0, d]]),
                (
                    true,
                    [[a, depth, d], [b, depth, d], [b, depth, c], [a, depth, c]],
                ),
                (
                    !opaque(x as isize - 1, z as isize),
                    [[a, 0.0, d], [a, depth, d], [a, depth, c], [a, 0.0, c]],
                ),
                (
                    !opaque(x as isize + 1, z as isize),
                    [[b, 0.0, c], [b, depth, c], [b, depth, d], [b, 0.0, d]],
                ),
                (
                    !opaque(x as isize, z as isize - 1),
                    [[a, 0.0, c], [a, depth, c], [b, depth, c], [b, 0.0, c]],
                ),
                (
                    !opaque(x as isize, z as isize + 1),
                    [[b, 0.0, d], [b, depth, d], [a, depth, d], [a, 0.0, d]],
                ),
            ];
            for (visible, corners) in faces {
                if !visible {
                    continue;
                }
                let corners = corners.map(|p| matrix.transform_point3(Vec3::from_array(p)));
                let normal = (corners[1] - corners[0])
                    .cross(corners[2] - corners[0])
                    .normalize_or_zero()
                    .to_array();
                vertices.extend([0, 1, 2, 0, 2, 3].map(|i| ActorRigVertex {
                    position: corners[i].to_array(),
                    normal,
                    uv,
                    back_uv: uv,
                    bone_index,
                }));
                if vertices.len() > MAX_ACTOR_RIG_VERTICES {
                    return Err(ActorRigGeometryError::CatalogCapacity);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_depth_and_alpha_follow_native_tessellator() {
        let texture = EquipmentTexture {
            identifier: "test:mesh".into(),
            width: 2,
            height: 1,
            rgba8: std::sync::Arc::from([255, 255, 255, 1, 255, 255, 255, 2]),
        };
        let mut vertices = Vec::new();
        append_pixels(&mut vertices, &texture, 0, Mat4::IDENTITY, true).unwrap();
        assert_eq!(vertices.len(), 36);
        assert!(vertices.iter().all(|v| v.position[0] >= 1.0));
        assert!(
            vertices
                .iter()
                .all(|v| v.position[1] >= 0.0 && v.position[1] <= 2.0 / 16.0)
        );
        assert!(
            vertices
                .iter()
                .all(|v| v.uv == [0.75, 0.5] && v.back_uv == v.uv)
        );
        let mut fixed = Vec::new();
        append_pixels(&mut fixed, &texture, 0, Mat4::IDENTITY, false).unwrap();
        assert_eq!(
            fixed.iter().map(|v| v.position[1]).fold(0.0_f32, f32::max),
            1.0
        );
    }
}
