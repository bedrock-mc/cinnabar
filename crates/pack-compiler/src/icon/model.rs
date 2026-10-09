//! Depth-tested and alpha-tested block thumbnails from shared GUI geometry.
//! Includes slabs, stairs, walls, glass and server custom blocks.

use std::{borrow::Cow, sync::Arc};

use assets::gui_item::{
    GUI_BLOCK_ALPHA_THRESHOLD, GUI_ITEM_SIDE, GuiBlockReject, block_item_quads, cube_face,
    face_brightness, material_tile,
};
use assets::{
    BlockFace, BlockOverlay, BlockVisualId, IconSprite, Material, ModelQuad, ModelTemplate,
    NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets, TextureArray, TexturePage, VisualKind,
};

use super::cube::Reject;

pub(super) const SIDE: usize = 32;

struct Face<'a> {
    corners: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    tile: Cow<'a, [u8]>,
    size: [usize; 2],
    blend: bool,
}

/// Where a visual's material tiles live.
#[derive(Clone, Copy)]
enum Textures<'a> {
    /// World carrier pages, indexed by each material's page.
    World(&'a [TexturePage]),
    /// A session overlay's one array, which its materials address as page 1.
    Overlay(&'a TextureArray, &'a [u8]),
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
    projection: fn([f32; 3]) -> [f32; 3],
    light: f32,
    face_lighting: bool,
}

impl<'a> Model<'a> {
    /// Builds a model from source rectangles and its own inventory projection.
    pub(super) fn textured(
        quads: impl IntoIterator<Item = ([[f32; 3]; 4], [[f32; 2]; 4], &'a IconSprite)>,
        projection: fn([f32; 3]) -> [f32; 3],
        light: f32,
    ) -> Self {
        let faces = quads
            .into_iter()
            .map(|(corners, uvs, sprite)| Face {
                corners,
                uvs,
                tile: Cow::Borrowed(&sprite.rgba8),
                size: [usize::from(sprite.width), usize::from(sprite.height)],
                blend: false,
            })
            .collect();
        Self {
            faces,
            projection,
            light,
            face_lighting: true,
        }
    }

    /// Preserves flat model lighting for inventory materials without face shading.
    pub(super) fn unshaded(mut self) -> Self {
        self.face_lighting = false;
        self
    }

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
            textures: Textures::Overlay(texture, &overlay.texture_source_grids),
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
        let faces = block_item_quads(kind, materials, template, parts.templates, parts.quads)
            .map_err(reject)?
            .into_iter()
            .map(|quad| {
                let (tile, side, blend, uv_scale) = tile(parts, quad.material)?;
                Ok(Face {
                    corners: quad.corners,
                    uvs: quad.uvs.map(|uv| uv.map(|component| component * uv_scale)),
                    tile: Cow::Borrowed(tile),
                    size: [side; 2],
                    blend,
                })
            })
            .collect::<Result<Vec<_>, Reject>>()?;
        Ok(Self {
            faces,
            projection: assets::gui_item::project_cube,
            light: 1.0,
            face_lighting: true,
        })
    }

    /// A full cube from six 16x16 tiles and their alpha modes in `BlockFace` order.
    pub(super) fn cube(tiles: [Box<[u8]>; 6], blending: [bool; 6]) -> Model<'static> {
        let faces = BlockFace::ALL
            .into_iter()
            .zip(tiles)
            .map(|(face, tile)| {
                let (corners, uvs) = cube_face(face);
                Face {
                    corners,
                    uvs,
                    tile: Cow::Owned(tile.into_vec()),
                    size: [usize::from(assets::BLOCK_ITEM_FACE_SIDE); 2],
                    blend: blending[face as usize],
                }
            })
            .collect();
        Model {
            faces,
            projection: assets::gui_item::project_cube,
            light: 1.0,
            face_lighting: true,
        }
    }

    pub(super) fn raster(&self) -> IconSprite {
        let mut pixels = vec![0u8; SIDE * SIDE * 4];
        let mut depth = vec![f32::INFINITY; SIDE * SIDE];
        let mut fragments = vec![Vec::new(); SIDE * SIDE];
        for face in &self.faces {
            let brightness = if self.face_lighting {
                face_brightness(face.corners)
            } else {
                1.0
            } * self.light;
            let points = face.corners.map(|point| {
                let [x, y, z] = (self.projection)(point);
                let scale = SIDE as f32 / GUI_ITEM_SIDE;
                [scale * x, scale * y, z]
            });
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

/// Reads a material tile and preserves a session overlay's source texture scale.
fn tile(parts: Parts<'_>, id: u32) -> Result<(&[u8], usize, bool, f32), Reject> {
    let (tile, side, blend) = material_tile(parts.materials, id, |page| match parts.textures {
        Textures::World(pages) => pages.get(page as usize).map(|page| &page.texture),
        Textures::Overlay(array, _) => (page == 1).then_some(array),
    })
    .map_err(reject)?;
    let material = &parts.materials[id as usize];
    let grid = match parts.textures {
        Textures::World(_) => 0,
        Textures::Overlay(_, grids) => grids
            .get(material.texture.layer() as usize)
            .copied()
            .unwrap_or(0),
    };
    if side
        .checked_shr(u32::from(grid))
        .is_none_or(|size| size == 0)
    {
        return Err(Reject::Texture);
    }
    Ok((tile, side, blend, 1.0 / (1u32 << grid) as f32))
}

/// Maps shared GUI geometry errors to the thumbnail baker's errors.
fn reject(reason: GuiBlockReject) -> Reject {
    match reason {
        GuiBlockReject::Geometry => Reject::Geometry,
        GuiBlockReject::Material => Reject::Material,
        GuiBlockReject::Texture => Reject::Texture,
    }
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
            let [width, height] = face.size;
            let tx = ((u.rem_euclid(1.) * width as f32) as usize).min(width - 1);
            let ty = ((v.rem_euclid(1.) * height as f32) as usize).min(height - 1);
            let texel = &face.tile[(ty * width + tx) * 4..][..4];
            if texel[3] < GUI_BLOCK_ALPHA_THRESHOLD && !(face.blend && texel[3] > 0) {
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
                size: [1; 2],
                blend: false,
            };
            let front = Face {
                corners: [[0.0; 3]; 4],
                uvs: [[0.0; 2]; 4],
                tile: Cow::Owned(vec![255, 0, 0, 128]),
                size: [1; 2],
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
