//! Shared quads and texels for block thumbnails and native GUI meshes.
//! Fences and walls use carried shapes; vanilla gates turn into the carried item axis.

use crate::{
    BlockFace, MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT, MATERIAL_FLAG_DISABLE_AO,
    MATERIAL_FLAG_DISABLE_FACE_DIMMING, MATERIAL_FLAG_ISOTROPIC, MODEL_TEMPLATE_FLAG_FENCE_NETHER,
    MODEL_TEMPLATE_FLAG_FENCE_WOOD, MODEL_TEMPLATE_FLAG_GATE_AXIS_X,
    MODEL_TEMPLATE_FLAG_GATE_AXIS_Z, MODEL_TEMPLATE_FLAG_WALL, Material, ModelQuad, ModelTemplate,
    TextureArray, VisualKind,
};

use super::CUBE_FACES;

mod plants;

/// Largest material tile side drawn; session overlays resample to at most 128.
pub const MAX_GUI_TILE_SIDE: usize = 128;

/// Scale and design-pixel offset of the heavy core inventory preview.
const HEAVY_CORE_GUI_TRANSFORM: (f32, [f32; 2]) = (1.55, [-4.0, -0.8]);
/// Scale and design-pixel offset of a shelf inventory preview.
const SHELF_GUI_TRANSFORM: (f32, [f32; 2]) = (1.178, [1.4, 2.0]);

/// Flags whose material texels can be sampled without unresolved tint or UV transforms.
/// Lighting flags are retained for the renderer and do not alter the stored pixels.
pub const GUI_BLOCK_MATERIAL_FLAGS: u32 = MATERIAL_FLAG_ALPHA_BLEND
    | MATERIAL_FLAG_ALPHA_CUTOUT
    | MATERIAL_FLAG_ISOTROPIC
    | MATERIAL_FLAG_DISABLE_AO
    | MATERIAL_FLAG_DISABLE_FACE_DIMMING;

/// One textured quad of a block item, in block units with tile-relative UVs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GuiBlockQuad {
    pub corners: [[f32; 3]; 4],
    pub uvs: [[f32; 2]; 4],
    pub material: u32,
    pub two_sided: bool,
}

impl GuiBlockQuad {
    /// Keeps outward faces visible to the fixed inventory camera and authored two-sided planes.
    #[must_use]
    pub fn visible_in_gui(&self) -> bool {
        if self.two_sided {
            return true;
        }
        let [a, b, c, _] = self.corners.map(super::project_cube);
        (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) < 0.0
    }
}

/// Why a block item's geometry or texels cannot be drawn.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GuiBlockReject {
    Geometry,
    Material,
    Texture,
}

/// Inventory geometry, independent of the placed block's connection shape.
pub fn block_gui_quads(
    kind: VisualKind,
    faces: [u32; 6],
    template: Option<u32>,
    templates: &[ModelTemplate],
    quads: &[ModelQuad],
) -> Result<Vec<GuiBlockQuad>, GuiBlockReject> {
    if kind == VisualKind::Model
        && template
            .and_then(|id| templates.get(id as usize))
            .is_some_and(|template| template.flags & crate::MODEL_TEMPLATE_FLAG_PANE != 0)
    {
        let corners = carried_cuboid_faces([112, 0, 0], [144, 256, 256]);
        return Ok(BlockFace::ALL
            .into_iter()
            .zip(corners)
            .map(|(face, corners)| {
                let uvs = corners.map(|[x, y, z]| match face {
                    BlockFace::Up | BlockFace::Down => [x, z],
                    BlockFace::North | BlockFace::South => [x, 1.0 - y],
                    BlockFace::West | BlockFace::East => [z, 1.0 - y],
                });
                GuiBlockQuad {
                    corners: corners.map(|[x, y, z]| [x - 0.2, y, z]),
                    uvs,
                    material: faces[match face {
                        BlockFace::West | BlockFace::East => face,
                        _ => BlockFace::Up,
                    } as usize],
                    two_sided: false,
                }
            })
            .collect());
    }
    let flags = template
        .and_then(|id| templates.get(id as usize))
        .map_or(0, |part| part.flags);
    if kind == VisualKind::Model && flags & crate::MODEL_TEMPLATE_FLAG_AZALEA != 0 {
        return Ok(plants::azalea(faces));
    }
    if kind == VisualKind::Model && flags & crate::MODEL_TEMPLATE_FLAG_CHORUS != 0 {
        let boxes = [
            ([0.0; 3], [1.0; 3]),
            ([0.125, 1.0, 0.125], [0.875, 1.125, 0.875]),
            ([-0.125, 0.125, 0.125], [0.0, 0.875, 0.875]),
            ([0.125, 0.125, 1.0], [0.875, 0.875, 1.125]),
        ];
        return Ok(boxes
            .into_iter()
            .flat_map(|(min, max)| inventory_cuboid_quads(min, max, faces))
            .collect());
    }
    let mut carried = block_item_quads(kind, faces, template, templates, quads)?;
    if kind == VisualKind::Model && flags & crate::MODEL_TEMPLATE_FLAG_HEAVY_CORE != 0 {
        let (scale, offset) = HEAVY_CORE_GUI_TRANSFORM;
        transform_inventory_quads(&mut carried, scale, offset);
    }
    if kind == VisualKind::Model && flags & crate::MODEL_TEMPLATE_FLAG_SHELF != 0 {
        let (scale, offset) = SHELF_GUI_TRANSFORM;
        transform_inventory_quads(&mut carried, scale, offset);
    }
    Ok(carried)
}

/// Applies a GUI scale and screen offset in model space while retaining authored UVs.
fn transform_inventory_quads(quads: &mut [GuiBlockQuad], scale: f32, offset: [f32; 2]) {
    let [sx, cx, sy, cy] = super::CUBE_ROTATION_SIN_COS;
    let [x, y] = offset.map(|value| value / super::CUBE_SCALE);
    let rotated = [x, cx * y, -sx * y];
    let translation = [
        cy * rotated[0] - sy * rotated[2],
        rotated[1],
        sy * rotated[0] + cy * rotated[2],
    ];
    for quad in quads {
        for point in &mut quad.corners {
            for axis in 0..3 {
                point[axis] = point[axis] * scale + translation[axis];
            }
        }
    }
}

/// Outward cuboid faces with position-derived UVs; overhanging spans start at the tile edge.
fn inventory_cuboid_quads(min: [f32; 3], max: [f32; 3], materials: [u32; 6]) -> Vec<GuiBlockQuad> {
    BlockFace::ALL
        .into_iter()
        .map(|face| {
            let (unit, _) = cube_face(face);
            let corners = unit.map(|point| {
                std::array::from_fn(|axis| min[axis] + point[axis] * (max[axis] - min[axis]))
            });
            let mut uvs = corners.map(|[x, y, z]| match face {
                BlockFace::Up | BlockFace::Down => [x, z],
                BlockFace::North | BlockFace::South => [x, 1.0 - y],
                BlockFace::West | BlockFace::East => [z, 1.0 - y],
            });
            for axis in 0..2 {
                if uvs.iter().any(|uv| !(0.0..=1.0).contains(&uv[axis])) {
                    let start = uvs.iter().map(|uv| uv[axis]).fold(f32::INFINITY, f32::min);
                    for uv in &mut uvs {
                        uv[axis] -= start;
                    }
                }
            }
            GuiBlockQuad {
                corners,
                uvs,
                material: materials[face as usize],
                two_sided: false,
            }
        })
        .collect()
}

/// The quads a block item draws: a cube's camera-facing GUI surfaces, a fence's inventory
/// shape, or its template's quads.
pub fn block_item_quads(
    kind: VisualKind,
    faces: [u32; 6],
    template: Option<u32>,
    templates: &[ModelTemplate],
    quads: &[ModelQuad],
) -> Result<Vec<GuiBlockQuad>, GuiBlockReject> {
    let mut out = Vec::new();
    match (kind, template) {
        (VisualKind::Cube, _) => {
            for (face, corners, uvs, _) in CUBE_FACES {
                out.push(GuiBlockQuad {
                    corners,
                    uvs,
                    material: faces[face as usize],
                    two_sided: false,
                });
            }
        }
        (VisualKind::Model, Some(template)) => {
            let first = templates
                .get(template as usize)
                .ok_or(GuiBlockReject::Geometry)?;
            if first.flags & (MODEL_TEMPLATE_FLAG_FENCE_WOOD | MODEL_TEMPLATE_FLAG_FENCE_NETHER)
                != 0
            {
                out.extend(inventory_fence_quads(faces[BlockFace::South as usize]));
            } else if first.flags & MODEL_TEMPLATE_FLAG_WALL != 0 {
                out.extend(carried_wall_quads(faces));
            } else {
                let parts = crate::model_template_parts(templates, template)
                    .ok_or(GuiBlockReject::Geometry)?;
                let gate = first.flags
                    & (MODEL_TEMPLATE_FLAG_GATE_AXIS_X | MODEL_TEMPLATE_FLAG_GATE_AXIS_Z)
                    != 0;
                for part in parts {
                    let start = part.quad_start as usize;
                    let part_quads = quads
                        .get(start..start + part.quad_count as usize)
                        .ok_or(GuiBlockReject::Geometry)?;
                    out.extend(part_quads.iter().map(|quad| {
                        GuiBlockQuad {
                            corners: quad.positions.map(|[x, y, z]| {
                                let point = if gate { [256 - z, y, x] } else { [x, y, z] };
                                point.map(|component| f32::from(component) / 256.0)
                            }),
                            uvs: quad
                                .uvs
                                .map(|uv| uv.map(|component| f32::from(component) / 4096.0)),
                            material: quad.material,
                            two_sided: quad.flags & crate::MODEL_QUAD_FLAG_TWO_SIDED != 0,
                        }
                    }));
                }
            }
        }
        _ => return Err(GuiBlockReject::Geometry),
    }
    if out.is_empty() {
        return Err(GuiBlockReject::Geometry);
    }
    Ok(out)
}

/// Bedrock's inventory fence in block pixels: two end posts joined by two rails that overhang
/// the block by two pixels. Rails split at the block edges so each piece's UVs wrap inside one
/// texture tile; the splits are internal and do not change the silhouette.
const INVENTORY_FENCE_BOXES: [([i8; 3], [i8; 3]); 8] = [
    ([6, 0, 0], [10, 16, 4]),
    ([6, 0, 12], [10, 16, 16]),
    ([7, 13, -2], [9, 15, 0]),
    ([7, 13, 0], [9, 15, 16]),
    ([7, 13, 16], [9, 15, 18]),
    ([7, 5, -2], [9, 7, 0]),
    ([7, 5, 0], [9, 7, 16]),
    ([7, 5, 16], [9, 7, 18]),
];

/// Every outward face of the inventory fence, with native position-derived UVs. Held items
/// draw these one-sided, so faces hidden from the inventory camera are kept.
fn inventory_fence_quads(material: u32) -> impl Iterator<Item = GuiBlockQuad> {
    INVENTORY_FENCE_BOXES
        .into_iter()
        .flat_map(move |(min, max)| {
            BlockFace::ALL.map(|face| {
                let (unit, _) = cube_face(face);
                let corners = unit.map(|corner| {
                    std::array::from_fn(|axis| {
                        let [min, max] = [min[axis], max[axis]].map(f32::from);
                        (min + corner[axis] * (max - min)) / 16.0
                    })
                });
                let mut uvs = corners.map(|[x, y, z]| match face {
                    BlockFace::Up | BlockFace::Down => [x, z],
                    BlockFace::North | BlockFace::South => [x, 1.0 - y],
                    BlockFace::West | BlockFace::East => [z, 1.0 - y],
                });
                for axis in 0..2 {
                    let tile = uvs.iter().map(|uv| uv[axis]).fold(f32::INFINITY, f32::min);
                    for uv in &mut uvs {
                        uv[axis] -= tile.floor();
                    }
                }
                GuiBlockQuad {
                    corners,
                    uvs,
                    material,
                    two_sided: false,
                }
            })
        })
}

/// Two end pillars and a lower span, matching the authored `geometry.wall_render` item.
fn carried_wall_quads(materials: [u32; 6]) -> Vec<GuiBlockQuad> {
    let post_uvs = [
        [0, 0, 8, 16],
        [0, 0, 8, 16],
        [8, 8, 0, 0],
        [8, 8, 0, 0],
        [0, 0, 8, 16],
        [0, 0, 8, 16],
    ];
    let elements = [
        ([64, 0, 192], [192, 256, 320], post_uvs),
        ([64, 0, -64], [192, 256, 64], post_uvs),
        (
            [80, 0, 16],
            [176, 208, 240],
            [
                [0, 0, 4, 13],
                [0, 0, 4, 13],
                [6, 4, 0, 0],
                [6, 4, 0, 0],
                [0, 0, 6, 13],
                [0, 0, 6, 13],
            ],
        ),
    ];
    let mut out = Vec::with_capacity(elements.len() * 6);
    for (min, max, uvs) in elements {
        for (face, (corners, uv)) in carried_cuboid_faces(min, max)
            .into_iter()
            .zip(uvs)
            .enumerate()
        {
            out.push(GuiBlockQuad {
                // The item's root turns the wall across X, independently of world connections.
                corners: corners.map(|[x, y, z]| [z, y, 1.0 - x]),
                uvs: carried_face_uvs(face, uv),
                material: materials[match BlockFace::ALL[face] {
                    BlockFace::West => BlockFace::South,
                    BlockFace::East => BlockFace::North,
                    BlockFace::North => BlockFace::West,
                    BlockFace::South => BlockFace::East,
                    vertical => vertical,
                } as usize],
                two_sided: false,
            });
        }
    }
    out
}

/// Outward-facing cuboid corners from fixed-point block coordinates.
fn carried_cuboid_faces(
    [min_x, min_y, min_z]: [i16; 3],
    [max_x, max_y, max_z]: [i16; 3],
) -> [[[f32; 3]; 4]; 6] {
    let convert = |[x, y, z]: [i16; 3]| {
        [
            f32::from(x) / 256.0,
            f32::from(y) / 256.0,
            f32::from(z) / 256.0,
        ]
    };
    [
        [
            [min_x, min_y, min_z],
            [min_x, min_y, max_z],
            [min_x, max_y, max_z],
            [min_x, max_y, min_z],
        ],
        [
            [max_x, min_y, min_z],
            [max_x, max_y, min_z],
            [max_x, max_y, max_z],
            [max_x, min_y, max_z],
        ],
        [
            [min_x, min_y, min_z],
            [max_x, min_y, min_z],
            [max_x, min_y, max_z],
            [min_x, min_y, max_z],
        ],
        [
            [min_x, max_y, min_z],
            [min_x, max_y, max_z],
            [max_x, max_y, max_z],
            [max_x, max_y, min_z],
        ],
        [
            [min_x, min_y, min_z],
            [min_x, max_y, min_z],
            [max_x, max_y, min_z],
            [max_x, min_y, min_z],
        ],
        [
            [min_x, min_y, max_z],
            [max_x, min_y, max_z],
            [max_x, max_y, max_z],
            [min_x, max_y, max_z],
        ],
    ]
    .map(|face| face.map(convert))
}

/// Maps a pixel UV rectangle to the corresponding outward-facing cuboid corners.
fn carried_face_uvs(face: usize, [u0, v0, u1, v1]: [u16; 4]) -> [[f32; 2]; 4] {
    let [u0, v0, u1, v1] = [u0, v0, u1, v1].map(|value| f32::from(value) / 16.0);
    match face {
        0 => [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
        1 => [[u1, v1], [u1, v0], [u0, v0], [u0, v1]],
        2 => [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
        3 => [[u0, v0], [u0, v1], [u1, v1], [u1, v0]],
        4 => [[u1, v1], [u1, v0], [u0, v0], [u0, v1]],
        _ => [[u0, v1], [u1, v1], [u1, v0], [u0, v0]],
    }
}

/// Reads a material's first-mip tile, side and alpha mode from its texture page.
/// World isotropy and disabled ambient occlusion preserve isolated tiles; tints and rotated UVs do not.
pub fn material_tile<'a>(
    materials: &[Material],
    id: u32,
    array: impl FnOnce(u32) -> Option<&'a TextureArray>,
) -> Result<(&'a [u8], usize, bool), GuiBlockReject> {
    if id == crate::DIAGNOSTIC_MATERIAL {
        return Err(GuiBlockReject::Material);
    }
    let material = materials.get(id as usize).ok_or(GuiBlockReject::Material)?;
    if material.flags & !GUI_BLOCK_MATERIAL_FLAGS != 0 {
        return Err(GuiBlockReject::Material);
    }
    let array = array(material.texture.page()).ok_or(GuiBlockReject::Texture)?;
    let mip = array.mips.first().ok_or(GuiBlockReject::Texture)?;
    let side = mip.size as usize;
    if side == 0 || side > MAX_GUI_TILE_SIDE || material.texture.layer() >= array.layers {
        return Err(GuiBlockReject::Texture);
    }
    let bytes = side * side * 4;
    let start = material.texture.layer() as usize * bytes;
    let tile = mip
        .rgba8
        .get(start..start + bytes)
        .ok_or(GuiBlockReject::Texture)?;
    Ok((tile, side, material.flags & MATERIAL_FLAG_ALPHA_BLEND != 0))
}

/// A full-block face with the cube thumbnail's UV orientation.
#[must_use]
pub fn cube_face(face: BlockFace) -> ([[f32; 3]; 4], [[f32; 2]; 4]) {
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
#[must_use]
pub fn face_brightness(corners: [[f32; 3]; 4]) -> f32 {
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

#[cfg(test)]
mod tests;
