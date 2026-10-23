//! Inventory models whose placed visuals live outside the terrain mesh carrier.

use std::path::Path;

use assets::{AssetError, EntityGeometry, IconSprite, block_entity_geometry as geometry};

use super::{blocks::IconBlocks, model::Model};

struct Part {
    texture: String,
    logical: [f32; 2],
    faces: Vec<geometry::ModelFace>,
    color: [u8; 3],
}

/// Expands model-part boxes into textured faces without changing their authoring frame.
fn boxes(texture: &str, logical: [f32; 2], boxes: &[geometry::ModelBox]) -> Part {
    Part {
        texture: texture.to_owned(),
        logical,
        faces: boxes
            .iter()
            .flat_map(|&(origin, size, uv)| geometry::box_faces(origin, size, uv, 0.0))
            .collect(),
        color: [255; 3],
    }
}

/// Only banner dye changes these block models by item metadata.
pub(super) fn metadata_variant(name: &str, metadata: u32) -> u32 {
    if is_banner(name) { metadata } else { 0 }
}

/// Identifies the standing inventory model shared by both placed banner forms.
fn is_banner(name: &str) -> bool {
    renderer(name) == Some(assets::facts::BlockEntityRenderer::Banner)
}

/// Uses the registry-owned block-entity route for cold inventory model selection.
fn renderer(name: &str) -> Option<assets::facts::BlockEntityRenderer> {
    assets::facts::BlockEntityFacts::pinned()
        .by_block_identifier(name)
        .map(|entry| entry.renderer)
}

/// Bakes an entity-drawn block inventory model, refusing missing optional textures.
pub(super) fn raster(
    root: &Path,
    name: &str,
    metadata: u32,
    geometries: &[EntityGeometry],
) -> Result<Option<IconSprite>, AssetError> {
    let mut projection: fn([f32; 3]) -> [f32; 3] = assets::gui_item::project_cube;
    let mut light = 1.0;
    let mut parts = if let Some(color) = geometry::shulker_color_from_block_name(name) {
        let mut part = boxes(
            &geometry::shulker_texture(color),
            geometry::SHULKER_TEXTURE_SIZE,
            &[geometry::SHULKER_BASE, geometry::SHULKER_LID],
        );
        for (corners, _) in &mut part.faces {
            for [x, y, z] in corners {
                *x = (*x + 8.0) / 16.0;
                *y /= 16.0;
                *z = (*z + 8.0) / 16.0;
            }
        }
        light = assets::gui_item::SHULKER_GUI_LIGHT;
        vec![part]
    } else if let Some(skull_type) = assets::vanilla_skull_type(name) {
        let Some(part) = skull(skull_type, geometries) else {
            return Ok(None);
        };
        vec![part]
    } else {
        match renderer(name) {
            Some(assets::facts::BlockEntityRenderer::Conduit) => {
                projection = assets::gui_item::project_conduit;
                vec![boxes(
                    geometry::CONDUIT_TEXTURE.0,
                    geometry::CONDUIT_TEXTURE.1,
                    &[geometry::CONDUIT_SHELL],
                )]
            }
            Some(assets::facts::BlockEntityRenderer::DecoratedPot) => {
                projection = assets::gui_item::project_decorated_pot;
                let mut base = boxes(
                    geometry::POT_BASE_TEXTURE.0,
                    geometry::POT_BASE_TEXTURE.1,
                    &[],
                );
                for ((origin, size, uv), inflate) in [
                    (geometry::POT_NECK, geometry::POT_NECK_INFLATE),
                    (geometry::POT_LIP, geometry::POT_LIP_INFLATE),
                ] {
                    base.faces
                        .extend(geometry::box_faces(origin, size, uv, inflate));
                }
                for (texels, y) in geometry::POT_PLANES {
                    let half = geometry::POT_BODY_HALF;
                    base.faces.push((
                        [
                            [half, y, half],
                            [-half, y, half],
                            [-half, y, -half],
                            [half, y, -half],
                        ],
                        texels,
                    ));
                }
                vec![
                    base,
                    Part {
                        texture: geometry::POT_SIDE_TEXTURE.0.to_owned(),
                        logical: geometry::POT_SIDE_TEXTURE.1,
                        faces: geometry::pot_sides()
                            .into_iter()
                            .map(|corners| (corners, [0.0, 0.0, 16.0, 16.0]))
                            .collect(),
                        color: [255; 3],
                    },
                ]
            }
            Some(assets::facts::BlockEntityRenderer::Statue) => {
                projection = assets::gui_item::project_statue;
                let Some(parts) = statue(name, geometries) else {
                    return Ok(None);
                };
                parts
            }
            Some(assets::facts::BlockEntityRenderer::Lectern) => lectern(),
            Some(assets::facts::BlockEntityRenderer::Banner) => {
                projection = assets::gui_item::project_banner;
                let frame = boxes(
                    geometry::BANNER_TEXTURE.0,
                    geometry::BANNER_TEXTURE.1,
                    &[geometry::BANNER_POLE, geometry::BANNER_BAR],
                );
                let mut cloth = boxes(
                    geometry::BANNER_TEXTURE.0,
                    geometry::BANNER_TEXTURE.1,
                    &[geometry::BANNER_CLOTH],
                );
                cloth.color = assets::banner::color_rgb(i64::from(metadata));
                for (corners, _) in &mut cloth.faces {
                    for point in corners {
                        for (value, offset) in point.iter_mut().zip(geometry::BANNER_CLOTH_PIVOT) {
                            *value += offset;
                        }
                    }
                }
                vec![frame, cloth]
            }
            _ => return Ok(None),
        }
    };
    let mut textures = Vec::with_capacity(parts.len());
    for part in &parts {
        let Some(mut texture) = IconBlocks::model_texture(root, &part.texture)? else {
            return Ok(None);
        };
        if part.color != [255; 3] {
            for pixel in std::sync::Arc::make_mut(&mut texture.rgba8)
                .as_chunks_mut::<4>()
                .0
            {
                for (value, tint) in pixel[..3].iter_mut().zip(part.color) {
                    *value = (u16::from(*value) * u16::from(tint) / 255) as u8;
                }
            }
        }
        textures.push(texture);
    }
    let mut quads = Vec::new();
    for (part, texture) in parts.iter_mut().zip(&textures) {
        let [width, height] = part.logical;
        for &(corners, [u, v, w, h]) in &part.faces {
            if w <= 0.0 || h == 0.0 {
                continue;
            }
            let uvs = [[u, v], [u + w, v], [u + w, v + h], [u, v + h]]
                .map(|[u, v]| [u / width, v / height]);
            quads.push((corners, uvs, texture));
        }
    }
    let model = Model::textured(quads, projection, light);
    Ok(Some(
        if is_banner(name) || renderer(name) == Some(assets::facts::BlockEntityRenderer::Statue) {
            model.unshaded()
        } else {
            model
        }
        .raster(),
    ))
}

/// Builds the default standing statue with its eye layer above the body artwork.
fn statue(name: &str, geometries: &[EntityGeometry]) -> Option<Vec<Part>> {
    use assets::facts::BlockModelVariant;
    let BlockModelVariant::Statue(age) = assets::facts::block_render_fact(name).variant? else {
        return None;
    };
    let geometry = geometries
        .iter()
        .find(|geometry| geometry.identifier.as_ref() == geometry::heads::STATUE_GEOMETRIES[0])?;
    let model = geometry::heads::HeadModel::build_tree(geometry, None, 1.0, false)?;
    let faces: Vec<_> = model
        .boxes
        .iter()
        .flat_map(geometry::heads::HeadBox::faces)
        .collect();
    let [body, eyes] = geometry::heads::statue_textures(age);
    // Equal-depth opaque samples keep the first layer in the offline rasterizer.
    Some(
        [eyes, body]
            .map(|texture| Part {
                texture,
                logical: model.texture,
                faces: faces.clone(),
                color: [255; 3],
            })
            .into(),
    )
}

/// A head in the ordinary cube frame, from `geometries` for dragon and piglin heads. Like other
/// block items' fronts, the face turns south, and the head's uninflated bounds fill the cube
/// (an 8-texel head box spans it). Both are provisional until vanilla's skull GUI transform is
/// measured.
fn skull(skull_type: u8, geometries: &[EntityGeometry]) -> Option<Part> {
    let (model, texture) = geometry::heads::skull_model(skull_type, geometries)?;
    let mut bounds = [[f32::MAX; 3], [f32::MIN; 3]];
    for head_box in &model.boxes {
        let bare = geometry::heads::HeadBox {
            matrix: head_box.matrix,
            spec: head_box.spec.inflated(0.0),
        };
        for point in bare.faces().into_iter().flat_map(|(corners, _)| corners) {
            for axis in 0..3 {
                bounds[0][axis] = bounds[0][axis].min(point[axis]);
                bounds[1][axis] = bounds[1][axis].max(point[axis]);
            }
        }
    }
    let [min, max] = bounds;
    let extent = (0..3).map(|axis| max[axis] - min[axis]).fold(0.0, f32::max);
    if !extent.is_finite() || extent <= 0.0 {
        return None;
    }
    let center = [0, 2].map(|axis| (min[axis] + max[axis]) / 2.0);
    let faces = model
        .boxes
        .iter()
        .flat_map(geometry::heads::HeadBox::faces)
        .map(|(corners, texels)| {
            // A half turn about Y, centered, bottom on the floor.
            let corners = corners.map(|[x, y, z]| {
                [
                    (center[0] - x) / extent + 0.5,
                    (y - min[1]) / extent,
                    (center[1] - z) / extent + 0.5,
                ]
            });
            (corners, texels)
        })
        .collect();
    Some(Part {
        texture: texture.to_owned(),
        logical: model.texture,
        faces,
        color: [255; 3],
    })
}

/// Groups the authored lectern faces by their terrain texture.
fn lectern() -> Vec<Part> {
    let mut parts: Vec<Part> = Vec::new();
    for (texture_index, (corners, texels)) in geometry::lectern_faces() {
        let texture = geometry::LECTERN_TEXTURES[texture_index];
        let face = (
            corners.map(|[x, y, z]| [(x + 8.0) / 16.0, y / 16.0, (z + 8.0) / 16.0]),
            texels,
        );
        if let Some(part) = parts.iter_mut().find(|part| part.texture == texture) {
            part.faces.push(face);
        } else {
            parts.push(Part {
                texture: texture.to_owned(),
                logical: [16.0; 2],
                faces: vec![face],
                color: [255; 3],
            });
        }
    }
    parts
}

#[cfg(test)]
mod tests;
