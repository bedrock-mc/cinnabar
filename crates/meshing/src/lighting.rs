use std::cell::Cell;

mod java;
pub use java::JAVA_SAMPLE_FLAG;

use assets::{MODEL_QUAD_FLAG_FACE_MASK, ModelQuad, NetworkIdMode, RuntimeAssets, VisualKind};
use world::{MeshDependencyMask, MeshNeighbourhood, SubChunk};

use crate::{BlockClassifier, ContributorResolver, Face, PackedQuadLighting};

/// Light inputs for meshes baked without a light sampler.
pub const FULL_BRIGHT_BLOCK_LIGHT: u8 = 0;
pub const FULL_BRIGHT_SKY_LIGHT: u8 = 15;

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
    pub const FULL_BRIGHT: Self = Self(FULL_BRIGHT_BLOCK_LIGHT | (FULL_BRIGHT_SKY_LIGHT << 4));

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

/// One halo cell's inputs to a bake corner, read together.
#[derive(Clone, Copy)]
pub(crate) struct CellInputs {
    occludes: bool,
    shade_darkened: bool,
    light: MeshLightSample,
}

/// Occlusion and light lookups shared by the direct and cached bake paths.
pub(crate) trait LightingInputs {
    /// Selects the optional corner-light calculation for this immutable mesh job.
    fn java_world_lighting(&self) -> bool {
        false
    }

    /// Whether terrain faces interpolate neighbouring light and geometric shade.
    fn smooth_lighting(&self) -> bool {
        true
    }
    /// Native cached solid-render bit, independent of geometric face coverage.
    fn occludes(&self, coordinate: [i32; 3]) -> bool;
    /// Native shade brightness is 0.2 for leaves and solid render blocks; only
    /// the latter also block diagonal light sampling. Light emission does not
    /// count, so torches and lanterns leave neighbouring faces unshaded.
    fn shade_darkened(&self, coordinate: [i32; 3]) -> bool {
        self.occludes(coordinate)
    }
    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample;
    /// [`Self::occludes`], [`Self::shade_darkened`] and [`Self::sample`] of one cell; caches
    /// answer all three from one lookup.
    fn cell(&self, coordinate: [i32; 3]) -> CellInputs {
        CellInputs {
            occludes: self.occludes(coordinate),
            shade_darkened: self.shade_darkened(coordinate),
            light: self.sample(coordinate),
        }
    }
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
        self.occludes(coordinate) || self.leaf_shaded(coordinate)
    }

    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample {
        self.sampler.sample(coordinate)
    }
}

impl<S: MeshLightSampler + ?Sized> DirectInputs<'_, '_, S> {
    /// Whether the storage-zero block is a leaf, which darkens shade whatever its solidity.
    fn leaf_shaded(&self, coordinate: [i32; 3]) -> bool {
        let Some((chunk, local)) = self.neighbourhood.block_source(coordinate) else {
            return false;
        };
        chunk
            .runtime_id(0, local[0], local[1], local[2])
            .filter(|&id| !self.classifier.is_air(id))
            .is_some_and(|id| {
                let visual = self.assets.resolve(self.network_id_mode, id);
                // Vanilla shade brightness also counts leaves, whatever their solidity.
                visual.flags().contains(assets::BlockFlags::LEAF_MODEL)
            })
    }
}

const HALO_SIDE: usize = 18;
const HALO_VOLUME: usize = HALO_SIDE * HALO_SIDE * HALO_SIDE;
const HALO_WORDS: usize = HALO_VOLUME.div_ceil(64);

/// Per-mesh memo of light samples and native solid-render bits over the one-block
/// halo, filled lazily so sparse sub-chunks resolve only what their faces touch.
/// Misses read the mesh's per-palette-value bits rather than resolving each cell.
pub(crate) struct MeshLightingCache<'a, 'n, S: MeshLightSampler + ?Sized> {
    smooth_lighting: bool,
    direct: DirectInputs<'a, 'n, S>,
    bits: &'a crate::palette_bits::NeighbourhoodBlockBits<'a, 'n>,
    light: [Cell<MeshLightSample>; HALO_VOLUME],
    light_known: [Cell<u64>; HALO_WORDS],
    occluder_known: [Cell<u64>; HALO_WORDS],
    occluders: [Cell<u64>; HALO_WORDS],
    shade_known: [Cell<u64>; HALO_WORDS],
    darkened_shades: [Cell<u64>; HALO_WORDS],
    java_world_lighting: bool,
}

impl<'a, 'n, S: MeshLightSampler + ?Sized> MeshLightingCache<'a, 'n, S> {
    pub(crate) fn new(
        classifier: &'a BlockClassifier,
        assets: &'a RuntimeAssets,
        network_id_mode: NetworkIdMode,
        neighbourhood: &'a MeshNeighbourhood<'n>,
        sampler: &'a S,
        bits: &'a crate::palette_bits::NeighbourhoodBlockBits<'a, 'n>,
        smooth_lighting: bool,
    ) -> Self {
        Self {
            smooth_lighting,
            direct: DirectInputs {
                classifier,
                assets,
                network_id_mode,
                neighbourhood,
                sampler,
            },
            bits,
            light: [const { Cell::new(MeshLightSample::FULL_BRIGHT) }; HALO_VOLUME],
            light_known: [const { Cell::new(0) }; HALO_WORDS],
            occluder_known: [const { Cell::new(0) }; HALO_WORDS],
            occluders: [const { Cell::new(0) }; HALO_WORDS],
            shade_known: [const { Cell::new(0) }; HALO_WORDS],
            darkened_shades: [const { Cell::new(0) }; HALO_WORDS],
            java_world_lighting: false,
        }
    }
}

impl<S: MeshLightSampler + ?Sized> MeshLightingCache<'_, '_, S> {
    /// Selects a job-local light format without changing solved world light.
    pub(crate) fn with_java_world_lighting(mut self, enabled: bool) -> Self {
        self.java_world_lighting = enabled;
        self
    }
    fn occludes_at(&self, index: usize, coordinate: [i32; 3]) -> bool {
        let (word, bit) = (index / 64, 1_u64 << (index % 64));
        if self.occluder_known[word].get() & bit == 0 {
            if self.bits.solid_render(coordinate) {
                self.occluders[word].set(self.occluders[word].get() | bit);
            }
            self.occluder_known[word].set(self.occluder_known[word].get() | bit);
        }
        self.occluders[word].get() & bit != 0
    }

    fn shade_darkened_at(&self, index: usize, coordinate: [i32; 3]) -> bool {
        let (word, bit) = (index / 64, 1_u64 << (index % 64));
        if self.shade_known[word].get() & bit == 0 {
            if self.occludes_at(index, coordinate)
                || self.bits.layer(0, coordinate) & crate::palette_bits::SHADE_LEAF != 0
            {
                self.darkened_shades[word].set(self.darkened_shades[word].get() | bit);
            }
            self.shade_known[word].set(self.shade_known[word].get() | bit);
        }
        self.darkened_shades[word].get() & bit != 0
    }

    fn sample_at(&self, index: usize, coordinate: [i32; 3]) -> MeshLightSample {
        let (word, bit) = (index / 64, 1_u64 << (index % 64));
        if self.light_known[word].get() & bit == 0 {
            self.light[index].set(self.direct.sample(coordinate));
            self.light_known[word].set(self.light_known[word].get() | bit);
        }
        self.light[index].get()
    }
}

impl<S: MeshLightSampler + ?Sized> LightingInputs for MeshLightingCache<'_, '_, S> {
    fn java_world_lighting(&self) -> bool {
        self.java_world_lighting
    }

    /// Uses the quality captured when this mesh cache was created.
    fn smooth_lighting(&self) -> bool {
        self.smooth_lighting
    }
    fn occludes(&self, coordinate: [i32; 3]) -> bool {
        halo_index(coordinate).map_or_else(
            || self.direct.occludes(coordinate),
            |index| self.occludes_at(index, coordinate),
        )
    }

    fn shade_darkened(&self, coordinate: [i32; 3]) -> bool {
        halo_index(coordinate).map_or_else(
            || self.direct.shade_darkened(coordinate),
            |index| self.shade_darkened_at(index, coordinate),
        )
    }

    fn sample(&self, coordinate: [i32; 3]) -> MeshLightSample {
        halo_index(coordinate).map_or_else(
            || self.direct.sample(coordinate),
            |index| self.sample_at(index, coordinate),
        )
    }

    fn cell(&self, coordinate: [i32; 3]) -> CellInputs {
        let Some(index) = halo_index(coordinate) else {
            return self.direct.cell(coordinate);
        };
        CellInputs {
            occludes: self.occludes_at(index, coordinate),
            shade_darkened: self.shade_darkened_at(index, coordinate),
            light: self.sample_at(index, coordinate),
        }
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
    // y+1 plane, rounding each channel. Admission uses a per-block property > 0.5,
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
    if !inputs.smooth_lighting() {
        return bake_flat_quad(inputs, block, face, positions, emitting_block);
    }
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

/// Repeats one face light sample without corner interpolation or ambient occlusion.
fn bake_flat_quad<I: LightingInputs + ?Sized>(
    inputs: &I,
    block: [i32; 3],
    face: Face,
    positions: [[i16; 3]; 4],
    emitting_block: bool,
) -> PackedQuadLighting {
    let (normal, _, _) = face_basis(face);
    let axis = normal
        .iter()
        .position(|&value| value != 0)
        .expect("face axis");
    let boundary = positions.iter().all(|position| {
        if normal[axis] < 0 {
            position[axis] <= 0
        } else {
            position[axis] == 256
        }
    });
    let sample = inputs.sample(if boundary {
        add_normal(block, normal)
    } else {
        block
    });
    let packed = pack_sample(sample.block(), sample.sky(), 0) | (u16::from(emitting_block) << 11);
    PackedQuadLighting::new([packed; 4])
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
    if inputs.java_world_lighting() && !liquid_outward_center {
        return java::bake(inputs, block, face, positions, check_bounds);
    }
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
    let origin_light = inputs.sample(light_origin);
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
        let a = inputs.cell(side_a);
        let b = inputs.cell(side_b);
        // Vanilla ambient occlusion averages four independent 0.2/1 shade samples. Its diagonal fallback
        // uses the solid-render bits, not those shade samples: two leaves may
        // darken the vertex while still admitting the diagonal's light.
        let diagonal = if a.occludes && b.occludes {
            a
        } else {
            inputs.cell(corner)
        };
        let ao = u8::from(a.shade_darkened)
            + u8::from(b.shade_darkened)
            + u8::from(diagonal.shade_darkened);
        let light = maximum_light([origin_light, a.light, b.light, diagonal.light]);
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
    // Lily pads, bubble columns and torches read light at their own cell.
    // Neither neighbor AO nor terrain's underside face coefficient is applied.
    if template.flags
        & (assets::MODEL_TEMPLATE_FLAG_LILY_PAD
            | assets::MODEL_TEMPLATE_FLAG_BUBBLE_COLUMN
            | assets::MODEL_TEMPLATE_FLAG_TORCH)
        != 0
    {
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

/// Nether portals take vanilla's flat lighting path: boundary-adjacent light, own light
/// on inset faces, no AO, and the registered emission as a per-face minimum.
pub(crate) fn bake_portal_template<I: LightingInputs + ?Sized>(
    inputs: &I,
    assets: &RuntimeAssets,
    block: [i32; 3],
    template_id: u32,
    emission: u8,
) -> Option<Vec<PackedQuadLighting>> {
    let template = assets.model_templates().get(template_id as usize)?;
    let start = template.quad_start as usize;
    let end = start.checked_add(template.quad_count as usize)?;
    let quads = assets.model_quads().get(start..end)?;
    Some(
        quads
            .iter()
            .map(|quad| {
                let sample_position = model_quad_face(*quad, 0).map_or(block, |face| {
                    let (normal, _, _) = face_basis(face);
                    let axis = normal.iter().position(|&n| n != 0).expect("face axis");
                    let boundary = quad.positions.iter().all(|position| {
                        if normal[axis] < 0 {
                            position[axis] <= 0
                        } else {
                            position[axis] >= 256
                        }
                    });
                    if boundary {
                        add_normal(block, normal)
                    } else {
                        block
                    }
                });
                let sample = inputs.sample(sample_position);
                let packed = pack_sample(sample.block().max(emission), sample.sky(), 0)
                    | (u16::from(emission > 0) << 11);
                PackedQuadLighting::new([packed; 4])
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

pub(crate) const fn full_bright_lighting() -> PackedQuadLighting {
    PackedQuadLighting::new([pack_sample(FULL_BRIGHT_BLOCK_LIGHT, FULL_BRIGHT_SKY_LIGHT, 0); 4])
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
#[path = "lighting/flat_tests.rs"]
mod flat_tests;
#[cfg(test)]
#[path = "lighting/native_planes.rs"]
mod native_planes;

#[cfg(test)]
#[path = "lighting/native_liquid.rs"]
mod native_liquid;

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::{
        CellInputs, HALO_VOLUME, LightingInputs, MeshLightSample, bake_quad, bake_quad_with,
        halo_index,
    };
    use crate::Face;

    /// Cells read whole; per-input reads count separately, so a bake's reads are visible.
    struct CountedReads {
        solid: fn([i32; 3]) -> bool,
        cells: Cell<usize>,
        single: Cell<usize>,
    }

    impl LightingInputs for CountedReads {
        fn occludes(&self, coordinate: [i32; 3]) -> bool {
            self.single.set(self.single.get() + 1);
            (self.solid)(coordinate)
        }

        fn sample(&self, _coordinate: [i32; 3]) -> MeshLightSample {
            self.single.set(self.single.get() + 1);
            MeshLightSample::try_new(4, 9).unwrap()
        }

        fn cell(&self, coordinate: [i32; 3]) -> CellInputs {
            self.cells.set(self.cells.get() + 1);
            let solid = (self.solid)(coordinate);
            CellInputs {
                occludes: solid,
                shade_darkened: solid,
                light: MeshLightSample::try_new(4, 9).unwrap(),
            }
        }
    }

    /// Each corner reads its two sides and, unless both block it, its diagonal once; the face
    /// reads its own block, light origin and outward shade once rather than per corner.
    #[test]
    fn bake_reads_each_corner_cell_once() {
        let positions = [[0, 256, 0], [256, 256, 0], [256, 256, 256], [0, 256, 256]];
        let open = CountedReads {
            solid: |_| false,
            cells: Cell::new(0),
            single: Cell::new(0),
        };
        bake_quad(&open, [0, 0, 0], Face::PositiveY, positions, false);
        assert_eq!(open.cells.get(), 4 * 3);
        assert_eq!(open.single.get(), 3);

        // Solid on both sides of every corner: the diagonal reuses a side.
        let walled = CountedReads {
            solid: |[x, y, z]| y == 1 && (x != 0 || z != 0),
            cells: Cell::new(0),
            single: Cell::new(0),
        };
        bake_quad(&walled, [0, 0, 0], Face::PositiveY, positions, false);
        assert_eq!(walled.cells.get(), 4 * 2);
    }

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
