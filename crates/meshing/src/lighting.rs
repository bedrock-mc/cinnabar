use std::cell::Cell;

use assets::{MODEL_QUAD_FLAG_FACE_MASK, ModelQuad, NetworkIdMode, RuntimeAssets, VisualKind};
use world::{MeshDependencyMask, MeshNeighbourhood, SubChunk};

use crate::{BlockClassifier, ContributorResolver, Face, PackedQuadLighting};

/// Temporary Phase 2.6 light inputs. Phase 2.7 replaces only these inputs.
pub const PHASE26_BLOCK_LIGHT: u8 = 0;
pub const PHASE26_SKY_LIGHT: u8 = 15;

const FIXED_HALF_BLOCK: i16 = 128;

/// One allocation-free block/sky sample owned by the render meshing boundary.
///
/// Block and sky light remain independent four-bit channels. Direct-sky
/// provenance is deliberately not a render channel: the world light solver
/// resolves it before exposing samples to meshing.
#[repr(transparent)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct MeshLightSample(u8);

impl MeshLightSample {
    pub const FULL_BRIGHT: Self = Self(PHASE26_BLOCK_LIGHT | (PHASE26_SKY_LIGHT << 4));

    #[must_use]
    pub const fn try_new(block: u8, sky: u8) -> Option<Self> {
        if block <= 15 && sky <= 15 {
            Some(Self(block | (sky << 4)))
        } else {
            None
        }
    }

    #[must_use]
    pub const fn block(self) -> u8 {
        self.0 & 15
    }

    #[must_use]
    pub const fn sky(self) -> u8 {
        self.0 >> 4
    }
}

/// Allocation-free source of solved block and sky light for mesh baking.
pub trait MeshLightSampler {
    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample;
}

impl<F> MeshLightSampler for F
where
    F: Fn([i32; 3]) -> MeshLightSample,
{
    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample {
        self(coordinate)
    }
}

/// Compatibility source used until a caller supplies its solved light view.
#[derive(Debug, Clone, Copy, Default)]
pub struct FullBrightLightSampler;

impl MeshLightSampler for FullBrightLightSampler {
    fn sample(&self, _coordinate: [i32; 3]) -> MeshLightSample {
        MeshLightSample::FULL_BRIGHT
    }
}

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
    bake_quad_lighting_with_sampler(
        classifier,
        assets,
        network_id_mode,
        neighbourhood,
        &FullBrightLightSampler,
        block,
        face,
        positions,
    )
}

/// Bakes one face sidecar from solved block/sky light and geometric AO.
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "the sampler augments the established face-bake boundary without bundling unrelated assets"
)]
pub fn bake_quad_lighting_with_sampler<S: MeshLightSampler + ?Sized>(
    classifier: &BlockClassifier,
    assets: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    light_sampler: &S,
    block: [i32; 3],
    face: Face,
    positions: [[i16; 3]; 4],
) -> PackedQuadLighting {
    bake_quad(
        &DirectInputs {
            classifier,
            assets,
            network_id_mode,
            neighbourhood,
            sampler: light_sampler,
        },
        block,
        face,
        positions,
        surface_emits(classifier, assets, network_id_mode, neighbourhood, block),
    )
}

/// Occlusion and light lookups shared by the direct and cached bake paths.
pub(crate) trait LightingInputs {
    /// Native cached solid-render bit, independent of geometric face coverage.
    fn occludes(&self, coordinate: [i32; 3]) -> bool;
    /// Native shade brightness is 0.2 for leaves, emitting blocks and solid
    /// render blocks; only the last group also blocks diagonal light sampling.
    fn shade_darkened(&self, coordinate: [i32; 3]) -> bool {
        self.occludes(coordinate)
    }
    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample;
}

fn surface_emits(
    classifier: &BlockClassifier,
    assets: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    coordinate: [i32; 3],
) -> bool {
    let Some((chunk, local)) = neighbourhood.block_source(coordinate) else {
        return false;
    };
    ContributorResolver::resolve_direct(*classifier, assets, network_id_mode, chunk, local)
        .primary_network_value()
        .is_some_and(|id| {
            assets
                .resolve(network_id_mode, id)
                .light_properties()
                .emission()
                > 0
        })
}

struct DirectInputs<'a, 'n, S: MeshLightSampler + ?Sized> {
    classifier: &'a BlockClassifier,
    assets: &'a RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &'a MeshNeighbourhood<'n>,
    sampler: &'a S,
}

impl<S: MeshLightSampler + ?Sized> LightingInputs for DirectInputs<'_, '_, S> {
    fn occludes(&self, coordinate: [i32; 3]) -> bool {
        sample_solid_render(
            self.classifier,
            self.assets,
            self.network_id_mode,
            self.neighbourhood,
            coordinate,
        )
    }

    fn shade_darkened(&self, coordinate: [i32; 3]) -> bool {
        if self.occludes(coordinate) {
            return true;
        }
        let Some((chunk, local)) = self.neighbourhood.block_source(coordinate) else {
            return false;
        };
        chunk
            .runtime_id(0, local[0], local[1], local[2])
            .filter(|&id| !self.classifier.is_air(id))
            .is_some_and(|id| {
                let visual = self.assets.resolve(self.network_id_mode, id);
                // BlockType::getShadeBrightness:
                // property 0x20 and Block+0x71, independently of Block+0xa3.
                visual.flags().contains(assets::BlockFlags::LEAF_MODEL)
                    || visual.light_properties().emission() > 0
            })
    }

    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample {
        self.sampler.sample(coordinate)
    }
}

const HALO_SIDE: usize = 18;
const HALO_VOLUME: usize = HALO_SIDE * HALO_SIDE * HALO_SIDE;
const HALO_WORDS: usize = HALO_VOLUME.div_ceil(64);

/// Per-mesh memo of light samples and native solid-render bits over the one-block
/// halo, filled lazily so sparse sub-chunks resolve only what their faces touch.
pub(crate) struct MeshLightingCache<'a, 'n, S: MeshLightSampler + ?Sized> {
    direct: DirectInputs<'a, 'n, S>,
    light: [Cell<MeshLightSample>; HALO_VOLUME],
    light_known: [Cell<u64>; HALO_WORDS],
    occluder_known: [Cell<u64>; HALO_WORDS],
    occluders: [Cell<u64>; HALO_WORDS],
    shade_known: [Cell<u64>; HALO_WORDS],
    darkened_shades: [Cell<u64>; HALO_WORDS],
}

impl<'a, 'n, S: MeshLightSampler + ?Sized> MeshLightingCache<'a, 'n, S> {
    pub(crate) fn new(
        classifier: &'a BlockClassifier,
        assets: &'a RuntimeAssets,
        network_id_mode: NetworkIdMode,
        neighbourhood: &'a MeshNeighbourhood<'n>,
        sampler: &'a S,
    ) -> Self {
        Self {
            direct: DirectInputs {
                classifier,
                assets,
                network_id_mode,
                neighbourhood,
                sampler,
            },
            light: [const { Cell::new(MeshLightSample::FULL_BRIGHT) }; HALO_VOLUME],
            light_known: [const { Cell::new(0) }; HALO_WORDS],
            occluder_known: [const { Cell::new(0) }; HALO_WORDS],
            occluders: [const { Cell::new(0) }; HALO_WORDS],
            shade_known: [const { Cell::new(0) }; HALO_WORDS],
            darkened_shades: [const { Cell::new(0) }; HALO_WORDS],
        }
    }
}

impl<S: MeshLightSampler + ?Sized> LightingInputs for MeshLightingCache<'_, '_, S> {
    fn occludes(&self, coordinate: [i32; 3]) -> bool {
        let Some(index) = halo_index(coordinate) else {
            return self.direct.occludes(coordinate);
        };
        let (word, bit) = (index / 64, 1_u64 << (index % 64));
        if self.occluder_known[word].get() & bit == 0 {
            if self.direct.occludes(coordinate) {
                self.occluders[word].set(self.occluders[word].get() | bit);
            }
            self.occluder_known[word].set(self.occluder_known[word].get() | bit);
        }
        self.occluders[word].get() & bit != 0
    }

    fn shade_darkened(&self, coordinate: [i32; 3]) -> bool {
        let Some(index) = halo_index(coordinate) else {
            return self.direct.shade_darkened(coordinate);
        };
        let (word, bit) = (index / 64, 1_u64 << (index % 64));
        if self.shade_known[word].get() & bit == 0 {
            if self.direct.shade_darkened(coordinate) {
                self.darkened_shades[word].set(self.darkened_shades[word].get() | bit);
            }
            self.shade_known[word].set(self.shade_known[word].get() | bit);
        }
        self.darkened_shades[word].get() & bit != 0
    }

    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample {
        let Some(index) = halo_index(coordinate) else {
            return self.direct.sample(coordinate);
        };
        let (word, bit) = (index / 64, 1_u64 << (index % 64));
        if self.light_known[word].get() & bit == 0 {
            self.light[index].set(self.direct.sample(coordinate));
            self.light_known[word].set(self.light_known[word].get() | bit);
        }
        self.light[index].get()
    }
}

fn halo_index(coordinate: [i32; 3]) -> Option<usize> {
    let [x, y, z] = coordinate.map(|value| {
        usize::try_from(value.wrapping_add(1))
            .ok()
            .filter(|&v| v < HALO_SIDE)
    });
    Some((x? * HALO_SIDE + y?) * HALO_SIDE + z?)
}

pub(crate) fn bake_quad<I: LightingInputs + ?Sized>(
    inputs: &I,
    block: [i32; 3],
    face: Face,
    positions: [[i16; 3]; 4],
    emitting_block: bool,
) -> PackedQuadLighting {
    bake_quad_with(inputs, block, face, positions, false, emitting_block)
}

/// Current liquid tessellation never applies terrain AO;
/// sides and the bottom repeat one outward-cell sample across the whole face.
pub(crate) fn bake_liquid_quad<I: LightingInputs + ?Sized>(
    inputs: &I,
    block: [i32; 3],
    face: Face,
    positions: [[i16; 3]; 4],
) -> PackedQuadLighting {
    if face != Face::PositiveY {
        return lighting_at(inputs.sample(add_normal(block, face_basis(face).0)));
    }
    // Incomplete top-light parity: native averages four admitted samples in the
    // y+1 plane, rounding each channel. Admission uses BlockType+0x15c > 0.5,
    // which is not carried by LightingInputs and is not the solid-render bit
    // (ordinary ice is non-solid but retains 0.0 here). Preserve the existing
    // top light/admission until that independent native property is available.
    let top = bake_quad_in_plane(inputs, block, face, positions, false, true, false);
    PackedQuadLighting::new(top.samples().map(|sample| sample & 0x00ff))
}

/// Templates check their bounds; cube faces are known to lie on a block boundary.
pub(crate) fn bake_quad_with<I: LightingInputs + ?Sized>(
    inputs: &I,
    block: [i32; 3],
    face: Face,
    positions: [[i16; 3]; 4],
    check_bounds: bool,
    emitting_block: bool,
) -> PackedQuadLighting {
    bake_quad_in_plane(
        inputs,
        block,
        face,
        positions,
        check_bounds,
        false,
        emitting_block,
    )
}

fn bake_quad_in_plane<I: LightingInputs + ?Sized>(
    inputs: &I,
    block: [i32; 3],
    face: Face,
    positions: [[i16; 3]; 4],
    check_bounds: bool,
    liquid_outward_center: bool,
    emitting_block: bool,
) -> PackedQuadLighting {
    let (normal, tangent_a, tangent_b) = face_basis(face);
    let outward = add_normal(block, normal);
    let axis = normal.iter().position(|&n| n != 0).expect("face axis");
    let boundary = !check_bounds
        || positions.iter().all(|p| {
            let position = f32::from(p[axis]) / 256.0;
            if normal[axis] < 0 {
                position <= 0.0005
            } else {
                position >= 0.9995
            }
        });
    // Vanilla moves only the center light sample when the OWN block's
    // cached solid bit is set. Tangential AO/light samples follow boundary alone.
    let light_origin = if boundary && (liquid_outward_center || inputs.occludes(block)) {
        outward
    } else {
        block
    };
    let plane_normal = if boundary { normal } else { [0; 3] };
    // Native AO/flat tessellation reads the rendered block’s emission
    // from the rendered contributor, not the union of colocated storage layers.
    // Solved light samples still include every contributor's physical emission.
    let emitter = u16::from(emitting_block) << 11;
    let shade_face = u8::from(inputs.shade_darkened(outward));
    let samples = positions.map(|position| {
        let sign_a = corner_sign(position[tangent_a]);
        let sign_b = corner_sign(position[tangent_b]);
        let side_a = offset(block, plane_normal, tangent_a, sign_a, None);
        let side_b = offset(block, plane_normal, tangent_b, sign_b, None);
        let corner = offset(
            block,
            plane_normal,
            tangent_a,
            sign_a,
            Some((tangent_b, sign_b)),
        );
        let solid_a = inputs.occludes(side_a);
        let solid_b = inputs.occludes(side_b);
        let blocked_diagonal = solid_a && solid_b;
        // AmbientOcclusionCalculator::calculateWithCache
        // averages four independent 0.2/1 shade samples. Its diagonal fallback
        // uses the solid-render bits, not those shade samples: two leaves may
        // darken the vertex while still admitting the diagonal's light.
        let shade_diagonal = if blocked_diagonal { side_a } else { corner };
        let ao = u8::from(inputs.shade_darkened(side_a))
            + u8::from(inputs.shade_darkened(side_b))
            + u8::from(inputs.shade_darkened(shade_diagonal));
        let light = maximum_light([
            inputs.sample(light_origin),
            inputs.sample(side_a),
            inputs.sample(side_b),
            inputs.sample(shade_diagonal),
        ]);
        pack_sample(light.block(), light.sky(), ao + shade_face) | emitter
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
    rotation: u32,
) -> Option<Vec<PackedQuadLighting>> {
    bake_template_lighting_with_sampler(
        classifier,
        assets,
        network_id_mode,
        neighbourhood,
        &FullBrightLightSampler,
        block,
        template_id,
        rotation,
    )
}

/// Bakes one sampler-driven sidecar for every immutable template quad.
#[must_use]
#[allow(
    clippy::too_many_arguments,
    reason = "the sampler augments the established template-bake boundary without bundling unrelated assets"
)]
pub fn bake_template_lighting_with_sampler<S: MeshLightSampler + ?Sized>(
    classifier: &BlockClassifier,
    assets: &RuntimeAssets,
    network_id_mode: NetworkIdMode,
    neighbourhood: &MeshNeighbourhood<'_>,
    light_sampler: &S,
    block: [i32; 3],
    template_id: u32,
    rotation: u32,
) -> Option<Vec<PackedQuadLighting>> {
    bake_template(
        &DirectInputs {
            classifier,
            assets,
            network_id_mode,
            neighbourhood,
            sampler: light_sampler,
        },
        assets,
        block,
        template_id,
        rotation,
        surface_emits(classifier, assets, network_id_mode, neighbourhood, block),
    )
}

pub(crate) fn bake_template<I: LightingInputs + ?Sized>(
    inputs: &I,
    assets: &RuntimeAssets,
    block: [i32; 3],
    template_id: u32,
    rotation: u32,
    emitting_block: bool,
) -> Option<Vec<PackedQuadLighting>> {
    let template = assets.model_templates().get(template_id as usize)?;
    let start = template.quad_start as usize;
    let end = start.checked_add(template.quad_count as usize)?;
    let quads = assets.model_quads().get(start..end)?;
    // Current lily tessellator reads light at the pad's own cell.
    // Neither neighbor AO nor terrain's underside face coefficient is applied.
    if template.flags & assets::MODEL_TEMPLATE_FLAG_LILY_PAD != 0 {
        return Some(vec![lighting_at(inputs.sample(block)); quads.len()]);
    }
    Some(
        quads
            .iter()
            .map(|quad| {
                model_quad_face(*quad, rotation).map_or_else(
                    || lighting_at(inputs.sample(block)),
                    |face| {
                        bake_quad_with(
                            inputs,
                            block,
                            face,
                            quad.positions
                                .map(|position| rotate_model_position(position, rotation)),
                            true,
                            emitting_block,
                        )
                    },
                )
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
                VisualKind::Cube | VisualKind::Cross | VisualKind::Model => mask.diagonal_ao = true,
                VisualKind::Liquid => mask.liquid = true,
                VisualKind::Diagnostic | VisualKind::Invisible => {}
            }
            if assets.resolve(network_id_mode, network_value).variant()
                & assets::BLOCK_VISUAL_VARIANT_SEASONAL_LEAF
                != 0
            {
                mask.seasonal_foliage = true;
            }
            if mask.diagonal_ao && mask.liquid && mask.seasonal_foliage {
                return mask;
            }
        }
    }
    mask
}

pub(crate) const fn phase26_default_lighting() -> PackedQuadLighting {
    PackedQuadLighting::new([pack_sample(PHASE26_BLOCK_LIGHT, PHASE26_SKY_LIGHT, 0); 4])
}

const fn lighting_at(sample: MeshLightSample) -> PackedQuadLighting {
    PackedQuadLighting::new([pack_sample(sample.block(), sample.sky(), 0); 4])
}

/// Vanilla selects the brightest of the four samples independently for each channel.
fn maximum_light(samples: [MeshLightSample; 4]) -> MeshLightSample {
    let block = samples.iter().map(|s| s.block()).max().unwrap_or(0);
    let sky = samples.iter().map(|s| s.sky()).max().unwrap_or(0);
    MeshLightSample::try_new(block, sky).expect("maximum nibbles remain bounded")
}

const fn pack_sample(block: u8, sky: u8, ao: u8) -> u16 {
    debug_assert!(block <= 15 && sky <= 15 && ao <= 4);
    (block as u16) | ((sky as u16) << 4) | ((ao as u16) << 8)
}

fn sample_solid_render(
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
                if classifier.is_air(network_value) {
                    return false;
                }
                let visual = assets.resolve(network_id_mode, network_value);
                // TopSnow's constructor clears solid-render for
                // every height. Its height-7 face coverage still culls geometry.
                visual.variant() != assets::BLOCK_VISUAL_VARIANT_TOP_SNOW
                    && visual
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

const fn add_normal(mut block: [i32; 3], normal: [i32; 3]) -> [i32; 3] {
    block[0] += normal[0];
    block[1] += normal[1];
    block[2] += normal[2];
    block
}

pub(crate) const fn cube_face_positions(face: Face) -> [[i16; 3]; 4] {
    match face {
        Face::NegativeX => [[0, 0, 0], [0, 0, 256], [0, 256, 256], [0, 256, 0]],
        Face::PositiveX => [[256, 0, 0], [256, 256, 0], [256, 256, 256], [256, 0, 256]],
        Face::NegativeY => [[0, 0, 0], [256, 0, 0], [256, 0, 256], [0, 0, 256]],
        Face::PositiveY => [[0, 256, 0], [0, 256, 256], [256, 256, 256], [256, 256, 0]],
        Face::NegativeZ => [[0, 0, 0], [0, 256, 0], [256, 256, 0], [256, 0, 0]],
        Face::PositiveZ => [[0, 0, 256], [256, 0, 256], [256, 256, 256], [0, 256, 256]],
    }
}

const fn model_quad_face(quad: ModelQuad, rotation: u32) -> Option<Face> {
    let face = match quad.flags & MODEL_QUAD_FLAG_FACE_MASK {
        1 => Some(Face::NegativeY),
        2 => Some(Face::PositiveY),
        3 => Some(Face::NegativeX),
        4 => Some(Face::PositiveX),
        5 => Some(Face::NegativeZ),
        6 => Some(Face::PositiveZ),
        _ => None,
    };
    rotate_face(face, rotation)
}

const fn rotate_face(face: Option<Face>, rotation: u32) -> Option<Face> {
    match (face, rotation & 3) {
        (Some(Face::NegativeX), 1) => Some(Face::NegativeZ),
        (Some(Face::PositiveX), 1) => Some(Face::PositiveZ),
        (Some(Face::NegativeZ), 1) => Some(Face::PositiveX),
        (Some(Face::PositiveZ), 1) => Some(Face::NegativeX),
        (Some(Face::NegativeX), 2) => Some(Face::PositiveX),
        (Some(Face::PositiveX), 2) => Some(Face::NegativeX),
        (Some(Face::NegativeZ), 2) => Some(Face::PositiveZ),
        (Some(Face::PositiveZ), 2) => Some(Face::NegativeZ),
        (Some(Face::NegativeX), 3) => Some(Face::PositiveZ),
        (Some(Face::PositiveX), 3) => Some(Face::NegativeZ),
        (Some(Face::NegativeZ), 3) => Some(Face::NegativeX),
        (Some(Face::PositiveZ), 3) => Some(Face::PositiveX),
        (other, _) => other,
    }
}

const fn rotate_model_position([x, y, z]: [i16; 3], rotation: u32) -> [i16; 3] {
    match rotation & 3 {
        1 => [256 - z, y, x],
        2 => [256 - x, y, 256 - z],
        3 => [z, y, 256 - x],
        _ => [x, y, z],
    }
}

#[cfg(test)]
#[path = "lighting/native_planes.rs"]
mod native_planes;

#[cfg(test)]
#[path = "lighting/native_liquid.rs"]
mod native_liquid;

#[cfg(test)]
mod tests {
    use super::{
        HALO_VOLUME, LightingInputs, MeshLightSample, bake_quad, bake_quad_with, halo_index,
    };
    use crate::Face;

    /// A log to the west (opaque, unlit) beside an open cell lit at block 9, sky 12.
    struct VineOnLog {
        own_solid: bool,
    }

    impl LightingInputs for VineOnLog {
        fn occludes(&self, coordinate: [i32; 3]) -> bool {
            coordinate[0] < 0 || (coordinate == [0, 0, 0] && self.own_solid)
        }

        fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample {
            if coordinate[0] < 0 {
                MeshLightSample::try_new(0, 0).unwrap()
            } else {
                MeshLightSample::try_new(9, 12).unwrap()
            }
        }
    }

    // A model face against an opaque neighbour reads its own cell instead of the dark neighbour.
    #[test]
    fn attached_model_face_samples_its_own_cell() {
        let positions = [[0, 0, 0], [0, 0, 256], [0, 256, 256], [0, 256, 0]];
        let attached = bake_quad_with(
            &VineOnLog { own_solid: false },
            [0, 0, 0],
            Face::NegativeX,
            positions,
            true,
            false,
        );
        let plain = bake_quad(
            &VineOnLog { own_solid: true },
            [0, 0, 0],
            Face::NegativeX,
            positions,
            false,
        );
        assert_eq!(attached.samples()[0] & 0xff, 9 | (12 << 4));
        assert_eq!(plain.samples()[0] & 0xff, 0);
    }

    /// The memo covers exactly the one-block halo; anything else takes the direct path.
    #[test]
    fn halo_index_covers_only_the_one_block_halo() {
        assert_eq!(halo_index([-1, -1, -1]), Some(0));
        assert_eq!(halo_index([16, 16, 16]), Some(HALO_VOLUME - 1));
        for outside in [[-2, 0, 0], [0, 17, 0], [0, 0, i32::MAX], [i32::MIN, 0, 0]] {
            assert_eq!(halo_index(outside), None, "{outside:?}");
        }
    }
}
