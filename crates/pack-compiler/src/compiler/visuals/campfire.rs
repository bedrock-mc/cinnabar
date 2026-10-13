//! Campfire and soul campfire logs, ember bed and crossed flames.
//!
//! The boxes, face textures and vertex UVs follow the vanilla 26.30 model.
//! Each facing has its own submission order and UV orientation. Upper logs
//! use lit art on their bottoms and long sides; only the ember bed's top is lit.
//! Extinguished states keep the same UVs and replace every lit texture with bark.

use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use {super::super::*, assets::BlockFace};

/// Texture slots supplied by the campfire block graphics.
#[derive(Clone, Copy)]
enum Texture {
    /// Unlit log art, including bark and end grain.
    L,
    /// Lit log art, replaced by L when extinguished.
    H,
}

use Texture::{H, L};

/// One vanilla box in pixel bounds, with final vertex UVs for each face.
struct BoxModel {
    min: [i16; 3],
    max: [i16; 3],
    faces: [(Texture, [[u16; 2]; 4]); 6],
}

/// Face order used by each box's texture and UV table.
const FACE_ORDER: [BlockFace; 6] = [
    BlockFace::Down,
    BlockFace::Up,
    BlockFace::North,
    BlockFace::South,
    BlockFace::West,
    BlockFace::East,
];

/// Vanilla boxes for north, west, south and east, in submission order.
const BOXES: [[BoxModel; 5]; 4] = [
    // North.
    [
        // Lower A.
        BoxModel {
            min: [1, 0, 0],
            max: [5, 4, 16],
            faces: [
                (L, [[0, 4], [16, 4], [16, 0], [0, 0]]), // down
                (L, [[16, 0], [0, 0], [0, 4], [16, 4]]), // up
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                (L, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                (L, [[0, 0], [16, 0], [16, 4], [0, 4]]), // west
                (H, [[0, 5], [16, 5], [16, 1], [0, 1]]), // east
            ],
        },
        // Upper A.
        BoxModel {
            min: [0, 3, 1],
            max: [16, 7, 5],
            faces: [
                (H, [[0, 8], [16, 8], [16, 4], [0, 4]]), // down
                (L, [[0, 0], [0, 4], [16, 4], [16, 0]]), // up
                (H, [[0, 0], [16, 0], [16, 4], [0, 4]]), // north
                (H, [[0, 0], [0, 4], [16, 4], [16, 0]]), // south
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
            ],
        },
        // Lower B.
        BoxModel {
            min: [11, 0, 0],
            max: [15, 4, 16],
            faces: [
                (L, [[0, 4], [16, 4], [16, 0], [0, 0]]), // down
                (L, [[16, 0], [0, 0], [0, 4], [16, 4]]), // up
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                (L, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                (H, [[0, 1], [16, 1], [16, 5], [0, 5]]), // west
                (L, [[0, 4], [16, 4], [16, 0], [0, 0]]), // east
            ],
        },
        // Upper B.
        BoxModel {
            min: [0, 3, 11],
            max: [16, 7, 15],
            faces: [
                (H, [[0, 8], [16, 8], [16, 4], [0, 4]]), // down
                (L, [[0, 0], [0, 4], [16, 4], [16, 0]]), // up
                (H, [[16, 0], [0, 0], [0, 4], [16, 4]]), // north
                (H, [[16, 0], [16, 4], [0, 4], [0, 0]]), // south
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
            ],
        },
        // Ember bed.
        BoxModel {
            min: [5, 0, 0],
            max: [11, 1, 16],
            faces: [
                (L, [[0, 14], [16, 14], [16, 8], [0, 8]]),     // down
                (H, [[16, 8], [0, 8], [0, 14], [16, 14]]),     // up
                (L, [[6, 15], [0, 15], [0, 16], [6, 16]]),     // north
                (L, [[10, 15], [10, 16], [16, 16], [16, 15]]), // south
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),         // west
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),         // east
            ],
        },
    ],
    // West.
    [
        // Lower A.
        BoxModel {
            min: [0, 0, 1],
            max: [16, 4, 5],
            faces: [
                (L, [[0, 0], [0, 4], [16, 4], [16, 0]]), // down
                (L, [[16, 4], [16, 0], [0, 0], [0, 4]]), // up
                (L, [[0, 0], [16, 0], [16, 4], [0, 4]]), // north
                (H, [[0, 1], [0, 5], [16, 5], [16, 1]]), // south
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
            ],
        },
        // Upper A.
        BoxModel {
            min: [1, 3, 0],
            max: [5, 7, 16],
            faces: [
                (H, [[16, 4], [0, 4], [0, 8], [16, 8]]), // down
                (L, [[16, 0], [0, 0], [0, 4], [16, 4]]), // up
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                (L, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                (H, [[16, 0], [0, 0], [0, 4], [16, 4]]), // west
                (H, [[16, 4], [0, 4], [0, 0], [16, 0]]), // east
            ],
        },
        // Lower B.
        BoxModel {
            min: [0, 0, 11],
            max: [16, 4, 15],
            faces: [
                (L, [[0, 0], [0, 4], [16, 4], [16, 0]]), // down
                (L, [[16, 4], [16, 0], [0, 0], [0, 4]]), // up
                (H, [[0, 1], [16, 1], [16, 5], [0, 5]]), // north
                (L, [[0, 0], [0, 4], [16, 4], [16, 0]]), // south
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
            ],
        },
        // Upper B.
        BoxModel {
            min: [11, 3, 0],
            max: [15, 7, 16],
            faces: [
                (H, [[16, 4], [0, 4], [0, 8], [16, 8]]), // down
                (L, [[16, 0], [0, 0], [0, 4], [16, 4]]), // up
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                (L, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                (H, [[0, 0], [16, 0], [16, 4], [0, 4]]), // west
                (H, [[0, 4], [16, 4], [16, 0], [0, 0]]), // east
            ],
        },
        // Ember bed.
        BoxModel {
            min: [0, 0, 5],
            max: [16, 1, 11],
            faces: [
                (L, [[0, 8], [0, 14], [16, 14], [16, 8]]),     // down
                (H, [[16, 14], [16, 8], [0, 8], [0, 14]]),     // up
                (L, [[0, 4], [0, 8], [4, 8], [4, 4]]),         // north
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),         // south
                (L, [[16, 15], [10, 15], [10, 16], [16, 16]]), // west
                (L, [[0, 16], [6, 16], [6, 15], [0, 15]]),     // east
            ],
        },
    ],
    // South.
    [
        // Lower A.
        BoxModel {
            min: [11, 0, 0],
            max: [15, 4, 16],
            faces: [
                (L, [[16, 0], [0, 0], [0, 4], [16, 4]]), // down
                (L, [[0, 4], [16, 4], [16, 0], [0, 0]]), // up
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                (L, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                (H, [[16, 1], [0, 1], [0, 5], [16, 5]]), // west
                (L, [[16, 4], [0, 4], [0, 0], [16, 0]]), // east
            ],
        },
        // Upper A.
        BoxModel {
            min: [0, 3, 11],
            max: [16, 7, 15],
            faces: [
                (H, [[0, 8], [16, 8], [16, 4], [0, 4]]), // down
                (L, [[16, 4], [16, 0], [0, 0], [0, 4]]), // up
                (H, [[16, 0], [0, 0], [0, 4], [16, 4]]), // north
                (H, [[16, 0], [16, 4], [0, 4], [0, 0]]), // south
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
            ],
        },
        // Lower B.
        BoxModel {
            min: [1, 0, 0],
            max: [5, 4, 16],
            faces: [
                (L, [[16, 0], [0, 0], [0, 4], [16, 4]]), // down
                (L, [[0, 4], [16, 4], [16, 0], [0, 0]]), // up
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                (L, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                (L, [[16, 0], [0, 0], [0, 4], [16, 4]]), // west
                (H, [[16, 5], [0, 5], [0, 1], [16, 1]]), // east
            ],
        },
        // Upper B.
        BoxModel {
            min: [0, 3, 1],
            max: [16, 7, 5],
            faces: [
                (H, [[0, 8], [16, 8], [16, 4], [0, 4]]), // down
                (L, [[16, 4], [16, 0], [0, 0], [0, 4]]), // up
                (H, [[0, 0], [16, 0], [16, 4], [0, 4]]), // north
                (H, [[0, 0], [0, 4], [16, 4], [16, 0]]), // south
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
            ],
        },
        // Ember bed.
        BoxModel {
            min: [5, 0, 0],
            max: [11, 1, 16],
            faces: [
                (L, [[16, 8], [0, 8], [0, 14], [16, 14]]),     // down
                (H, [[0, 14], [16, 14], [16, 8], [0, 8]]),     // up
                (L, [[16, 15], [10, 15], [10, 16], [16, 16]]), // north
                (L, [[0, 15], [0, 16], [6, 16], [6, 15]]),     // south
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),         // west
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),         // east
            ],
        },
    ],
    // East.
    [
        // Lower A.
        BoxModel {
            min: [0, 0, 11],
            max: [16, 4, 15],
            faces: [
                (L, [[16, 4], [16, 0], [0, 0], [0, 4]]), // down
                (L, [[0, 0], [0, 4], [16, 4], [16, 0]]), // up
                (H, [[16, 1], [0, 1], [0, 5], [16, 5]]), // north
                (L, [[16, 0], [16, 4], [0, 4], [0, 0]]), // south
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
            ],
        },
        // Upper A.
        BoxModel {
            min: [11, 3, 0],
            max: [15, 7, 16],
            faces: [
                (H, [[16, 4], [0, 4], [0, 8], [16, 8]]), // down
                (L, [[0, 4], [16, 4], [16, 0], [0, 0]]), // up
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                (L, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                (H, [[0, 0], [16, 0], [16, 4], [0, 4]]), // west
                (H, [[0, 4], [16, 4], [16, 0], [0, 0]]), // east
            ],
        },
        // Lower B.
        BoxModel {
            min: [0, 0, 1],
            max: [16, 4, 5],
            faces: [
                (L, [[16, 4], [16, 0], [0, 0], [0, 4]]), // down
                (L, [[0, 0], [0, 4], [16, 4], [16, 0]]), // up
                (L, [[16, 0], [0, 0], [0, 4], [16, 4]]), // north
                (H, [[16, 1], [16, 5], [0, 5], [0, 1]]), // south
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // west
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),   // east
            ],
        },
        // Upper B.
        BoxModel {
            min: [1, 3, 0],
            max: [5, 7, 16],
            faces: [
                (H, [[16, 4], [0, 4], [0, 8], [16, 8]]), // down
                (L, [[0, 4], [16, 4], [16, 0], [0, 0]]), // up
                (L, [[4, 4], [0, 4], [0, 8], [4, 8]]),   // north
                (L, [[0, 4], [0, 8], [4, 8], [4, 4]]),   // south
                (H, [[16, 0], [0, 0], [0, 4], [16, 4]]), // west
                (H, [[16, 4], [0, 4], [0, 0], [16, 0]]), // east
            ],
        },
        // Ember bed.
        BoxModel {
            min: [0, 0, 5],
            max: [16, 1, 11],
            faces: [
                (L, [[16, 14], [16, 8], [0, 8], [0, 14]]),     // down
                (H, [[0, 8], [0, 14], [16, 14], [16, 8]]),     // up
                (L, [[4, 8], [4, 4], [0, 4], [0, 8]]),         // north
                (L, [[0, 8], [4, 8], [4, 4], [0, 4]]),         // south
                (L, [[6, 15], [0, 15], [0, 16], [6, 16]]),     // west
                (L, [[10, 16], [16, 16], [16, 15], [10, 15]]), // east
            ],
        },
    ],
];

/// Identifies both campfire blocks before their provisional envelope route.
pub(in crate::compiler) fn is_record(record: &RegistryRecord) -> bool {
    matches!(
        record.name.as_ref(),
        "minecraft:campfire" | "minecraft:soul_campfire"
    ) && record.contributor_role == ContributorRole::Primary
        && !record.flags.contains(BlockFlags::AIR)
}

/// Returns the typed extinguished bit and the north/west/south/east table index.
fn pose(record: &RegistryRecord) -> Option<(bool, u8)> {
    let state: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(&record.canonical_state).ok()?;
    if state.len() != 2 {
        return None;
    }
    let extinguished = super::state::exact_tagged_byte(state.get("extinguished")?, 1)? != 0;
    let facing =
        match super::state::exact_tagged_string(state.get("minecraft:cardinal_direction")?)? {
            "north" => 0,
            "west" => 1,
            "south" => 2,
            "east" => 3,
            _ => return None,
        };
    Some((extinguished, facing))
}

/// Compiles every campfire pose with the pack's unlit log, lit log and flame materials.
pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<([u32; 6], bool, u8), u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_record(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let Some((extinguished, facing)) = pose(record) else {
        return Ok(CompileRuleResult::Reject);
    };
    let mut visual = diagnostic_visual(record);
    let Some(materials) = inputs.materials(record) else {
        return Ok(CompileRuleResult::Compiled(visual));
    };
    let key = (materials, extinguished, facing);
    let template = if let Some(&template) = templates.get(&key) {
        template
    } else {
        let template = push_model_template(
            quads(materials, extinguished, facing),
            0,
            storage.templates,
            storage.quads,
        )?;
        templates.insert(key, template);
        template
    };
    set_model_visual(&mut visual, materials, template);
    visual.support = VisualSupport::VanillaFallback;
    Ok(CompileRuleResult::Compiled(visual))
}

/// Emits the facing's five boxes, then two double-sided flame planes while lit.
fn quads(materials: [u32; 6], extinguished: bool, facing: u8) -> Vec<ModelQuad> {
    let unlit = materials[BlockFace::Down as usize];
    let lit = if extinguished {
        unlit
    } else {
        materials[BlockFace::South as usize]
    };
    let mut result = Vec::with_capacity(32);
    for model in &BOXES[usize::from(facing)] {
        let [x0, y0, z0] = model.min.map(|p| p * 16);
        let [x1, y1, z1] = model.max.map(|p| p * 16);
        let positions = [
            [[x0, y0, z1], [x0, y0, z0], [x1, y0, z0], [x1, y0, z1]],
            [[x1, y1, z1], [x1, y1, z0], [x0, y1, z0], [x0, y1, z1]],
            [[x0, y1, z0], [x1, y1, z0], [x1, y0, z0], [x0, y0, z0]],
            [[x0, y1, z1], [x0, y0, z1], [x1, y0, z1], [x1, y1, z1]],
            [[x0, y1, z1], [x0, y1, z0], [x0, y0, z0], [x0, y0, z1]],
            [[x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x1, y1, z1]],
        ];
        for ((positions, (texture, uvs)), face) in
            positions.into_iter().zip(model.faces).zip(FACE_ORDER)
        {
            result.push(ModelQuad {
                positions,
                uvs: uvs.map(|uv| uv.map(|p| p * 256)),
                material: match texture {
                    L => unlit,
                    H => lit,
                },
                flags: face.model_quad_face_id(),
            });
        }
    }
    if !extinguished {
        let flame = materials[BlockFace::Up as usize];
        // Diagonal planes from 1 to 16 pixels tall, inset 0.8 pixels from the corners.
        let (low, high, bottom, top) = (13, 243, 16, 256);
        for (start, end) in [([low, low], [high, high]), ([high, low], [low, high])] {
            result.push(ModelQuad {
                positions: [
                    [start[0], bottom, start[1]],
                    [end[0], bottom, end[1]],
                    [end[0], top, end[1]],
                    [start[0], top, start[1]],
                ],
                uvs: [[0, 4096], [4096, 4096], [4096, 0], [0, 0]],
                material: flame,
                flags: MODEL_QUAD_FLAG_TWO_SIDED,
            });
        }
    }
    result
}
