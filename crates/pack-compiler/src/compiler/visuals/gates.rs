use super::context::{
    GateTemplateKey, ModelStorage, RuleInputs, diagnostic_visual, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::{UvRect, stretch_uvs};
use {super::super::*, assets::BlockFace};

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<GateTemplateKey, u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_gate(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    const IN_WALL: u32 = 1 << 6;
    const GATE_STATE_MASK: u8 = 0x85;
    let mut visual = diagnostic_visual(record);
    if record.model_state.mask() == GATE_STATE_MASK
        && let Some(materials) = inputs.materials(record)
        && let (Some(orientation @ 0..=3), Some(open @ 0..=1), Some(flags @ (0 | IN_WALL))) = (
            record.model_state.get(ModelStateField::Orientation),
            record.model_state.get(ModelStateField::Open),
            record.model_state.get(ModelStateField::Flags),
        )
    {
        let key = GateTemplateKey {
            materials,
            orientation: orientation as u8,
            open: open != 0,
            in_wall: flags != 0,
            bamboo: assets::facts::block_render_fact(&record.name).kind
                == assets::facts::BlockRenderKind::BambooGate,
        };
        let template = if let Some(&template) = templates.get(&key) {
            template
        } else {
            let [head, tail] =
                gate_quads(materials, orientation, key.open, key.in_wall, key.bamboo);
            let template = u32::try_from(storage.templates.len()).map_err(|_| {
                AssetError::BlobSizeOverflow {
                    section: "model template",
                }
            })?;
            let gate_axis = if orientation & 1 == 0 {
                MODEL_TEMPLATE_FLAG_GATE_AXIS_Z
            } else {
                MODEL_TEMPLATE_FLAG_GATE_AXIS_X
            };
            for (part, flags) in [
                (head, MODEL_TEMPLATE_FLAG_COMPOUND_NEXT | gate_axis),
                (tail, 0),
            ] {
                let quad_start = u32::try_from(storage.quads.len()).map_err(|_| {
                    AssetError::BlobSizeOverflow {
                        section: "model quad",
                    }
                })?;
                let quad_count =
                    u32::try_from(part.len()).map_err(|_| AssetError::BlobSizeOverflow {
                        section: "model quad count",
                    })?;
                debug_assert!(quad_count <= 32);
                storage.templates.push(ModelTemplate {
                    quad_start,
                    quad_count,
                    flags,
                });
                storage.quads.extend(part);
            }
            templates.insert(key, template);
            template
        };
        set_model_visual(&mut visual, materials, template);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

#[derive(Clone, Copy)]
pub(in crate::compiler) struct GateFaceUv {
    rect: [u16; 4],
}

#[derive(Clone, Copy)]
pub(in crate::compiler) struct GateElement {
    min: [i16; 3],
    max: [i16; 3],
    faces: [Option<GateFaceUv>; 6],
}

pub(in crate::compiler) const fn gate_uv(u1: u16, v1: u16, u2: u16, v2: u16) -> Option<GateFaceUv> {
    Some(GateFaceUv {
        rect: [u1, v1, u2, v2],
    })
}

const GATE_CLOSED: [GateElement; 8] = [
    GateElement {
        min: [0, 80, 112],
        max: [32, 256, 144],
        faces: [
            gate_uv(7, 0, 9, 11),
            gate_uv(7, 0, 9, 11),
            gate_uv(0, 7, 2, 9),
            gate_uv(0, 7, 2, 9),
            gate_uv(0, 0, 2, 11),
            gate_uv(0, 0, 2, 11),
        ],
    },
    GateElement {
        min: [224, 80, 112],
        max: [256, 256, 144],
        faces: [
            gate_uv(7, 0, 9, 11),
            gate_uv(7, 0, 9, 11),
            gate_uv(14, 7, 16, 9),
            gate_uv(14, 7, 16, 9),
            gate_uv(14, 0, 16, 11),
            gate_uv(14, 0, 16, 11),
        ],
    },
    GateElement {
        min: [96, 96, 112],
        max: [128, 240, 144],
        faces: [
            gate_uv(7, 1, 9, 10),
            gate_uv(7, 1, 9, 10),
            gate_uv(6, 7, 8, 9),
            gate_uv(6, 7, 8, 9),
            gate_uv(6, 1, 8, 10),
            gate_uv(6, 1, 8, 10),
        ],
    },
    GateElement {
        min: [128, 96, 112],
        max: [160, 240, 144],
        faces: [
            gate_uv(7, 1, 9, 10),
            gate_uv(7, 1, 9, 10),
            gate_uv(8, 7, 10, 9),
            gate_uv(8, 7, 10, 9),
            gate_uv(8, 1, 10, 10),
            gate_uv(8, 1, 10, 10),
        ],
    },
    GateElement {
        min: [32, 96, 112],
        max: [96, 144, 144],
        faces: [
            None,
            None,
            gate_uv(2, 7, 6, 9),
            gate_uv(2, 7, 6, 9),
            gate_uv(2, 7, 6, 10),
            gate_uv(2, 7, 6, 10),
        ],
    },
    GateElement {
        min: [32, 192, 112],
        max: [96, 240, 144],
        faces: [
            None,
            None,
            gate_uv(2, 7, 6, 9),
            gate_uv(2, 7, 6, 9),
            gate_uv(2, 1, 6, 4),
            gate_uv(2, 1, 6, 4),
        ],
    },
    GateElement {
        min: [160, 96, 112],
        max: [224, 144, 144],
        faces: [
            None,
            None,
            gate_uv(10, 7, 14, 9),
            gate_uv(10, 7, 14, 9),
            gate_uv(10, 7, 14, 10),
            gate_uv(10, 7, 14, 10),
        ],
    },
    GateElement {
        min: [160, 192, 112],
        max: [224, 240, 144],
        faces: [
            None,
            None,
            gate_uv(10, 7, 14, 9),
            gate_uv(10, 7, 14, 9),
            gate_uv(10, 1, 14, 4),
            gate_uv(10, 1, 14, 4),
        ],
    },
];

const GATE_OPEN: [GateElement; 8] = [
    GATE_CLOSED[0],
    GATE_CLOSED[1],
    GateElement {
        min: [0, 96, 208],
        max: [32, 240, 240],
        faces: [
            gate_uv(13, 1, 15, 10),
            gate_uv(13, 1, 15, 10),
            gate_uv(0, 13, 2, 15),
            gate_uv(0, 13, 2, 15),
            gate_uv(0, 1, 2, 10),
            gate_uv(0, 1, 2, 10),
        ],
    },
    GateElement {
        min: [224, 96, 208],
        max: [256, 240, 240],
        faces: [
            gate_uv(13, 1, 15, 10),
            gate_uv(13, 1, 15, 10),
            gate_uv(14, 13, 16, 15),
            gate_uv(14, 13, 16, 15),
            gate_uv(14, 1, 16, 10),
            gate_uv(14, 1, 16, 10),
        ],
    },
    GateElement {
        min: [0, 96, 144],
        max: [32, 144, 208],
        faces: [
            gate_uv(13, 7, 15, 10),
            gate_uv(13, 7, 15, 10),
            gate_uv(0, 9, 2, 13),
            gate_uv(0, 9, 2, 13),
            None,
            None,
        ],
    },
    GateElement {
        min: [0, 192, 144],
        max: [32, 240, 208],
        faces: [
            gate_uv(13, 1, 15, 4),
            gate_uv(13, 1, 15, 4),
            gate_uv(0, 9, 2, 13),
            gate_uv(0, 9, 2, 13),
            None,
            None,
        ],
    },
    GateElement {
        min: [224, 96, 144],
        max: [256, 144, 208],
        faces: [
            gate_uv(13, 7, 15, 10),
            gate_uv(13, 7, 15, 10),
            gate_uv(14, 9, 16, 13),
            gate_uv(14, 9, 16, 13),
            None,
            None,
        ],
    },
    GateElement {
        min: [224, 192, 144],
        max: [256, 240, 208],
        faces: [
            gate_uv(13, 1, 15, 4),
            gate_uv(13, 1, 15, 4),
            gate_uv(14, 9, 16, 13),
            gate_uv(14, 9, 16, 13),
            None,
            None,
        ],
    },
];

/// Converts a native texture fraction to 1/4096 tile units.
const fn tile_units(fraction: f32) -> u16 {
    (fraction * 4096.0 + 0.5) as u16
}

/// A native bamboo gate texture region from its `u0, u1, v0, v1` fractions.
const fn region(u0: f32, u1: f32, v0: f32, v1: f32) -> UvRect {
    [
        tile_units(u0),
        tile_units(u1),
        tile_units(v0),
        tile_units(v1),
    ]
}

/// The region read upside down, as native bottom faces read their top region.
const fn upside_down([u0, u1, v0, v1]: UvRect) -> UvRect {
    [u0, u1, v1, v0]
}

/// Native `_trySetFenceGateTexture` regions of the bamboo gate texture atlas.
const BAMBOO_POST_TOP: UvRect = region(0.002, 0.123, 0.002, 0.123);
const BAMBOO_POST_SIDE: UvRect = region(0.002, 0.123, 0.127, 0.8105);
const BAMBOO_BAR_TOP: UvRect = region(0.6895, 0.8105, 0.0645, 0.1855);
const BAMBOO_BAR_OUTER: UvRect = region(0.6895, 0.8105, 0.1895, 0.748);
const BAMBOO_BAR_INNER: UvRect = region(0.502, 0.623, 0.1895, 0.748);
const BAMBOO_RAIL_BOTTOM: UvRect = region(0.1895, 0.4355, 0.0645, 0.1855);
const BAMBOO_RAIL_NORTH: UvRect = region(0.1895, 0.4355, 0.1895, 0.373);
const BAMBOO_RAIL_SOUTH: UvRect = region(0.1895, 0.4355, 0.5645, 0.748);

/// Each face's region in `BlockFace::ALL` order (west, east, down, up, north, south).
type GateFaceRegions = [UvRect; 6];

/// A bar's regions with the given west, east, north and south sides.
const fn bamboo_bar(west: UvRect, east: UvRect, north: UvRect, south: UvRect) -> GateFaceRegions {
    [
        west,
        east,
        upside_down(BAMBOO_BAR_TOP),
        BAMBOO_BAR_TOP,
        north,
        south,
    ]
}

const BAMBOO_POST: GateFaceRegions = [
    BAMBOO_POST_SIDE,
    BAMBOO_POST_SIDE,
    upside_down(BAMBOO_POST_TOP),
    BAMBOO_POST_TOP,
    BAMBOO_POST_SIDE,
    BAMBOO_POST_SIDE,
];
const BAMBOO_RAIL: GateFaceRegions = [
    BAMBOO_RAIL_SOUTH,
    BAMBOO_RAIL_NORTH,
    BAMBOO_RAIL_BOTTOM,
    upside_down(BAMBOO_RAIL_BOTTOM),
    BAMBOO_RAIL_NORTH,
    BAMBOO_RAIL_SOUTH,
];
/// Closed bars: the one nearer the low post, then the other.
const BAMBOO_CLOSED_BARS: [GateFaceRegions; 2] = [
    bamboo_bar(
        BAMBOO_BAR_INNER,
        BAMBOO_BAR_OUTER,
        BAMBOO_BAR_OUTER,
        BAMBOO_BAR_INNER,
    ),
    bamboo_bar(
        BAMBOO_BAR_OUTER,
        BAMBOO_BAR_INNER,
        BAMBOO_BAR_INNER,
        BAMBOO_BAR_OUTER,
    ),
];
/// Open bars: the one beside the low post, then the one beside the high post.
const BAMBOO_OPEN_BARS: [GateFaceRegions; 2] = [
    bamboo_bar(
        BAMBOO_BAR_OUTER,
        BAMBOO_BAR_OUTER,
        BAMBOO_BAR_OUTER,
        BAMBOO_BAR_OUTER,
    ),
    bamboo_bar(
        BAMBOO_BAR_INNER,
        BAMBOO_BAR_INNER,
        BAMBOO_BAR_INNER,
        BAMBOO_BAR_INNER,
    ),
];

/// One native bamboo gate piece in block pixels, laid out for gates along X; gates along
/// Z swap X and Z. `turned` rails rotate their top and bottom a quarter turn each way.
struct BambooGatePiece {
    min: [i16; 3],
    max: [i16; 3],
    faces: GateFaceRegions,
    turned: bool,
}

/// Native bamboo gate pieces: posts and bars first, then the four rails. Native geometry
/// depends on the gate axis while closed and on the exact orientation while open, and its
/// texture regions stay in world directions however the gate turns.
fn bamboo_gate_pieces(orientation: u32, open: bool) -> [BambooGatePiece; 8] {
    let along_z = orientation & 1 != 0;
    let piece = |min: [i16; 3], max: [i16; 3], faces, turned| {
        let swap = |[x, y, z]: [i16; 3]| if along_z { [z, y, x] } else { [x, y, z] };
        BambooGatePiece {
            min: swap(min),
            max: swap(max),
            faces,
            turned,
        }
    };
    let rail = |x: [i16; 2], y: i16, z: [i16; 2]| {
        piece(
            [x[0], y, z[0]],
            [x[1], y + 3, z[1]],
            BAMBOO_RAIL,
            open != along_z,
        )
    };
    let posts = [
        piece([0, 5, 7], [2, 16, 9], BAMBOO_POST, false),
        piece([14, 5, 7], [16, 16, 9], BAMBOO_POST, false),
    ];
    let [first, second] = posts;
    if !open {
        let [near, far] = BAMBOO_CLOSED_BARS;
        return [
            first,
            second,
            piece([6, 6, 7], [8, 15, 9], near, false),
            piece([8, 6, 7], [10, 15, 9], far, false),
            rail([2, 6], 6, [7, 9]),
            rail([2, 6], 12, [7, 9]),
            rail([10, 14], 6, [7, 9]),
            rail([10, 14], 12, [7, 9]),
        ];
    }
    let (bar, rails) = if matches!(orientation, 0 | 3) {
        ([13, 15], [9, 13])
    } else {
        ([1, 3], [3, 7])
    };
    let [low, high] = BAMBOO_OPEN_BARS;
    [
        first,
        second,
        piece([0, 6, bar[0]], [2, 15, bar[1]], low, false),
        piece([14, 6, bar[0]], [16, 15, bar[1]], high, false),
        rail([0, 2], 6, rails),
        rail([0, 2], 12, rails),
        rail([14, 16], 6, rails),
        rail([14, 16], 12, rails),
    ]
}

/// Bamboo gates stretch native atlas regions over every face of each piece, as native
/// tessellation draws all six; posts and bars form the head part and rails the tail.
fn bamboo_gate_quads(
    materials: [u32; 6],
    orientation: u32,
    open: bool,
    in_wall: bool,
) -> [Vec<ModelQuad>; 2] {
    let drop = if in_wall { 3 } else { 0 };
    let mut parts = [Vec::new(), Vec::new()];
    for (index, piece) in bamboo_gate_pieces(orientation, open)
        .into_iter()
        .enumerate()
    {
        let scale =
            |point: [i16; 3], drop: i16| [point[0] * 16, (point[1] - drop) * 16, point[2] * 16];
        let quads = super::geometry::vanilla_cuboid_quads(
            materials,
            scale(piece.min, drop),
            scale(piece.max, drop),
        );
        for (face, mut quad) in BlockFace::ALL.into_iter().zip(quads) {
            if piece.turned {
                for uv in &mut quad.uvs {
                    let [u, v] = *uv;
                    *uv = match face {
                        BlockFace::Down => [v, 4096 - u],
                        BlockFace::Up => [4096 - v, u],
                        _ => [u, v],
                    };
                }
            }
            parts[usize::from(index >= 4)].push(stretch_uvs(
                quad,
                piece.faces[face as usize],
                false,
            ));
        }
    }
    parts
}

pub(in crate::compiler) fn gate_quads(
    materials: [u32; 6],
    orientation: u32,
    open: bool,
    in_wall: bool,
    bamboo: bool,
) -> [Vec<ModelQuad>; 2] {
    if bamboo {
        return bamboo_gate_quads(materials, orientation, open, in_wall);
    }
    let elements = if open { &GATE_OPEN } else { &GATE_CLOSED };
    let mut parts = [Vec::new(), Vec::new()];
    for (element_index, element) in elements.iter().enumerate() {
        let mut min = element.min;
        let mut max = element.max;
        if in_wall {
            min[1] -= 48;
            max[1] -= 48;
        }
        let (min, max) = rotate_gate_bounds(min, max, orientation);
        for (source_index, uv) in element.faces.iter().copied().enumerate() {
            let Some(uv) = uv else { continue };
            let target = rotate_gate_face(BlockFace::ALL[source_index], orientation);
            parts[usize::from(element_index >= 4)].push(gate_quad(
                materials[target as usize],
                min,
                max,
                target,
                uv,
            ));
        }
    }
    parts
}

pub(in crate::compiler) fn rotate_gate_face(face: BlockFace, orientation: u32) -> BlockFace {
    const ROTATED: [[BlockFace; 6]; 4] = [
        BlockFace::ALL,
        [
            BlockFace::North,
            BlockFace::South,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::East,
            BlockFace::West,
        ],
        [
            BlockFace::East,
            BlockFace::West,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::South,
            BlockFace::North,
        ],
        [
            BlockFace::South,
            BlockFace::North,
            BlockFace::Down,
            BlockFace::Up,
            BlockFace::West,
            BlockFace::East,
        ],
    ];
    ROTATED[orientation as usize][face as usize]
}

pub(in crate::compiler) fn rotate_gate_bounds(
    [min_x, min_y, min_z]: [i16; 3],
    [max_x, max_y, max_z]: [i16; 3],
    orientation: u32,
) -> ([i16; 3], [i16; 3]) {
    match orientation {
        0 => ([min_x, min_y, min_z], [max_x, max_y, max_z]),
        1 => ([256 - max_z, min_y, min_x], [256 - min_z, max_y, max_x]),
        2 => (
            [256 - max_x, min_y, 256 - max_z],
            [256 - min_x, max_y, 256 - min_z],
        ),
        3 => ([min_z, min_y, 256 - max_x], [max_z, max_y, 256 - min_x]),
        _ => unreachable!("gate selectors are checked before geometry generation"),
    }
}

pub(in crate::compiler) fn gate_quad(
    material: u32,
    min: [i16; 3],
    max: [i16; 3],
    face: BlockFace,
    uv: GateFaceUv,
) -> ModelQuad {
    let mut quad = cuboid_quads([material; 6], min, max)[face as usize];
    let [u1, v1, u2, v2] = uv.rect.map(|coordinate| coordinate * 256);
    quad.uvs = match face {
        BlockFace::West | BlockFace::South => [[u1, v2], [u2, v2], [u2, v1], [u1, v1]],
        BlockFace::East | BlockFace::North => [[u1, v2], [u1, v1], [u2, v1], [u2, v2]],
        BlockFace::Down => [[u1, v1], [u2, v1], [u2, v2], [u1, v2]],
        BlockFace::Up => [[u1, v1], [u1, v2], [u2, v2], [u2, v1]],
    };
    quad.flags = match face {
        BlockFace::West => 3,
        BlockFace::East => 4,
        BlockFace::Down => 1,
        BlockFace::Up => 2,
        BlockFace::North => 5,
        BlockFace::South => 6,
    };
    quad
}

#[cfg(test)]
#[path = "gates_tests.rs"]
mod tests;
