//! Vanilla's GUI shield drawing, not the first-person attachable animation or a flat UV sheet.
//! Vanilla transform: T(8,10,-10) S(11) Rx(30) Ry(30), model unit 1/16.

use crate::entity::{EntityAssetCompilation, compile_equipment_textures};
use assets::gui_item::{
    GUI_ITEM_SIDE, SHIELD_FACE_CORNERS as FACE_CORNERS, SHIELD_ROOT_BONE as ROOT_BONE,
    shield_face_uvs as face_uvs,
};
use assets::{
    AssetError, EntityGeometry, EntityGeometryBone, EntityGeometryCube, EquipmentTexture,
    IconSprite,
};
use std::{path::Path, sync::Arc};

mod raster;

use assets::gui_item::SHIELD_IDENTIFIER as IDENTIFIER;
/// Carrier resolution only; native GUI coordinates retain their sixteen-pixel item frame.
const SIDE: usize = 64;
const PIXELS_PER_GUI_PIXEL: f32 = SIDE as f32 / GUI_ITEM_SIDE;

pub(super) fn compile(
    root: &Path,
    compilation: &EntityAssetCompilation,
) -> Result<Option<IconSprite>, AssetError> {
    let Some(binding) = compilation
        .equipment_bindings
        .iter()
        .find(|binding| binding.identifier.as_ref() == IDENTIFIER)
    else {
        return Ok(None);
    };
    let Some(geometry) = compilation
        .assets
        .geometries
        .iter()
        .find(|geometry| geometry.identifier == binding.geometry.identifier)
    else {
        return Ok(None);
    };
    let textures = compile_equipment_textures(
        root,
        &compilation.assets.sources,
        std::slice::from_ref(binding),
    )?;
    let Some(texture) = textures
        .iter()
        .find(|texture| texture.identifier == binding.texture.identifier)
    else {
        return Ok(None);
    };
    Ok(bake(geometry, texture))
}

fn bake(geometry: &EntityGeometry, texture: &EquipmentTexture) -> Option<IconSprite> {
    // The vanilla shield model loads its named root. Exotic animated/inherited model-part trees
    // must not silently use an invented transform; those remain an explicit unsupported branch.
    let bone = geometry
        .bones
        .iter()
        .find(|bone| bone.name.as_ref() == ROOT_BONE)?;
    if geometry.inherits.is_some()
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
    let mut pixels = vec![0; SIDE * SIDE * 4];
    for cube in &bone.cubes {
        if cube.rotation.iter().any(|value| value.get() != 0.0) {
            return None;
        }
        append_cube(&mut pixels, geometry, bone, cube, texture);
    }
    Some(IconSprite {
        width: SIDE as u16,
        height: SIDE as u16,
        rgba8: Arc::from(pixels),
    })
}

fn project(authored: [f32; 3]) -> [f32; 2] {
    let [x, y, _] = assets::gui_item::project_shield(authored);
    [x, y].map(|value| value * PIXELS_PER_GUI_PIXEL)
}

fn append_cube(
    pixels: &mut [u8],
    geometry: &EntityGeometry,
    bone: &EntityGeometryBone,
    cube: &EntityGeometryCube,
    texture: &EquipmentTexture,
) {
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
        let Some(uvs) = uvs[face] else {
            continue;
        };
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
        // Native GUI disables depth testing, but keeps backface culling. Preserve authored
        // cube/face order (the handle precedes the board), no cube-thumbnail light/depth heuristic.
        for triangle in [[0, 1, 2], [0, 2, 3]] {
            raster::triangle(
                pixels,
                triangle.map(|i| points[i]),
                triangle.map(|i| uvs[i]),
                texture,
            );
        }
    }
}

#[cfg(test)]
mod tests;
