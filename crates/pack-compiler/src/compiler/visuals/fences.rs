use super::context::{
    ModelStorage, RuleInputs, diagnostic_visual, push_model_template, set_model_visual,
};
use super::dispatcher::CompileRuleResult;
use super::geometry::{UvRect, is_horizontal, stretch_uvs};
use {super::super::*, assets::BlockFace};

pub(in crate::compiler) fn compile_rule(
    record: &RegistryRecord,
    inputs: &RuleInputs<'_>,
    templates: &mut BTreeMap<[u32; 3], u32>,
    storage: &mut ModelStorage<'_>,
) -> Result<CompileRuleResult, AssetError> {
    if !is_fence(record) {
        return Ok(CompileRuleResult::NoMatch);
    }
    let mut visual = diagnostic_visual(record);
    if let Some(material) = inputs.material(record, BlockFace::South) {
        let kind = assets::facts::block_render_fact(&record.name).kind;
        let flag = if kind == assets::facts::BlockRenderKind::NetherBrickFence {
            MODEL_TEMPLATE_FLAG_FENCE_NETHER
        } else {
            MODEL_TEMPLATE_FLAG_FENCE_WOOD
        };
        let bamboo = kind == assets::facts::BlockRenderKind::BambooFence;
        let key = [material, flag, u32::from(bamboo)];
        let base = if let Some(&base) = templates.get(&key) {
            base
        } else {
            let base = u32::try_from(storage.templates.len()).map_err(|_| {
                AssetError::BlobSizeOverflow {
                    section: "model template",
                }
            })?;
            let post = if bamboo {
                bamboo_post_quads(material)
            } else {
                cuboid_quads([material; 6], [96, 0, 96], [160, 256, 160])
            };
            push_model_template(post.to_vec(), flag, storage.templates, storage.quads)?;
            for mask in 0..16 {
                let arms = if bamboo {
                    bamboo_fence_arm_quads(material, mask)
                } else {
                    fence_arm_quads(material, mask)
                };
                push_model_template(arms, flag, storage.templates, storage.quads)?;
            }
            templates.insert(key, base);
            base
        };
        set_model_visual(&mut visual, [material; 6], base);
    }
    Ok(CompileRuleResult::Compiled(visual))
}

pub(in crate::compiler) fn fence_arm_quads(material: u32, mask: u32) -> Vec<ModelQuad> {
    connected_arm_quads(material, mask, &ARMS, |_, quad| quad)
}

/// A rail's horizontal bounds and connection bit, extended along X or Z.
struct Arm {
    bit: u32,
    min: [i16; 3],
    max: [i16; 3],
}

impl Arm {
    const fn along_x(&self) -> bool {
        self.bit == 2 || self.bit == 8
    }
}

/// Ordinary rails run from the post centre to the block edge.
const ARMS: [Arm; 4] = [
    Arm {
        bit: 1,
        min: [112, 0, 0],
        max: [144, 0, 128],
    },
    Arm {
        bit: 2,
        min: [128, 0, 112],
        max: [256, 0, 144],
    },
    Arm {
        bit: 4,
        min: [112, 0, 128],
        max: [144, 0, 256],
    },
    Arm {
        bit: 8,
        min: [0, 0, 112],
        max: [128, 0, 144],
    },
];

/// Bamboo slats are nine pixels long, reaching one pixel past the post centre as native
/// `tessellateFenceInWorld` slat shapes do.
const BAMBOO_ARMS: [Arm; 4] = [
    Arm {
        bit: 1,
        min: [112, 0, 0],
        max: [144, 0, 144],
    },
    Arm {
        bit: 2,
        min: [112, 0, 112],
        max: [256, 0, 144],
    },
    Arm {
        bit: 4,
        min: [112, 0, 112],
        max: [144, 0, 256],
    },
    Arm {
        bit: 8,
        min: [0, 0, 112],
        max: [144, 0, 144],
    },
];

/// Emits both rails of every connected arm without the caps along its extension axis;
/// `shade` adjusts each kept face, given its arm.
fn connected_arm_quads(
    material: u32,
    mask: u32,
    arms: &[Arm; 4],
    shade: impl Fn(&Arm, ModelQuad) -> ModelQuad,
) -> Vec<ModelQuad> {
    debug_assert!(mask <= 15);
    let mut quads = Vec::with_capacity(mask.count_ones() as usize * 8);
    for arm in arms.iter().filter(|arm| mask & arm.bit != 0) {
        let (mut min, mut max) = (arm.min, arm.max);
        for (min_y, max_y) in [(96, 144), (192, 240)] {
            min[1] = min_y;
            max[1] = max_y;
            for (face, quad) in cuboid_quads([material; 6], min, max)
                .into_iter()
                .enumerate()
            {
                let is_extension_cap = if arm.along_x() {
                    matches!(face, 0 | 1)
                } else {
                    matches!(face, 4 | 5)
                };
                if !is_extension_cap {
                    quads.push(shade(arm, quad));
                }
            }
        }
    }
    quads
}

/// Native `_setBambooFencePostTexture` regions: the top reads u 0.25–0.499 and v 0–0.25,
/// every side u 0–0.249 over the full height. The bottom flips the top vertically, which
/// lands on the same orientation as the top in this carrier's down-face convention.
const BAMBOO_POST_TOP: UvRect = [1024, 2044, 0, 1024];
const BAMBOO_POST_SIDE: UvRect = [0, 1020, 0, 4096];
/// Native `_tessellateBambooFenceSlatsInWorld` regions: tops u 0.8125–0.9365 and v
/// 0.4375–1, sides u 0.25–0.8115 and v 0.25–0.4375.
const BAMBOO_SLAT_TOP: UvRect = [3328, 3836, 1792, 4096];
const BAMBOO_SLAT_SIDE: UvRect = [1024, 3324, 1024, 1792];

/// The bamboo post: the fence post shape with each face stretched over its native region.
fn bamboo_post_quads(material: u32) -> [ModelQuad; 6] {
    cuboid_quads([material; 6], [96, 0, 96], [160, 256, 160]).map(|quad| {
        let rect = if is_horizontal(&quad) {
            BAMBOO_POST_TOP
        } else {
            BAMBOO_POST_SIDE
        };
        stretch_uvs(quad, rect, false)
    })
}

/// Bamboo slats stretch fixed texture regions over each face. East and west slat tops
/// turn a quarter so the region's long axis follows the slat, as native rotation 1/2 does.
fn bamboo_fence_arm_quads(material: u32, mask: u32) -> Vec<ModelQuad> {
    connected_arm_quads(material, mask, &BAMBOO_ARMS, |arm, quad| {
        if is_horizontal(&quad) {
            stretch_uvs(quad, BAMBOO_SLAT_TOP, arm.along_x())
        } else {
            stretch_uvs(quad, BAMBOO_SLAT_SIDE, false)
        }
    })
}

#[cfg(test)]
#[path = "fences_tests.rs"]
mod tests;
