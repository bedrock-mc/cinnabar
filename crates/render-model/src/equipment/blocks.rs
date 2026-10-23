//! Block items held in hand and drawn in slots: full cubes with six independently authored
//! faces, and other shapes as their template geometry.

use std::{collections::BTreeMap, sync::Arc};

use assets::{
    BLOCK_ITEM_FACE_SIDE, BLOCK_ITEM_SHEET_GRID, BlockFace, BlockFlags, BlockOverlay,
    DIAGNOSTIC_MATERIAL, IconSprite, ItemVisualDefinitionRoute, Material, NO_ANIMATION,
    NetworkIdMode, RuntimeAssets, RuntimeEntityAssets, TextureArray, VisualKind, VisualSupport,
    compose_block_item_sheet,
};

const TILE: usize = BLOCK_ITEM_FACE_SIDE as usize;

#[cfg(test)]
mod tests;

/// Composed block-item sheets and the visual IDs that select them.
pub struct BlockSheets {
    pub sheets: Vec<IconSprite>,
    /// Block visual id to its sheet index.
    pub by_visual: BTreeMap<u32, usize>,
    /// Exact carried models with sheet-relative UVs, in centred block coordinates.
    pub models: BTreeMap<u32, Vec<crate::ActorRigVertex>>,
}

impl BlockSheets {
    /// Lets matching authored carried faces use canonical cube UVs instead of world-model UVs.
    /// Non-cube models retain their geometry when a carried sheet is present.
    pub fn prefer_carried_faces(
        &mut self,
        world: &RuntimeAssets,
        icons: &assets::RuntimeIconCatalog,
    ) {
        if icons.source_manifest_sha256() != world.provenance().source_manifest_sha256 {
            return;
        }
        for sheet in icons.block_sheets() {
            if assets::gui_item::full_cube_model_quads(world, sheet.visual).is_some() {
                self.models.remove(&sheet.visual.0);
            }
        }
    }
}

/// Shares face sheets across cube items independently of their terrain occlusion and alpha.
pub fn collect(world: &RuntimeAssets, entities: &RuntimeEntityAssets) -> BlockSheets {
    collect_shapes(world, entities, None)
}

/// Like [`collect`], adding the template geometry non-cube items hold as: the state the icon
/// carrier draws each item's inventory model from (a wall's post with arms). Items without a
/// model are drawn flat in the inventory and keep their sprite in hand.
pub fn collect_held(
    world: &RuntimeAssets,
    entities: &RuntimeEntityAssets,
    icons: &assets::RuntimeIconCatalog,
) -> BlockSheets {
    let states: BTreeMap<u32, u32> = icons
        .block_models()
        .iter()
        .map(|model| (model.sprite, model.visual.0))
        .collect();
    let display = |key: &assets::ItemVisualKey| {
        let sprite = u32::try_from(icons.lookup_index(&key.identifier, key.metadata)?).ok()?;
        states.get(&sprite).copied()
    };
    collect_shapes(world, entities, Some(&display))
}

/// Resolves an item's inventory display state while borrowing its startup catalog.
type DisplayState<'a> = dyn Fn(&assets::ItemVisualKey) -> Option<u32> + 'a;

/// Collects cube sheets, and template models when `display` is given.
fn collect_shapes(
    world: &RuntimeAssets,
    entities: &RuntimeEntityAssets,
    display: Option<&DisplayState<'_>>,
) -> BlockSheets {
    let mut sheets = Vec::new();
    let mut by_materials = BTreeMap::<[u32; 6], usize>::new();
    let mut by_visual = BTreeMap::new();
    let mut models = BTreeMap::new();
    if !world.provenance().is_complete() {
        return BlockSheets {
            sheets,
            by_visual,
            models,
        };
    }
    for definition in entities.item_visuals() {
        let ItemVisualDefinitionRoute::BlockItem { block_visual } = definition.route else {
            continue;
        };
        let visual = block_visual.0;
        if by_visual.contains_key(&visual) || visual as usize >= world.visual_count() {
            continue;
        }
        let model = (definition.key.identifier.as_ref() == assets::END_PORTAL_FRAME_IDENTIFIER)
            .then(|| frame_model(world, visual))
            .flatten()
            .or_else(|| cube_model(world, visual));
        let cube = model
            .is_none()
            .then(|| cube_materials(world, visual))
            .flatten();
        let model = model.or_else(|| {
            // Items the inventory draws flat (torches, plants, panes) stay sprites in hand.
            let display = display.filter(|_| cube.is_none())?;
            let shown = display(&definition.key)
                .filter(|shown| (*shown as usize) < world.visual_count())?;
            template_model(world, shown)
        });
        let Some(materials) = model.as_ref().map(|(materials, _)| *materials).or(cube) else {
            continue;
        };
        let index = match by_materials.get(&materials) {
            Some(index) => *index,
            None => {
                let Some(sheet) = compose_sheet(world, &materials) else {
                    continue;
                };
                sheets.push(sheet);
                by_materials.insert(materials, sheets.len() - 1);
                sheets.len() - 1
            }
        };
        by_visual.insert(visual, index);
        if let Some((_, vertices)) = model {
            models.insert(visual, vertices);
        }
    }
    BlockSheets {
        sheets,
        by_visual,
        models,
    }
}

/// Reuses the exact unfilled portal frame template instead of inventing a full cube.
fn frame_model(
    world: &RuntimeAssets,
    visual: u32,
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    if block.kind() != VisualKind::Model || block.support() != VisualSupport::Exact {
        return None;
    }
    let template = world
        .model_templates()
        .get(block.model_template()? as usize)?;
    if template.flags != 0 || template.quad_count != BlockFace::ALL.len() as u32 {
        return None;
    }
    let quads = world.model_quads().get(
        template.quad_start as usize..template.quad_start as usize + template.quad_count as usize,
    )?;
    sheet_model(quads)
}

/// Retains full-cube models' directional UVs when world art supplies the held faces.
fn cube_model(
    world: &RuntimeAssets,
    visual: u32,
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    if world
        .resolve(NetworkIdMode::Sequential, visual)
        .animation()
        .is_some()
    {
        return None;
    }
    sheet_model(assets::gui_item::full_cube_model_quads(
        world,
        assets::BlockVisualId(visual),
    )?)
}

/// Places six canonical model faces on the shared carried sheet without changing their UVs.
fn sheet_model(quads: &[assets::ModelQuad]) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let rects = face_rects([0.0, 0.0, 1.0, 1.0]);
    let mut materials = [0; 6];
    let mut vertices = Vec::with_capacity(36);
    for (index, quad) in quads.iter().enumerate() {
        let face = index;
        if quad.flags & assets::MODEL_QUAD_FLAG_FACE_MASK
            != BlockFace::ALL[face].model_quad_face_id()
        {
            return None;
        }
        materials[face] = quad.material;
        let rect = rects[face];
        let normal = match BlockFace::ALL[face] {
            BlockFace::West => [-1.0, 0.0, 0.0],
            BlockFace::East => [1.0, 0.0, 0.0],
            BlockFace::Down => [0.0, -1.0, 0.0],
            BlockFace::Up => [0.0, 1.0, 0.0],
            BlockFace::North => [0.0, 0.0, -1.0],
            BlockFace::South => [0.0, 0.0, 1.0],
        };
        for corner in [0, 1, 2, 0, 2, 3] {
            let uv = std::array::from_fn(|axis| {
                rect[axis]
                    + f32::from(quad.uvs[corner][axis]) / 4096.0 * (rect[axis + 2] - rect[axis])
            });
            vertices.push(crate::ActorRigVertex {
                position: quad.positions[corner]
                    .map(|coordinate| f32::from(coordinate) / 256.0 - 0.5),
                normal,
                uv,
                back_uv: uv,
                bone_index: 0,
                surface: crate::ActorRigSurface::SINGLE_FACE,
                glint_origin: [rect[0], rect[1]],
            });
        }
    }
    Some((materials, vertices))
}

/// The geometry a non-cube block item (slab, stairs, fence, anvil) holds as: its template's
/// quads, as the inventory model draws them, with up to six distinct face materials composed
/// into one sheet, in centred block coordinates. `None` keeps the sprite: a refused state, more
/// than six materials, or a UV wrapping past its tile, which one sheet tile cannot hold.
fn template_model(
    world: &RuntimeAssets,
    visual: u32,
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    if !block.is_known()
        || block.kind() != VisualKind::Model
        || !matches!(
            block.support(),
            VisualSupport::Exact | VisualSupport::VanillaFallback
        )
        || block.animation().is_some()
        || block
            .model_template()
            .and_then(|id| world.model_templates().get(id as usize))
            .is_some_and(|template| template.flags & assets::MODEL_TEMPLATE_FLAG_PANE != 0)
    {
        return None;
    }
    let faces = BlockFace::ALL.map(|face| block.face(face).material_id());
    let quads = assets::gui_item::block_item_quads(
        block.kind(),
        faces,
        block.model_template(),
        world.model_templates(),
        world.model_quads(),
    )
    .ok()?;
    template_quads(&quads)
}

/// Maps bounded template geometry into the same material sheet used by held cubes.
fn template_quads(
    quads: &[assets::gui_item::GuiBlockQuad],
) -> Option<([u32; 6], Vec<crate::ActorRigVertex>)> {
    let mut materials: Vec<u32> = Vec::with_capacity(BlockFace::ALL.len());
    for quad in quads {
        let outside = quad
            .uvs
            .iter()
            .flatten()
            .any(|component| !(0.0..=1.0).contains(component));
        if quad.material == DIAGNOSTIC_MATERIAL || outside {
            return None;
        }
        if !materials.contains(&quad.material) {
            materials.push(quad.material);
        }
    }
    if materials.is_empty() || materials.len() > BlockFace::ALL.len() {
        return None;
    }
    let slots: [u32; 6] = std::array::from_fn(|slot| *materials.get(slot).unwrap_or(&materials[0]));
    let rects = face_rects([0.0, 0.0, 1.0, 1.0]);
    let mut vertices = Vec::with_capacity(quads.len() * 6);
    for quad in quads {
        let rect = rects[materials.iter().position(|id| *id == quad.material)?];
        let [a, b, c, _] = quad.corners.map(glam::Vec3::from);
        let normal = (b - a).cross(c - a).normalize_or_zero().to_array();
        for corner in [0, 1, 2, 0, 2, 3] {
            let uv = std::array::from_fn(|axis| {
                rect[axis] + quad.uvs[corner][axis] * (rect[axis + 2] - rect[axis])
            });
            vertices.push(crate::ActorRigVertex {
                position: quad.corners[corner].map(|coordinate| coordinate - 0.5),
                normal,
                uv,
                back_uv: uv,
                bone_index: 0,
                surface: crate::ActorRigSurface::SINGLE_FACE,
                glint_origin: [rect[0], rect[1]],
            });
        }
    }
    Some((slots, vertices))
}

/// Selects exact cube geometry, including the carrier's transparent cube templates.
fn cube_materials(world: &RuntimeAssets, visual: u32) -> Option<[u32; 6]> {
    let block = world.resolve(NetworkIdMode::Sequential, visual);
    let cube = match block.kind() {
        VisualKind::Cube => block.flags().contains(BlockFlags::CUBE_GEOMETRY),
        VisualKind::Model => block
            .model_template()
            .and_then(|id| world.model_templates().get(id as usize))
            .is_some_and(|template| {
                template.flags & assets::MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE != 0
            }),
        _ => false,
    };
    if !block.is_known()
        || !cube
        || block.support() != VisualSupport::Exact
        || block.animation().is_some()
    {
        return None;
    }
    let mut materials = [0; 6];
    for (slot, face) in materials.iter_mut().zip(BlockFace::ALL) {
        *slot = block.face(face).material_id();
        if *slot == DIAGNOSTIC_MATERIAL {
            return None;
        }
    }
    Some(materials)
}

/// Composes the six admitted face materials into their shared sheet layout.
fn compose_sheet(world: &RuntimeAssets, materials: &[u32; 6]) -> Option<IconSprite> {
    let mut tiles = Vec::with_capacity(BlockFace::ALL.len());
    for id in materials {
        let material = world.materials().get(*id as usize)?;
        let page = world
            .texture_pages()
            .get(material.texture.page() as usize)?;
        tiles.push(face_tile(
            material,
            &page.texture,
            *world.texture_source_size(material.texture).iter().max()?,
            world.texture_source_grid(material.texture),
        )?);
    }
    compose_block_item_sheet(&tiles.try_into().ok()?)
}

/// A session cube's face sheet, including blended and cutout cube templates.
pub fn overlay_sheet(
    overlay: &BlockOverlay,
    visual: usize,
    base: Option<&RuntimeAssets>,
) -> Option<IconSprite> {
    overlay_cube_sheet(overlay, visual, base, &|_| None)
}

/// Resolves a cube's carried faces with the caller's admitted inventory tint.
fn overlay_cube_sheet(
    overlay: &BlockOverlay,
    visual: usize,
    base: Option<&RuntimeAssets>,
    tint: &dyn Fn(u32) -> Option<[u8; 3]>,
) -> Option<IconSprite> {
    let block = overlay.visuals.get(visual)?;
    let cube = match block.kind {
        VisualKind::Cube => block.flags.contains(BlockFlags::CUBE_GEOMETRY),
        VisualKind::Model => overlay
            .model_templates
            .get(block.model_template as usize)
            .is_some_and(|template| {
                template.flags & assets::MODEL_TEMPLATE_FLAG_TRANSPARENT_CUBE != 0
            }),
        _ => false,
    };
    let carried = overlay
        .carried_cube_faces
        .binary_search_by_key(&(visual as u32), |entry| entry.0)
        .ok()
        .map(|index| overlay.carried_cube_faces[index].1);
    let faces = if let Some(faces) = carried {
        faces
    } else if cube && block.support == VisualSupport::Exact {
        block.faces
    } else {
        return None;
    };
    if block.animation != NO_ANIMATION {
        return None;
    }
    overlay_face_sheet_tinted(overlay, faces, base, tint)
}

/// Composes a session block model and its material sheet in centred block coordinates.
/// Unsupported material counts and wrapped UVs keep the caller's sprite fallback.
pub fn overlay_item(
    overlay: &BlockOverlay,
    visual: usize,
    base: Option<&RuntimeAssets>,
    tint: &dyn Fn(u32) -> Option<[u8; 3]>,
) -> Option<(IconSprite, Option<Vec<crate::ActorRigVertex>>)> {
    if let Some(sheet) = overlay_cube_sheet(overlay, visual, base, tint) {
        return Some((sheet, None));
    }
    let block = overlay.visuals.get(visual)?;
    if block.kind != VisualKind::Model
        || !matches!(
            block.support,
            VisualSupport::Exact | VisualSupport::VanillaFallback
        )
        || block.animation != NO_ANIMATION
    {
        return None;
    }
    let quads = assets::gui_item::block_item_quads(
        block.kind,
        block.faces,
        Some(block.model_template),
        &overlay.model_templates,
        &overlay.model_quads,
    )
    .ok()?;
    let (materials, vertices) = template_quads(&quads)?;
    Some((
        overlay_face_sheet_tinted(overlay, materials, base, tint)?,
        Some(vertices),
    ))
}

/// Splits a held block sheet into tile-local dropped materials, preserving glint coverage.
/// Vertices must contain complete quads in the held model's triangle order.
pub fn dropped_model(
    sheet: crate::DroppedItemSprite,
    vertices: &[crate::ActorRigVertex],
) -> Option<crate::DroppedItemBlock> {
    if vertices.is_empty() || !vertices.len().is_multiple_of(6) {
        return None;
    }
    let grid = BLOCK_ITEM_SHEET_GRID.map(u32::from);
    let [width, height] = [sheet.width / grid[0], sheet.height / grid[1]];
    if width == 0
        || height == 0
        || sheet.width != width * grid[0]
        || sheet.height != height * grid[1]
        || sheet.rgba8.len() != sheet.width as usize * sheet.height as usize * 4
    {
        return None;
    }
    let mut materials = Vec::with_capacity(BlockFace::ALL.len());
    for tile in 0..BlockFace::ALL.len() as u32 {
        let origin = [tile % grid[0] * width, tile / grid[0] * height];
        let mut pixels = Vec::with_capacity((width * height * 4) as usize);
        for y in origin[1]..origin[1] + height {
            let start = ((y * sheet.width + origin[0]) * 4) as usize;
            pixels.extend_from_slice(&sheet.rgba8[start..start + width as usize * 4]);
        }
        materials.push((
            crate::DroppedItemSprite {
                width,
                height,
                rgba8: pixels.into(),
            },
            crate::OPAQUE_WHITE,
        ));
    }
    let mut quads = Vec::with_capacity(vertices.len() / 6);
    for vertices in vertices.as_chunks::<6>().0 {
        let order = if vertices[4] == vertices[2] {
            [0, 1, 2, 5]
        } else {
            [0, 4, 1, 2]
        };
        let corners = order.map(|index| vertices[index]);
        if corners.iter().any(|vertex| {
            vertex
                .position
                .iter()
                .chain(&vertex.uv)
                .any(|value| !value.is_finite())
                || vertex.position.iter().any(|value| {
                    let fixed = (value + 0.5) * 256.0;
                    fixed < f32::from(i16::MIN) || fixed > f32::from(i16::MAX)
                })
                || vertex.uv.iter().any(|value| !(0.0..=1.0).contains(value))
        }) {
            return None;
        }
        let origin = corners[0].glint_origin;
        let tile = std::array::from_fn::<_, 2, _>(|axis| {
            (origin[axis] * grid[axis] as f32).round() as u32
        });
        if tile.iter().zip(grid).any(|(&tile, size)| tile >= size)
            || corners.iter().any(|vertex| vertex.glint_origin != origin)
        {
            return None;
        }
        let local_uvs = corners.map(|vertex| {
            std::array::from_fn::<_, 2, _>(|axis| {
                (vertex.uv[axis] - origin[axis]) * grid[axis] as f32
            })
        });
        if local_uvs
            .iter()
            .flatten()
            .any(|value| !(-0.0001..=1.0001).contains(value))
        {
            return None;
        }
        quads.push(assets::ModelQuad {
            positions: corners.map(|vertex| {
                vertex
                    .position
                    .map(|value| ((value + 0.5) * 256.0).round() as i16)
            }),
            uvs: local_uvs.map(|uv| {
                uv.map(|value| {
                    (value.clamp(0.0, 1.0) * f32::from(assets::MODEL_UV_SCALE)).round() as u16
                })
            }),
            material: tile[1] * grid[0] + tile[0],
            flags: 0,
        });
    }
    Some(crate::DroppedItemBlock {
        materials: materials.into(),
        quads: quads.into(),
        rotation: 0,
    })
}

/// Composes carried cube faces from overlay-local materials for inventory and held rendering.
pub fn overlay_face_sheet(
    overlay: &BlockOverlay,
    faces: [u32; 6],
    base: Option<&RuntimeAssets>,
) -> Option<IconSprite> {
    overlay_face_sheet_tinted(overlay, faces, base, &|_| None)
}

/// Bakes resolved per-material tints into shared held faces; unknown tint routes stay absent.
fn overlay_face_sheet_tinted(
    overlay: &BlockOverlay,
    faces: [u32; 6],
    base: Option<&RuntimeAssets>,
    tint: &dyn Fn(u32) -> Option<[u8; 3]>,
) -> Option<IconSprite> {
    let mut tiles = Vec::with_capacity(BlockFace::ALL.len());
    for id in faces {
        let material = overlay
            .materials
            .get(id as usize)
            .filter(|_| id != DIAGNOSTIC_MATERIAL)?;
        let mut material = *material;
        let color = if material.flags & assets::MATERIAL_FLAG_TINT_MASK != 0 {
            Some(tint(material.flags)?)
        } else {
            None
        };
        material.flags &= !assets::MATERIAL_FLAG_TINT_MASK;
        let mut tile = overlay_face_tile(overlay, &material, base)?;
        if let Some(color) = color {
            for pixel in Arc::make_mut(&mut tile.rgba8).as_chunks_mut::<4>().0 {
                for axis in 0..3 {
                    pixel[axis] = (u16::from(pixel[axis]) * u16::from(color[axis]) / 255) as u8;
                }
            }
        }
        tiles.push(tile);
    }
    compose_block_item_sheet(&tiles.try_into().ok()?)
}

/// Selects mips in exposed source pixels and resamples the admitted grid into a held face.
fn overlay_face_tile(
    overlay: &BlockOverlay,
    material: &Material,
    base: Option<&RuntimeAssets>,
) -> Option<IconSprite> {
    let texture = overlay.texture_page(material.texture.page(), base)?;
    if material.texture.page() < overlay.base_texture_pages {
        return face_tile(material, texture, texture.mips.first()?.size, 0);
    }
    let flat = overlay.flat_texture_layer(material.texture)?;
    let base = texture.mips.first()?;
    let grid = overlay.texture_source_grids.get(flat).copied().unwrap_or(0);
    let source_side = match overlay.texture_source_sizes.get(flat) {
        Some(size) => u32::from(*size.iter().max()?),
        None => base.size.checked_shr(u32::from(grid))?,
    };
    face_tile(material, texture, source_side, grid)
}

/// Unresolved tint and animation need authored carried faces instead of a static sheet.
fn face_material_is_admitted(material: &Material) -> bool {
    material.flags & !assets::gui_item::GUI_BLOCK_MATERIAL_FLAGS == 0
        && material.animation == NO_ANIMATION
}

/// Samples the exposed source rectangle, using the same mip and UV-grid rules for all sheets.
fn face_tile(
    material: &Material,
    texture: &TextureArray,
    source_side: u32,
    grid: u8,
) -> Option<IconSprite> {
    if !face_material_is_admitted(material)
        || source_side == 0
        || material.texture.layer() >= texture.layers
    {
        return None;
    }
    let level = source_side
        .ilog2()
        .saturating_sub(TILE.ilog2())
        .min(assets::VANILLA_TERRAIN_MIP_COUNT - 1);
    let mip = texture.mips.get(level as usize)?;
    if mip.size == 0 || mip.size > assets::MAX_TILE_SIZE {
        return None;
    }
    let side = mip.size as usize;
    let exposed = side.checked_shr(u32::from(grid))?;
    if exposed == 0 {
        return None;
    }
    let bytes = side.checked_mul(side)?.checked_mul(4)?;
    let start = (material.texture.layer() as usize).checked_mul(bytes)?;
    let tile = mip.rgba8.get(start..start.checked_add(bytes)?)?;
    let mut pixels = Vec::with_capacity(TILE * TILE * 4);
    for y in 0..TILE {
        for x in 0..TILE {
            let offset = ((y * exposed / TILE) * side + x * exposed / TILE) * 4;
            pixels.extend_from_slice(&tile[offset..offset + 4]);
        }
    }
    Some(IconSprite {
        width: BLOCK_ITEM_FACE_SIDE,
        height: BLOCK_ITEM_FACE_SIDE,
        rgba8: pixels.into(),
    })
}

/// The `[u0, v0, u1, v1]` region of each face's tile within a sheet placed at `region`.
pub fn face_rects(region: [f32; 4]) -> [[f32; 4]; 6] {
    let (width, height) = (region[2] - region[0], region[3] - region[1]);
    let columns = usize::from(BLOCK_ITEM_SHEET_GRID[0]);
    let [grid_width, grid_height] = BLOCK_ITEM_SHEET_GRID.map(f32::from);
    std::array::from_fn(|face| {
        let (column, row) = ((face % columns) as f32, (face / columns) as f32);
        [
            region[0] + width * column / grid_width,
            region[1] + height * row / grid_height,
            region[0] + width * (column + 1.0) / grid_width,
            region[1] + height * (row + 1.0) / grid_height,
        ]
    })
}
