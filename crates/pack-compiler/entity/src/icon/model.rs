//! Depth-tested and alpha-tested block thumbnails from shared GUI geometry.
//! Includes slabs, stairs, walls, glass and server custom blocks.

use std::sync::Arc;

use assets::gui_item::{
    GUI_BLOCK_ALPHA_THRESHOLD, GUI_ITEM_SIDE, GuiBlockReject, block_item_quads, cube_face,
    face_brightness, material_tile,
};
use assets::{
    BlockFace, BlockOverlay, BlockVisualId, IconSprite, Material, ModelQuad, ModelTemplate,
    NO_MODEL_TEMPLATE, NetworkIdMode, RuntimeAssets, TexturePage, VisualKind,
};

use super::cube::Reject;

pub(super) const SIDE: usize = 32;

struct Face<'a> {
    corners: [[f32; 3]; 4],
    uvs: [[f32; 2]; 4],
    tile: Pixels<'a>,
    size: [usize; 2],
    blend: bool,
    face_dimming: bool,
}

/// Carrier texels remain borrowed; processed texels share one immutable allocation.
#[derive(Clone)]
enum Pixels<'a> {
    Borrowed(&'a [u8]),
    Shared(Arc<[u8]>),
}

impl Pixels<'_> {
    /// Exposes texels independently of their carrier or material-cache owner.
    fn as_slice(&self) -> &[u8] {
        match self {
            Self::Borrowed(pixels) => pixels,
            Self::Shared(pixels) => pixels,
        }
    }
}

/// Sampled texels and their inventory material properties.
struct Tile<'a> {
    pixels: Pixels<'a>,
    side: usize,
    blend: bool,
    uv_scale: f32,
}

/// Where a visual's material tiles live.
#[derive(Clone, Copy)]
enum Textures<'a> {
    /// World carrier pages, indexed by each material's page.
    World(&'a [TexturePage]),
    /// A session overlay's shards, addressed after its reserved base pages.
    Overlay(&'a BlockOverlay, Option<&'a RuntimeAssets>),
}

#[derive(Clone, Copy)]
struct Parts<'a> {
    materials: &'a [Material],
    templates: &'a [ModelTemplate],
    quads: &'a [ModelQuad],
    textures: Textures<'a>,
    tint: Option<&'a dyn Fn(u32) -> Option<[u8; 3]>>,
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
                tile: Pixels::Borrowed(&sprite.rgba8),
                size: [usize::from(sprite.width), usize::from(sprite.height)],
                blend: false,
                face_dimming: true,
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
            tint: None,
        };
        let faces = BlockFace::ALL.map(|face| block.face(face).material_id());
        Self::build(parts, block.kind(), faces, block.model_template())
    }

    /// State `visual` of a session block overlay.
    pub(super) fn overlay(
        overlay: &'a BlockOverlay,
        visual: usize,
        base: Option<&'a RuntimeAssets>,
        tint: Option<&'a dyn Fn(u32) -> Option<[u8; 3]>>,
    ) -> Result<Self, Reject> {
        let block = overlay.visuals.get(visual).ok_or(Reject::Geometry)?;
        let parts = Parts {
            materials: &overlay.materials,
            templates: &overlay.model_templates,
            quads: &overlay.model_quads,
            textures: Textures::Overlay(overlay, base),
            tint,
        };
        let template = (block.model_template != NO_MODEL_TEMPLATE).then_some(block.model_template);
        Self::build(parts, block.kind, block.faces, template)
    }

    /// Draws carried cube faces from overlay pixels without cloning the texture page.
    pub(super) fn overlay_cube(
        overlay: &'a BlockOverlay,
        faces: [u32; 6],
        base: Option<&'a RuntimeAssets>,
    ) -> Result<Self, Reject> {
        let parts = Parts {
            materials: &overlay.materials,
            templates: &overlay.model_templates,
            quads: &overlay.model_quads,
            textures: Textures::Overlay(overlay, base),
            tint: None,
        };
        Self::build(parts, VisualKind::Cube, faces, None)
    }

    fn build(
        parts: Parts<'a>,
        kind: VisualKind,
        materials: [u32; 6],
        template: Option<u32>,
    ) -> Result<Self, Reject> {
        let mut tiles = std::collections::BTreeMap::new();
        if kind == VisualKind::Cube {
            // A cube remains eligible only when all faces resolve, including those
            // hidden by the inventory camera, just as on the opaque thumbnail path.
            for material in materials {
                if let std::collections::btree_map::Entry::Vacant(entry) = tiles.entry(material) {
                    entry.insert(tile(parts, material)?);
                }
            }
        }
        let faces = block_item_quads(kind, materials, template, parts.templates, parts.quads)
            .map_err(reject)?
            .into_iter()
            .map(|quad| {
                let tile = match tiles.entry(quad.material) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(tile(parts, quad.material)?)
                    }
                };
                Ok(Face {
                    corners: quad.corners,
                    uvs: quad
                        .uvs
                        .map(|uv| uv.map(|component| component * tile.uv_scale)),
                    tile: tile.pixels.clone(),
                    size: [tile.side; 2],
                    blend: tile.blend,
                    face_dimming: parts.materials[quad.material as usize].flags
                        & assets::MATERIAL_FLAG_DISABLE_FACE_DIMMING
                        == 0,
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

    /// A GUI cube's outward surfaces from six tiles and alpha modes in `BlockFace` order.
    pub(super) fn cube(tiles: [Box<[u8]>; 6], blending: [bool; 6]) -> Model<'static> {
        let faces = BlockFace::ALL
            .into_iter()
            .zip(tiles)
            .filter(|(face, _)| {
                assets::gui_item::CUBE_FACES
                    .iter()
                    .any(|(visible, _, _, _)| visible == face)
            })
            .map(|(face, tile)| {
                let (corners, uvs) = cube_face(face);
                Face {
                    corners,
                    uvs,
                    tile: Pixels::Shared(Arc::from(tile)),
                    size: [usize::from(assets::BLOCK_ITEM_FACE_SIDE); 2],
                    blend: blending[face as usize],
                    face_dimming: true,
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
            let brightness = if self.face_lighting && face.face_dimming {
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
fn tile(parts: Parts<'_>, id: u32) -> Result<Tile<'_>, Reject> {
    if id == assets::DIAGNOSTIC_MATERIAL {
        return Err(Reject::Material);
    }
    let material = parts.materials.get(id as usize).ok_or(Reject::Material)?;
    let tint = if material.flags & assets::MATERIAL_FLAG_TINT_MASK != 0 {
        Some(
            parts
                .tint
                .and_then(|resolve| resolve(material.flags))
                .ok_or(Reject::Material)?,
        )
    } else {
        None
    };
    let mut sampled = *material;
    if tint.is_some() {
        sampled.flags &= !assets::MATERIAL_FLAG_TINT_MASK;
    }
    let (tile, side, blend) =
        material_tile(&[Material::unvaried(), sampled], 1, |page| {
            match parts.textures {
                Textures::World(pages) => pages.get(page as usize).map(|page| &page.texture),
                Textures::Overlay(overlay, base) => overlay.texture_page(page, base),
            }
        })
        .map_err(reject)?;
    let tile = match tint {
        Some(tint) => {
            let mut pixels = tile.to_vec();
            pack_compile_input::apply_atlas_tint(&mut pixels, tint);
            Pixels::Shared(Arc::from(pixels))
        }
        None => Pixels::Borrowed(tile),
    };
    let grid = match parts.textures {
        Textures::World(_) => 0,
        Textures::Overlay(overlay, _) => overlay
            .flat_texture_layer(material.texture)
            .and_then(|flat| overlay.texture_source_grids.get(flat))
            .copied()
            .unwrap_or(0),
    };
    if side
        .checked_shr(u32::from(grid))
        .is_none_or(|size| size == 0)
    {
        return Err(Reject::Texture);
    }
    Ok(Tile {
        pixels: tile,
        side,
        blend,
        uv_scale: 1.0 / (1u32 << grid) as f32,
    })
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
            let texel = &face.tile.as_slice()[(ty * width + tx) * 4..][..4];
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
    fn cube_thumbnail_hides_rear_texels_through_front_holes() {
        for blend in [false, true] {
            let tiles = BlockFace::ALL.map(|face| {
                let visible = assets::gui_item::CUBE_FACES
                    .iter()
                    .any(|(front, _, _, _)| *front == face);
                let color = if visible {
                    [0, 0, 0, 0]
                } else {
                    [255, 0, 0, 255]
                };
                color
                    .repeat(usize::from(assets::BLOCK_ITEM_FACE_SIDE).pow(2))
                    .into_boxed_slice()
            });
            let icon = Model::cube(tiles, [blend; 6]).raster();
            assert!(
                icon.rgba8
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|pixel| pixel[3] == 0),
                "rear cube faces must not show through transparent front texels"
            );
        }
    }

    #[test]
    fn review_translucent_faces_preserve_opaque_geometry_in_both_orders() {
        for reverse in [false, true] {
            let rear = Face {
                corners: [[0.0; 3]; 4],
                uvs: [[0.0; 2]; 4],
                tile: Pixels::Shared(Arc::from([0, 0, 255, 255])),
                size: [1; 2],
                blend: false,
                face_dimming: true,
            };
            let front = Face {
                corners: [[0.0; 3]; 4],
                uvs: [[0.0; 2]; 4],
                tile: Pixels::Shared(Arc::from([255, 0, 0, 128])),
                size: [1; 2],
                blend: true,
                face_dimming: true,
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

    #[test]
    fn repeated_tinted_faces_share_one_material_tile() {
        let side = assets::gui_item::MAX_GUI_TILE_SIDE;
        let texture = assets::TextureArray {
            layers: 1,
            mips: vec![assets::TextureMip {
                size: side as u32,
                rgba8: [200, 200, 200, 255].repeat(side * side).into(),
            }]
            .into(),
        };
        let materials = [
            Material::unvaried(),
            Material {
                texture: assets::TextureRef::new(1, 0).unwrap(),
                flags: assets::MATERIAL_FLAG_GRASS_TINT,
                ..Material::unvaried()
            },
        ];
        let quad = ModelQuad {
            positions: [[0, 256, 0], [0, 256, 256], [256, 256, 256], [256, 256, 0]],
            uvs: [[0, 0], [0, 4096], [4096, 4096], [4096, 0]],
            material: 1,
            flags: 0,
        };
        let quads = vec![quad; 256 * BlockFace::ALL.len()];
        let templates = [ModelTemplate {
            quad_start: 0,
            quad_count: quads.len() as u32,
            flags: 0,
        }];
        let resolved = std::cell::Cell::new(0);
        let overlay = BlockOverlay {
            textures: vec![texture],
            ..Default::default()
        };
        let tint = |_| {
            resolved.set(resolved.get() + 1);
            Some([80, 180, 120])
        };
        let model = Model::build(
            Parts {
                materials: &materials,
                templates: &templates,
                quads: &quads,
                textures: Textures::Overlay(&overlay, None),
                tint: Some(&tint),
            },
            VisualKind::Model,
            [1; 6],
            Some(0),
        )
        .unwrap();
        assert_eq!(model.faces.len(), quads.len());
        let buffers = model
            .faces
            .iter()
            .map(|face| face.tile.as_slice().as_ptr())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            buffers.len(),
            1,
            "one retained processed texture per material"
        );
        assert_eq!(resolved.get(), 1, "one tint preparation per material");
        assert_eq!(model.faces[0].tile.as_slice().len(), side * side * 4);
    }
}
