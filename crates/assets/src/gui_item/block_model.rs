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

/// Largest material tile side drawn; session overlays resample to at most 128.
pub const MAX_GUI_TILE_SIDE: usize = 128;

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
    block_item_quads(kind, faces, template, templates, quads)
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
mod tests {
    use super::*;
    use crate::{MATERIAL_FLAG_ISOTROPIC, MATERIAL_FLAG_ROTATE_UV, TextureMip, TextureRef};

    #[test]
    fn chorus_inventory_geometry_has_three_outward_protrusions() {
        let source: Vec<_> = BlockFace::ALL
            .into_iter()
            .map(|face| {
                let (corners, uvs) = cube_face(face);
                ModelQuad {
                    positions: corners.map(|p| p.map(|v| (v * 256.0) as i16)),
                    uvs: uvs.map(|p| p.map(|v| (v * 4096.0) as u16)),
                    material: 1,
                    flags: face.model_quad_face_id(),
                }
            })
            .collect();
        let templates = [ModelTemplate {
            quad_start: 0,
            quad_count: source.len() as u32,
            flags: crate::MODEL_TEMPLATE_FLAG_CHORUS,
        }];
        let inventory =
            block_gui_quads(VisualKind::Model, [1; 6], Some(0), &templates, &source).unwrap();
        let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
        for point in inventory.iter().flat_map(|quad| quad.corners) {
            for axis in 0..3 {
                bounds[0][axis] = bounds[0][axis].min(point[axis]);
                bounds[1][axis] = bounds[1][axis].max(point[axis]);
            }
        }
        assert_eq!(bounds, [[-0.125, 0.0, 0.0], [1.0, 1.125, 1.125]]);
        for (axis, edge) in [(0, -0.125), (1, 1.125), (2, 1.125)] {
            assert!(
                inventory.iter().any(|quad| {
                    quad.visible_in_gui() && quad.corners.iter().all(|p| p[axis] == edge)
                }),
                "missing visible protrusion along axis {axis}"
            );
        }
        assert!(
            inventory
                .iter()
                .flat_map(|quad| quad.uvs)
                .flatten()
                .all(|v| (0.0..=1.0).contains(&v))
        );
        let held =
            block_item_quads(VisualKind::Model, [1; 6], Some(0), &templates, &source).unwrap();
        assert!(
            held.iter()
                .flat_map(|quad| quad.corners)
                .flatten()
                .all(|v| (0.0..=1.0).contains(&v))
        );
    }

    #[test]
    fn inventory_panes_show_a_full_plate_with_body_and_edge_materials() {
        let materials = BlockFace::ALL.map(|face| face as u32 + 1);
        let templates = [ModelTemplate {
            quad_start: 0,
            quad_count: 1,
            flags: crate::MODEL_TEMPLATE_FLAG_PANE,
        }];
        let source = [ModelQuad {
            positions: [
                [112, 0, 112],
                [112, 0, 144],
                [112, 256, 144],
                [112, 256, 112],
            ],
            uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
            material: materials[BlockFace::West as usize],
            flags: 0,
        }];
        let inventory =
            block_gui_quads(VisualKind::Model, materials, Some(0), &templates, &source).unwrap();
        let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
        for point in inventory.iter().flat_map(|quad| quad.corners) {
            for axis in 0..3 {
                bounds[0][axis] = bounds[0][axis].min(point[axis]);
                bounds[1][axis] = bounds[1][axis].max(point[axis]);
            }
        }
        assert_eq!(
            bounds,
            [[7.0 / 16.0 - 0.2, 0.0, 0.0], [9.0 / 16.0 - 0.2, 1.0, 1.0]]
        );
        for face in [
            BlockFace::Up,
            BlockFace::Down,
            BlockFace::North,
            BlockFace::South,
        ] {
            let u = inventory[face as usize].uvs.map(|uv| uv[0]);
            assert_eq!(u.into_iter().fold(f32::INFINITY, f32::min), 7.0 / 16.0);
            assert_eq!(u.into_iter().fold(f32::NEG_INFINITY, f32::max), 9.0 / 16.0);
        }
        let visible: Vec<_> = inventory
            .iter()
            .filter(|quad| quad.visible_in_gui())
            .collect();
        assert_eq!(visible.len(), 3);
        assert_eq!(
            visible
                .iter()
                .filter(|quad| quad.material == materials[BlockFace::Up as usize])
                .count(),
            2
        );
        assert_eq!(
            visible
                .iter()
                .filter(|quad| quad.material == materials[BlockFace::West as usize])
                .count(),
            1
        );
        let held =
            block_item_quads(VisualKind::Model, materials, Some(0), &templates, &source).unwrap();
        assert_eq!(
            held.len(),
            source.len(),
            "held geometry keeps the placed template"
        );
    }

    #[test]
    fn walls_use_two_carried_posts_with_a_lower_connecting_span() {
        let world_post =
            carried_cuboid_faces([64, 0, 64], [192, 256, 192]).map(|corners| ModelQuad {
                positions: corners.map(|point| point.map(|value| (value * 256.0) as i16)),
                uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
                material: 7,
                flags: 0,
            });
        let quads = block_item_quads(
            VisualKind::Model,
            [7; 6],
            Some(0),
            &[ModelTemplate {
                quad_start: 0,
                quad_count: world_post.len() as u32,
                flags: crate::MODEL_TEMPLATE_FLAG_WALL,
            }],
            &world_post,
        )
        .unwrap();
        let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
        for quad in &quads {
            assert_eq!(quad.material, 7);
            assert!(quad.uvs.iter().flatten().all(|uv| (0.0..=1.0).contains(uv)));
            for point in quad.corners {
                for axis in 0..3 {
                    bounds[0][axis] = bounds[0][axis].min(point[axis]);
                    bounds[1][axis] = bounds[1][axis].max(point[axis]);
                }
            }
        }
        assert_eq!(bounds, [[-0.25, 0.0, 0.25], [1.25, 1.0, 0.75]]);
        let top_faces = quads
            .iter()
            .filter(|quad| quad.corners.iter().all(|point| point[1] == 1.0))
            .collect::<Vec<_>>();
        assert_eq!(top_faces.len(), 2);
        assert!(
            top_faces
                .iter()
                .any(|quad| quad.corners.iter().all(|p| p[0] <= 0.25))
        );
        assert!(
            top_faces
                .iter()
                .any(|quad| quad.corners.iter().all(|p| p[0] >= 0.75))
        );
        assert!(quads.iter().any(|quad| {
            quad.corners.iter().all(|p| p[1] == 13.0 / 16.0)
                && quad.corners.iter().any(|p| p[0] < 0.5)
                && quad.corners.iter().any(|p| p[0] > 0.5)
        }));
    }

    #[test]
    fn vanilla_gates_use_the_custom_carried_inventory_axis() {
        let quad = ModelQuad {
            positions: [[0, 0, 64], [256, 0, 64], [256, 256, 192], [0, 256, 192]],
            uvs: [[0, 0], [4096, 0], [4096, 4096], [0, 4096]],
            material: 7,
            flags: 0,
        };
        let quads = block_item_quads(
            VisualKind::Model,
            [7; 6],
            Some(0),
            &[
                ModelTemplate {
                    quad_start: 0,
                    quad_count: 1,
                    flags: crate::MODEL_TEMPLATE_FLAG_COMPOUND_NEXT
                        | crate::MODEL_TEMPLATE_FLAG_GATE_AXIS_Z,
                },
                ModelTemplate {
                    quad_start: 0,
                    quad_count: 1,
                    flags: 0,
                },
            ],
            &[quad],
        )
        .unwrap();
        let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
        for point in quads.iter().flat_map(|quad| quad.corners) {
            for axis in 0..3 {
                bounds[0][axis] = bounds[0][axis].min(point[axis]);
                bounds[1][axis] = bounds[1][axis].max(point[axis]);
            }
        }
        assert_eq!(bounds, [[0.25, 0.0, 0.0], [0.75, 1.0, 1.0]]);
    }

    #[test]
    fn carried_walls_preserve_directional_materials() {
        let materials = BlockFace::ALL.map(|face| face as u32 + 1);
        for quad in carried_wall_quads(materials) {
            let [a, b, c, _] = quad.corners;
            let u = std::array::from_fn::<_, 3, _>(|i| b[i] - a[i]);
            let v = std::array::from_fn::<_, 3, _>(|i| c[i] - a[i]);
            let normal = [
                u[1] * v[2] - u[2] * v[1],
                u[2] * v[0] - u[0] * v[2],
                u[0] * v[1] - u[1] * v[0],
            ];
            let face = if normal[0] < 0.0 {
                BlockFace::West
            } else if normal[0] > 0.0 {
                BlockFace::East
            } else if normal[1] < 0.0 {
                BlockFace::Down
            } else if normal[1] > 0.0 {
                BlockFace::Up
            } else if normal[2] < 0.0 {
                BlockFace::North
            } else {
                BlockFace::South
            };
            assert_eq!(quad.material, materials[face as usize]);
        }
    }

    #[test]
    fn cube_gui_surfaces_face_the_inventory_camera() {
        let faces = BlockFace::ALL.map(|face| face as u32 + 1);
        let quads = block_item_quads(VisualKind::Cube, faces, None, &[], &[]).unwrap();
        for quad in &quads {
            let [a, b, c, _] = quad.corners.map(super::super::project_cube);
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            assert!(
                area < 0.0,
                "rear material {} is visible through the icon",
                quad.material
            );
        }
        assert_eq!(quads.len(), CUBE_FACES.len());
    }

    #[test]
    fn gui_materials_preserve_tiles_and_alpha_modes() {
        let array = TextureArray {
            layers: 1,
            mips: vec![TextureMip {
                size: 1,
                rgba8: vec![20, 40, 60, 128].into(),
            }]
            .into(),
        };
        for alpha in [0, MATERIAL_FLAG_ALPHA_BLEND, MATERIAL_FLAG_ALPHA_CUTOUT] {
            for flags in [
                MATERIAL_FLAG_ISOTROPIC,
                MATERIAL_FLAG_ISOTROPIC | MATERIAL_FLAG_DISABLE_AO,
            ] {
                let materials = [
                    Material::unvaried(),
                    Material {
                        texture: TextureRef::new(0, 0).unwrap(),
                        flags: alpha | flags,
                        ..Material::unvaried()
                    },
                ];
                let (tile, side, blend) = material_tile(&materials, 1, |_| Some(&array))
                    .expect("ambient occlusion does not affect isolated GUI tiles");
                assert_eq!(tile, [20, 40, 60, 128]);
                assert_eq!(side, 1);
                assert_eq!(blend, alpha == MATERIAL_FLAG_ALPHA_BLEND);
                let mut unsupported = materials;
                unsupported[1].flags |= MATERIAL_FLAG_ROTATE_UV;
                assert_eq!(
                    material_tile(&unsupported, 1, |_| Some(&array)),
                    Err(GuiBlockReject::Material)
                );
            }
        }
    }

    #[test]
    fn fences_draw_the_native_two_post_inventory_shape() {
        let template = ModelTemplate {
            quad_start: 0,
            quad_count: 0,
            flags: MODEL_TEMPLATE_FLAG_FENCE_WOOD,
        };
        let quads = block_item_quads(VisualKind::Model, [7; 6], Some(0), &[template], &[]).unwrap();
        assert_eq!(
            quads.len(),
            INVENTORY_FENCE_BOXES.len() * BlockFace::ALL.len()
        );
        let mut bounds = [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]];
        for quad in &quads {
            assert_eq!(quad.material, 7);
            assert!(quad.uvs.iter().flatten().all(|uv| (0.0..=1.0).contains(uv)));
            for point in quad.corners {
                for axis in 0..3 {
                    bounds[0][axis] = bounds[0][axis].min(point[axis]);
                    bounds[1][axis] = bounds[1][axis].max(point[axis]);
                }
            }
        }
        // Posts span the block's Z edges and both rails overhang them by two pixels.
        assert_eq!(bounds, [[0.375, 0.0, -0.125], [0.625, 1.0, 1.125]]);
        let tops = quads
            .iter()
            .filter(|quad| quad.corners.iter().all(|point| point[1] == 1.0))
            .count();
        assert_eq!(tops, 2, "only the two posts reach the top");
        // Held fences draw one-sided, so faces hidden from the inventory camera remain.
        let post_east_faces = quads
            .iter()
            .filter(|quad| quad.corners.iter().all(|point| point[0] == 0.625))
            .count();
        assert_eq!(post_east_faces, 2);
    }

    #[test]
    fn lighting_flags_preserve_material_tiles_without_admitting_unresolved_tint() {
        let array = TextureArray {
            layers: 1,
            mips: vec![crate::TextureMip {
                size: 1,
                rgba8: vec![20, 40, 60, 255].into(),
            }]
            .into(),
        };
        for flags in [
            MATERIAL_FLAG_ALPHA_CUTOUT | crate::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
            MATERIAL_FLAG_DISABLE_AO | crate::MATERIAL_FLAG_DISABLE_FACE_DIMMING,
        ] {
            let materials = [
                Material::unvaried(),
                Material {
                    flags,
                    ..Material::unvaried()
                },
            ];
            let (pixels, side, blend) = material_tile(&materials, 1, |_| Some(&array)).unwrap();
            assert_eq!(pixels, &[20, 40, 60, 255]);
            assert_eq!(side, 1);
            assert!(!blend);
        }
        let materials = [
            Material::unvaried(),
            Material {
                flags: crate::MATERIAL_FLAG_GRASS_TINT,
                ..Material::unvaried()
            },
        ];
        assert_eq!(
            material_tile(&materials, 1, |_| Some(&array)),
            Err(GuiBlockReject::Material)
        );
    }
}
