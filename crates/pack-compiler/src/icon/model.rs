//! Thumbnails for block items drawn in 3D that are not plain opaque cubes (slabs, stairs, walls,
//! glass, server custom blocks): the visual's isolated template quads through the cube
//! thumbnail's projection, depth-tested and alpha-tested. Provisional: vanilla's GUI tessellation of
//! connected shapes (fences, walls) differs and is not modelled.

use std::{borrow::Cow, sync::Arc};

use assets::gui_item::{CUBE_FACES, GUI_ITEM_SIDE};
use assets::{
    BlockFace, BlockOverlay, BlockVisualId, IconSprite, MATERIAL_FLAG_ALPHA_BLEND,
    MATERIAL_FLAG_ALPHA_CUTOUT, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT,
    MODEL_TEMPLATE_FLAG_FENCE_NETHER, MODEL_TEMPLATE_FLAG_FENCE_WOOD, Material, ModelQuad,
    ModelTemplate, NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets, TextureArray, TexturePage,
    VisualKind,
};

use super::cube::Reject;

pub(super) const SIDE: usize = 32;
/// Largest tile side sampled; session overlays resample to at most 128.
const MAX_TILE: usize = 128;

struct Face<'a> {
    corners: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    tile: Cow<'a, [u8]>,
    /// Square tile side in pixels.
    side: usize,
    blend: bool,
}

/// Where a visual's material tiles live.
#[derive(Clone, Copy)]
enum Textures<'a> {
    /// World carrier pages, indexed by each material's page.
    World(&'a [TexturePage]),
    /// A session overlay's one array, which its materials address as page 1.
    Overlay(&'a TextureArray),
}

#[derive(Clone, Copy)]
struct Parts<'a> {
    materials: &'a [Material],
    templates: &'a [ModelTemplate],
    quads: &'a [ModelQuad],
    textures: Textures<'a>,
}

pub(super) struct Model<'a> {
    faces: Vec<Face<'a>>,
}

impl<'a> Model<'a> {
    pub(super) fn read(world: &'a RuntimeAssets, visual: BlockVisualId) -> Result<Self, Reject> {
        if visual.0 as usize >= world.visual_count() {
            return Err(Reject::Geometry);
        }
        let block = world.resolve(NetworkIdMode::Sequential, visual.0);
        if !block.is_known() {
            return Err(Reject::Geometry);
        }
        let parts = Parts {
            materials: world.materials(),
            templates: world.model_templates(),
            quads: world.model_quads(),
            textures: Textures::World(world.texture_pages()),
        };
        let faces = BlockFace::ALL.map(|face| block.face(face).material_id());
        Self::build(parts, block.kind(), faces, block.model_template())
    }

    /// State `visual` of a session block overlay.
    pub(super) fn overlay(overlay: &'a BlockOverlay, visual: usize) -> Result<Self, Reject> {
        let block = overlay.visuals.get(visual).ok_or(Reject::Geometry)?;
        let texture = overlay.texture.as_ref().ok_or(Reject::Texture)?;
        let parts = Parts {
            materials: &overlay.materials,
            templates: &overlay.model_templates,
            quads: &overlay.model_quads,
            textures: Textures::Overlay(texture),
        };
        let template = (block.model_template != NO_MODEL_TEMPLATE).then_some(block.model_template);
        Self::build(parts, block.kind, block.faces, template)
    }

    fn build(
        parts: Parts<'a>,
        kind: VisualKind,
        materials: [u32; 6],
        template: Option<u32>,
    ) -> Result<Self, Reject> {
        let mut faces = Vec::new();
        match (kind, template) {
            (VisualKind::Cube, _) => {
                for face in BlockFace::ALL {
                    let (corners, uvs) = cube_face(face);
                    let (tile, side, blend) = tile(parts, materials[face as usize])?;
                    faces.push(Face {
                        corners,
                        uvs,
                        tile: Cow::Borrowed(tile),
                        side,
                        blend,
                    });
                }
            }
            (VisualKind::Model, Some(template)) => {
                let first = parts
                    .templates
                    .get(template as usize)
                    .ok_or(Reject::Geometry)?;
                // A fence item shows its post with east and west arms (connection mask 2 | 8).
                let offsets: &[usize] = if first.flags
                    & (MODEL_TEMPLATE_FLAG_FENCE_WOOD | MODEL_TEMPLATE_FLAG_FENCE_NETHER)
                    != 0
                {
                    &[0, 11]
                } else if first.flags & MODEL_TEMPLATE_FLAG_COMPOUND_NEXT != 0 {
                    &[0, 1]
                } else {
                    &[0]
                };
                for offset in offsets {
                    let template = parts
                        .templates
                        .get(template as usize + offset)
                        .ok_or(Reject::Geometry)?;
                    let start = template.quad_start as usize;
                    let quads = parts
                        .quads
                        .get(start..start + template.quad_count as usize)
                        .ok_or(Reject::Geometry)?;
                    for quad in quads {
                        faces.push(model_face(parts, quad)?);
                    }
                }
            }
            _ => return Err(Reject::Geometry),
        }
        if faces.is_empty() {
            return Err(Reject::Geometry);
        }
        Ok(Self { faces })
    }

    /// A cutout full cube from six 16x16 tiles in `BlockFace` order.
    pub(super) fn cube(tiles: [Box<[u8]>; 6]) -> Model<'static> {
        let faces = BlockFace::ALL
            .into_iter()
            .zip(tiles)
            .map(|(face, tile)| {
                let (corners, uvs) = cube_face(face);
                Face {
                    corners,
                    uvs,
                    tile: Cow::Owned(tile.into_vec()),
                    side: usize::from(assets::BLOCK_ITEM_FACE_SIDE),
                    blend: false,
                }
            })
            .collect();
        Model { faces }
    }

    pub(super) fn raster(&self) -> IconSprite {
        let mut pixels = vec![0u8; SIDE * SIDE * 4];
        let mut depth = vec![f32::INFINITY; SIDE * SIDE];
        let mut fragments = vec![Vec::new(); SIDE * SIDE];
        for face in &self.faces {
            let brightness = brightness(face.corners);
            let points = face.corners.map(project);
            for indices in [[0, 1, 2], [0, 2, 3]] {
                triangle(
                    &mut pixels,
                    &mut depth,
                    &mut fragments,
                    face,
                    indices.map(|i| points[i]),
                    indices.map(|i| face.uvs[i]),
                    brightness,
                );
            }
        }
        composite_fragments(&mut pixels, &depth, &mut fragments);
        IconSprite {
            width: SIDE as u16,
            height: SIDE as u16,
            rgba8: Arc::from(pixels),
        }
    }
}

fn tile(parts: Parts<'_>, id: u32) -> Result<(&[u8], usize, bool), Reject> {
    if id == assets::DIAGNOSTIC_MATERIAL {
        return Err(Reject::Material);
    }
    let material = parts.materials.get(id as usize).ok_or(Reject::Material)?;
    let alpha = MATERIAL_FLAG_ALPHA_BLEND | MATERIAL_FLAG_ALPHA_CUTOUT;
    // Tints, overlays and rotated UVs need per-biome or per-state data an icon lacks.
    if material.flags & !alpha != 0 {
        return Err(Reject::Material);
    }
    let array = match parts.textures {
        Textures::World(pages) => {
            &pages
                .get(material.texture.page() as usize)
                .ok_or(Reject::Texture)?
                .texture
        }
        Textures::Overlay(array) if material.texture.page() == 1 => array,
        Textures::Overlay(_) => return Err(Reject::Texture),
    };
    let mip = array.mips.first().ok_or(Reject::Texture)?;
    let side = mip.size as usize;
    if side == 0 || side > MAX_TILE || material.texture.layer() >= array.layers {
        return Err(Reject::Texture);
    }
    let bytes = side * side * 4;
    let start = material.texture.layer() as usize * bytes;
    let tile = mip.rgba8.get(start..start + bytes).ok_or(Reject::Texture)?;
    Ok((tile, side, material.flags & MATERIAL_FLAG_ALPHA_BLEND != 0))
}

fn model_face<'a>(parts: Parts<'a>, quad: &ModelQuad) -> Result<Face<'a>, Reject> {
    let (tile, side, blend) = tile(parts, quad.material)?;
    Ok(Face {
        tile: Cow::Borrowed(tile),
        side,
        corners: quad
            .positions
            .map(|point| point.map(|component| f32::from(component) / 256.0)),
        uvs: quad
            .uvs
            .map(|uv| uv.map(|component| f32::from(component) / 4096.0)),
        blend,
    })
}

/// A full-block face with the cube thumbnail's UV orientation.
fn cube_face(face: BlockFace) -> ([[f32; 3]; 4], [[f32; 2]; 4]) {
    let side_uv = [[0., 1.], [1., 1.], [1., 0.], [0., 0.]];
    match face {
        BlockFace::Up => (
            [[0., 1., 0.], [0., 1., 1.], [1., 1., 1.], [1., 1., 0.]],
            [[0., 0.], [0., 1.], [1., 1.], [1., 0.]],
        ),
        BlockFace::Down => (
            [[0., 0., 1.], [0., 0., 0.], [1., 0., 0.], [1., 0., 1.]],
            [[0., 0.], [0., 1.], [1., 1.], [1., 0.]],
        ),
        BlockFace::South => (
            [[0., 0., 1.], [1., 0., 1.], [1., 1., 1.], [0., 1., 1.]],
            side_uv,
        ),
        BlockFace::North => (
            [[1., 0., 0.], [0., 0., 0.], [0., 1., 0.], [1., 1., 0.]],
            side_uv,
        ),
        BlockFace::West => (
            [[0., 0., 0.], [0., 0., 1.], [0., 1., 1.], [0., 1., 0.]],
            side_uv,
        ),
        BlockFace::East => (
            [[1., 0., 1.], [1., 0., 0.], [1., 1., 0.], [1., 1., 1.]],
            side_uv,
        ),
    }
}

/// The cube thumbnail's side shading, chosen by the face normal's dominant axis.
fn brightness(corners: [[f32; 3]; 4]) -> f32 {
    let [a, b, c] = [corners[0], corners[1], corners[2]];
    let (u, v) = (
        [b[0] - a[0], b[1] - a[1], b[2] - a[2]],
        [c[0] - a[0], c[1] - a[1], c[2] - a[2]],
    );
    let normal = [
        (u[1] * v[2] - u[2] * v[1]).abs(),
        u[2] * v[0] - u[0] * v[2],
        (u[0] * v[1] - u[1] * v[0]).abs(),
    ];
    if normal[1].abs() >= normal[0] && normal[1].abs() >= normal[2] {
        CUBE_FACES[0].3
    } else if normal[0] >= normal[2] {
        CUBE_FACES[2].3
    } else {
        CUBE_FACES[1].3
    }
}

/// The cube thumbnail's projection at twice its scale, plus a view depth (smaller is nearer).
fn project(point: [f32; 3]) -> [f32; 3] {
    let [x, y, z] = assets::gui_item::project_cube(point);
    let scale = SIDE as f32 / GUI_ITEM_SIDE;
    [scale * x, scale * y, z]
}

fn edge(a: [f32; 3], b: [f32; 3], p: [f32; 2]) -> f32 {
    (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0])
}

fn triangle(
    pixels: &mut [u8],
    depth: &mut [f32],
    fragments: &mut [Vec<(f32, [u8; 4])>],
    face: &Face<'_>,
    mut p: [[f32; 3]; 3],
    mut uv: [[f32; 2]; 3],
    brightness: f32,
) {
    let area = edge(p[0], p[1], [p[2][0], p[2][1]]);
    if area < 0. {
        p.swap(1, 2);
        uv.swap(1, 2);
    }
    let area = area.abs();
    if !area.is_finite() || area == 0. {
        return;
    }
    for y in 0..SIDE {
        for x in 0..SIDE {
            let sample = [x as f32 + 0.5, y as f32 + 0.5];
            let weights = [
                edge(p[1], p[2], sample) / area,
                edge(p[2], p[0], sample) / area,
                edge(p[0], p[1], sample) / area,
            ];
            if !weights.iter().enumerate().all(|(index, &weight)| {
                let a = p[(index + 1) % 3];
                let b = p[(index + 2) % 3];
                weight > 0.0 || (weight == 0.0 && (b[1] > a[1] || (b[1] == a[1] && b[0] < a[0])))
            }) {
                continue;
            }
            let z = (0..3).map(|i| weights[i] * p[i][2]).sum::<f32>();
            let target = y * SIDE + x;
            if z >= depth[target] {
                continue;
            }
            let [u, v] = [0, 1].map(|axis| (0..3).map(|i| weights[i] * uv[i][axis]).sum::<f32>());
            let side = face.side;
            let tx = ((u.rem_euclid(1.) * side as f32) as usize).min(side - 1);
            let ty = ((v.rem_euclid(1.) * side as f32) as usize).min(side - 1);
            let texel = &face.tile[(ty * side + tx) * 4..][..4];
            if texel[3] < 128 && !(face.blend && texel[3] > 0) {
                continue;
            }
            let color = [
                (f32::from(texel[0]) * brightness).round().clamp(0., 255.) as u8,
                (f32::from(texel[1]) * brightness).round().clamp(0., 255.) as u8,
                (f32::from(texel[2]) * brightness).round().clamp(0., 255.) as u8,
                if face.blend { texel[3] } else { 255 },
            ];
            if color[3] < 255 {
                fragments[target].push((z, color));
            } else {
                depth[target] = z;
                pixels[target * 4..][..4].copy_from_slice(&color);
            }
        }
    }
}

/// Composites visible translucent samples from back to front over the nearest opaque sample.
fn composite_fragments(pixels: &mut [u8], depth: &[f32], fragments: &mut [Vec<(f32, [u8; 4])>]) {
    for (target, samples) in fragments.iter_mut().enumerate() {
        samples.sort_unstable_by(|left, right| right.0.total_cmp(&left.0));
        let output = &mut pixels[target * 4..][..4];
        for &(_, source) in samples.iter().filter(|(z, _)| *z < depth[target]) {
            let alpha = f32::from(source[3]) / 255.0;
            let previous = f32::from(output[3]) / 255.0;
            let combined = alpha + previous * (1.0 - alpha);
            for channel in 0..3 {
                output[channel] = ((f32::from(source[channel]) * alpha
                    + f32::from(output[channel]) * previous * (1.0 - alpha))
                    / combined)
                    .round() as u8;
            }
            output[3] = (combined * 255.0).round() as u8;
        }
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    #[test]
    fn review_translucent_faces_preserve_opaque_geometry_in_both_orders() {
        for reverse in [false, true] {
            let rear = Face {
                corners: [[0.0; 3]; 4],
                uvs: [[0.0; 2]; 4],
                tile: Cow::Owned(vec![0, 0, 255, 255]),
                side: 1,
                blend: false,
            };
            let front = Face {
                corners: [[0.0; 3]; 4],
                uvs: [[0.0; 2]; 4],
                tile: Cow::Owned(vec![255, 0, 0, 128]),
                side: 1,
                blend: true,
            };
            let mut layers = [(&rear, 2.0), (&front, 1.0)];
            if reverse {
                layers.reverse();
            }
            let mut pixels = vec![0; SIDE * SIDE * 4];
            let mut depth = vec![f32::INFINITY; SIDE * SIDE];
            let mut fragments = vec![Vec::new(); SIDE * SIDE];
            for (face, z) in layers {
                triangle(
                    &mut pixels,
                    &mut depth,
                    &mut fragments,
                    face,
                    [[0.0, 0.0, z], [4.0, 0.0, z], [0.0, 4.0, z]],
                    [[0.0; 2]; 3],
                    1.0,
                );
            }
            composite_fragments(&mut pixels, &depth, &mut fragments);
            assert_eq!(&pixels[..4], &[128, 0, 127, 255]);
        }
    }
}
