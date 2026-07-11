use assets::{MODEL_QUAD_FLAG_FACE_MASK, ModelQuad, NetworkIdMode, RuntimeAssets, VisualKind};
use world::{MeshDependencyMask, MeshNeighbourhood, SubChunk};

use crate::{BlockClassifier, Face, PackedQuadLighting};

/// Temporary Phase 2.6 light inputs. Phase 2.7 replaces only these inputs.
pub const PHASE26_BLOCK_LIGHT: u8 = 0;
pub const PHASE26_SKY_LIGHT: u8 = 15;

const FIXED_HALF_BLOCK: i16 = 128;

/// Bakes one face-specific four-vertex lighting sidecar.
#[must_use]
pub fn bake_quad_lighting(
    classifier: &BlockClassifier,
    assets: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    block: [i32; 3],
    face: Face,
    positions: [[i16; 3]; 4],
) -> PackedQuadLighting {
    let (normal, tangent_a, tangent_b) = face_basis(face);
    let samples = positions.map(|position| {
        let sign_a = corner_sign(position[tangent_a]);
        let sign_b = corner_sign(position[tangent_b]);
        let side_a = offset(block, normal, tangent_a, sign_a, None);
        let side_b = offset(block, normal, tangent_b, sign_b, None);
        let corner = offset(block, normal, tangent_a, sign_a, Some((tangent_b, sign_b)));
        let side_a = sample_occludes(classifier, assets, network_id_mode, neighbourhood, side_a);
        let side_b = sample_occludes(classifier, assets, network_id_mode, neighbourhood, side_b);
        let corner = sample_occludes(classifier, assets, network_id_mode, neighbourhood, corner);
        let ao = if side_a && side_b {
            3
        } else {
            u8::from(side_a) + u8::from(side_b) + u8::from(corner)
        };
        pack_sample(PHASE26_BLOCK_LIGHT, PHASE26_SKY_LIGHT, ao)
    });
    PackedQuadLighting::new(samples)
}

/// Bakes exactly one sidecar for every quad in a template's immutable order.
#[must_use]
pub fn bake_template_lighting(
    classifier: &BlockClassifier,
    assets: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    block: [i32; 3],
    template_id: u32,
) -> Option<Vec<PackedQuadLighting>> {
    let template = assets.model_templates().get(template_id as usize)?;
    let start = template.quad_start as usize;
    let end = start.checked_add(template.quad_count as usize)?;
    let quads = assets.model_quads().get(start..end)?;
    Some(
        quads
            .iter()
            .map(|quad| {
                model_quad_face(*quad).map_or_else(default_lighting, |face| {
                    bake_quad_lighting(
                        classifier,
                        assets,
                        network_id_mode,
                        neighbourhood,
                        block,
                        face,
                        quad.positions,
                    )
                })
            })
            .collect(),
    )
}

/// Computes diagonal sampling requirements directly from storage palettes.
/// No 4,096-block temporary array is created.
#[must_use]
pub fn mesh_dependency_mask(
    classifier: &BlockClassifier,
    assets: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    sub_chunk: &SubChunk,
) -> MeshDependencyMask {
    let mut mask = MeshDependencyMask::default();
    for storage in sub_chunk.storages() {
        for &network_value in storage.palette().values() {
            if classifier.is_air(network_value) {
                continue;
            }
            match assets.resolve(network_id_mode, network_value).kind() {
                VisualKind::Cross | VisualKind::Model => mask.diagonal_ao = true,
                VisualKind::Liquid => mask.liquid = true,
                VisualKind::Diagnostic | VisualKind::Cube | VisualKind::Invisible => {}
            }
            if mask.diagonal_ao && mask.liquid {
                return mask;
            }
        }
    }
    mask
}

const fn default_lighting() -> PackedQuadLighting {
    PackedQuadLighting::new([pack_sample(PHASE26_BLOCK_LIGHT, PHASE26_SKY_LIGHT, 0); 4])
}

const fn pack_sample(block: u8, sky: u8, ao: u8) -> u16 {
    debug_assert!(block <= 15 && sky <= 15 && ao <= 3);
    (block as u16) | ((sky as u16) << 4) | ((ao as u16) << 8)
}

fn sample_occludes(
    classifier: &BlockClassifier,
    assets: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    coordinate: [i32; 3],
) -> bool {
    let Some((sub_chunk, local)) = neighbourhood.block_source(coordinate) else {
        return false;
    };
    (0..sub_chunk.storages().len()).any(|layer| {
        sub_chunk
            .runtime_id(layer, local[0], local[1], local[2])
            .is_some_and(|network_value| {
                !classifier.is_air(network_value)
                    && assets
                        .resolve(network_id_mode, network_value)
                        .flags()
                        .contains(assets::BlockFlags::OCCLUDES_FULL_FACE)
            })
    })
}

const fn face_basis(face: Face) -> ([i32; 3], usize, usize) {
    match face {
        Face::NegativeX => ([-1, 0, 0], 1, 2),
        Face::PositiveX => ([1, 0, 0], 1, 2),
        Face::NegativeY => ([0, -1, 0], 0, 2),
        Face::PositiveY => ([0, 1, 0], 0, 2),
        Face::NegativeZ => ([0, 0, -1], 0, 1),
        Face::PositiveZ => ([0, 0, 1], 0, 1),
    }
}

const fn corner_sign(value: i16) -> i32 {
    if value < FIXED_HALF_BLOCK { -1 } else { 1 }
}

fn offset(
    mut block: [i32; 3],
    normal: [i32; 3],
    tangent_axis: usize,
    tangent_sign: i32,
    second_tangent: Option<(usize, i32)>,
) -> [i32; 3] {
    for axis in 0..3 {
        block[axis] += normal[axis];
    }
    block[tangent_axis] += tangent_sign;
    if let Some((axis, sign)) = second_tangent {
        block[axis] += sign;
    }
    block
}

const fn model_quad_face(quad: ModelQuad) -> Option<Face> {
    match quad.flags & MODEL_QUAD_FLAG_FACE_MASK {
        1 => Some(Face::NegativeY),
        2 => Some(Face::PositiveY),
        3 => Some(Face::NegativeX),
        4 => Some(Face::PositiveX),
        5 => Some(Face::NegativeZ),
        6 => Some(Face::PositiveZ),
        _ => None,
    }
}
