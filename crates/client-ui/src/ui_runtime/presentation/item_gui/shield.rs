//! The shield GUI model-part path, not the first-person attachable animation.

use std::sync::Arc;

use assets::gui_item::{
    SHIELD_ALPHA_CUTOFF, SHIELD_FACE_CORNERS as FACE_CORNERS, SHIELD_ROOT_BONE as ROOT_BONE,
    shield_face_uvs as face_uvs,
};
use assets::{EntityGeometry, EntityGeometryBone, EntityGeometryCube, EquipmentTexture};
use ui::{UiMesh, UiMeshVertex};

use super::{GUI_ITEM_SIDE, IconRef, atlas_uv, batch, vertex};

pub(super) fn mesh(
    geometry: &EntityGeometry,
    texture: &EquipmentTexture,
    texture_ref: IconRef,
) -> Option<Arc<UiMesh>> {
    let bone = geometry
        .bones
        .iter()
        .find(|bone| bone.name.as_ref() == ROOT_BONE)?;
    if geometry.inherits.is_some()
        || geometry.texture_width == 0
        || geometry.texture_height == 0
        || texture.width == 0
        || texture.height == 0
        || texture.rgba8.len() != usize::from(texture.width) * usize::from(texture.height) * 4
        || texture_ref.uv[2].checked_sub(texture_ref.uv[0])? != texture.width
        || texture_ref.uv[3].checked_sub(texture_ref.uv[1])? != texture.height
        || bone.parent.is_some()
        || bone.never_render == Some(true)
        || !bone.texture_meshes.is_empty()
        || bone
            .rotation
            .is_some_and(|r| r.iter().any(|v| v.get() != 0.0))
        || geometry
            .bones
            .iter()
            .any(|b| b.parent.as_deref() == Some(ROOT_BONE))
        || bone.cubes.is_empty()
    {
        return None;
    }
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for cube in &bone.cubes {
        if cube.rotation.iter().any(|value| value.get() != 0.0) {
            return None;
        }
        append_cube(
            &mut vertices,
            &mut indices,
            geometry,
            bone,
            cube,
            texture_ref,
        )?;
    }
    if indices.is_empty() {
        return None;
    }
    let range = 0..indices.len() as u32;
    UiMesh::new(
        vertices.into(),
        indices.into(),
        vec![batch(texture_ref.page, range, Some(SHIELD_ALPHA_CUTOFF))].into(),
    )
    .ok()
    .map(Arc::new)
}

fn project(authored: [f32; 3]) -> [f32; 2] {
    let [x, y, _] = assets::gui_item::project_shield(authored);
    [x / GUI_ITEM_SIDE, y / GUI_ITEM_SIDE]
}

fn append_cube(
    vertices: &mut Vec<UiMeshVertex>,
    indices: &mut Vec<u32>,
    geometry: &EntityGeometry,
    bone: &EntityGeometryBone,
    cube: &EntityGeometryCube,
    texture: IconRef,
) -> Option<()> {
    let origin = cube.origin.map(|value| value.get());
    let size = cube.size.map(|value| value.get());
    let inflate = cube.inflate.get() + bone.inflate.map_or(0.0, |value| value.get());
    let min: [f32; 3] = std::array::from_fn(|axis| origin[axis] - inflate);
    let max: [f32; 3] = std::array::from_fn(|axis| origin[axis] + size[axis] + inflate);
    let corners = [
        [min[0], min[1], min[2]],
        [max[0], min[1], min[2]],
        [max[0], max[1], min[2]],
        [min[0], max[1], min[2]],
        [min[0], min[1], max[2]],
        [max[0], min[1], max[2]],
        [max[0], max[1], max[2]],
        [min[0], max[1], max[2]],
    ]
    .map(project);
    let mirror = cube.mirror ^ bone.mirror.unwrap_or(false);
    let uvs = face_uvs(cube);
    for (face, corners_index) in FACE_CORNERS.into_iter().enumerate() {
        let Some(uvs) = uvs[face] else { continue };
        let mut points = corners_index.map(|index| corners[if mirror { index ^ 1 } else { index }]);
        let mut uvs = uvs.map(|uv| {
            [
                uv[0] / f32::from(geometry.texture_width),
                uv[1] / f32::from(geometry.texture_height),
            ]
        });
        if mirror {
            points.reverse();
            uvs.reverse();
        }
        // ui_shield preserves authored cube/face order, disables depth, and culls backfaces.
        if signed_area(points[0], points[1], points[2]) <= 0.0 {
            continue;
        }
        let first = vertices.len() as u32;
        for (point, uv) in points.into_iter().zip(uvs) {
            vertices.push(vertex(
                point,
                atlas_uv(texture, uv)?,
                [255; 4],
                texture.glint,
                true,
            ));
        }
        indices.extend([0, 1, 2, 0, 2, 3].map(|index| first + index));
    }
    Some(())
}

fn signed_area(a: [f32; 2], b: [f32; 2], c: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

#[cfg(test)]
mod tests;
