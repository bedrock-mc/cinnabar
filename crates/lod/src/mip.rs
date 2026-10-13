//! Section-level mipping: one child region into its parent, the local L0..L4 chain with
//! exact early-out, and the full re-mip of a section from its eight children.
//!
//! Parent biome columns take the majority of the known 2×2 fine cells of a child; cells at
//! [`UNKNOWN_BIOME`] never vote and a child with no known cell leaves the parent column
//! as it is. The sampler writes one surface biome per block column into every sub-chunk it
//! samples, so vertically stacked children agree wherever both are known, and the chain and
//! [`remip_from_children`] then give equal biomes in any order. Where stale children
//! disagree, the chain keeps the last child mipped and the re-mip the upper one.

use crate::{
    biome_grid::{BiomeGrid, UNKNOWN_BIOME},
    classify::{BlockClass, MaterialClasses, voxel_class},
    exposure::Neighbourhood,
    geometry::{CHILD_SPAN, Face, Octant, SECTION_SIDE, SectionPos, VoxelBox},
    mip_policy::{
        GroupDecision, HIDDEN_FALLBACK_ORDER, HiddenFallback, MipChild, fallback_voxel, majority,
        mip_group,
    },
    section::{ChildMask, ChildState},
    storage::DenseSection,
    voxel::{Light, MaterialId, Voxel},
};

/// Highest level the per-sub-chunk chain writes; a 16³ sub-chunk is one level-4 voxel.
/// Levels above are rebuilt from eight children by [`remip_from_children`].
pub const LOCAL_MIP_TOP_LEVEL: u8 = 4;

/// Parent fill for a child that was never sampled: unlit air.
pub const UNKNOWN_CHILD_FILL: Voxel = Voxel::AIR;
/// Parent fill for a child known to hold only air: sky-lit air.
pub const EMPTY_CHILD_FILL: Voxel = Voxel::air(Light::FULL_SKY);

/// A section nothing has been sampled or mipped into: [`UNKNOWN_CHILD_FILL`] voxels and
/// [`UNKNOWN_BIOME`] columns. Level-0 sections and chain ancestors start from it.
#[must_use]
pub fn unknown_section() -> DenseSection {
    DenseSection::filled(UNKNOWN_CHILD_FILL, UNKNOWN_BIOME)
}

/// What one mip step changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct MipOutcome {
    /// Parent voxels whose word or biome changed, in parent coordinates.
    pub changed: Option<VoxelBox>,
    /// Coarse voxels decided by the hidden fallback.
    pub hidden_fallbacks: u32,
    /// Hidden fallbacks that found no material and stayed [`Voxel::HIDDEN`].
    pub unresolved_hidden: u32,
}

/// Coarse voxels (child-octant coordinates) a change to fine `dirty` can alter: the
/// exposure halo widened to whole 2×2×2 coarse groups for the parent-group fallback, over
/// the full height for the column fallback.
fn affected_coarse_range(dirty: VoxelBox) -> ([usize; 3], [usize; 3]) {
    let halo = dirty.expanded(1);
    let group_min = |fine: usize| fine / 4 * 2;
    let group_max = |fine: usize| (fine.div_ceil(4) * 2).min(CHILD_SPAN);
    let [x0, _, z0] = halo.min();
    let [x1, _, z1] = halo.max();
    (
        [group_min(x0), 0, group_min(z0)],
        [group_max(x1), CHILD_SPAN, group_max(z1)],
    )
}

/// Mips the part of `child` affected by `dirty` (child coordinates) into `octant` of
/// `parent`, with every coarse voxel and biome cell that change can reach. `neighbours`
/// holds the planes across the child's faces; when one of them changes, call again with
/// that face's [`Face::slab`] as `dirty`. Biome columns follow the module rule; the
/// parent's child mask is the caller's.
pub fn mip_into_parent(
    child: &DenseSection,
    neighbours: &Neighbourhood,
    octant: Octant,
    dirty: VoxelBox,
    classes: &impl MaterialClasses,
    parent: &mut DenseSection,
) -> MipOutcome {
    let (coarse_min, coarse_max) = affected_coarse_range(dirty);
    let read = VoxelBox::new(
        coarse_min.map(|value| (value * 2).saturating_sub(1)),
        coarse_max.map(|value| value * 2 + 1),
    )
    .unwrap_or(VoxelBox::FULL);
    let view = FineView::new(child, neighbours, classes, read);
    let base = octant.parent_region().min();
    let mut outcome = MipOutcome::default();
    for cy in coarse_min[1]..coarse_max[1] {
        for cz in coarse_min[2]..coarse_max[2] {
            for cx in coarse_min[0]..coarse_max[0] {
                let voxel = view.mip_voxel([cx, cy, cz], &mut outcome);
                let [px, py, pz] = [base[0] + cx, base[1] + cy, base[2] + cz];
                if parent.get(px, py, pz) != voxel {
                    parent.set(px, py, pz, voxel);
                    outcome.changed = union(outcome.changed, VoxelBox::voxel(px, py, pz));
                }
            }
        }
    }
    for cz in coarse_min[2]..coarse_max[2] {
        for cx in coarse_min[0]..coarse_max[0] {
            let Some(biome) = biome_majority(child, cx, cz) else {
                continue;
            };
            let [px, pz] = [base[0] + cx, base[2] + cz];
            if parent.biomes().get(px, pz) != biome && parent.biomes_mut().set(px, pz, biome) {
                let column =
                    VoxelBox::new([px, base[1], pz], [px + 1, base[1] + CHILD_SPAN, pz + 1]);
                outcome.changed = union(outcome.changed, column);
            }
        }
    }
    outcome
}

/// What a local chain wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChainOutcome {
    /// Ancestors rewritten: `chain[1..=updated_levels]` changed in voxels, biomes or child
    /// mask and must be stored.
    pub updated_levels: usize,
    pub hidden_fallbacks: u32,
    pub unresolved_hidden: u32,
}

/// Propagates a change in `chain[0]` (at `base`) up through its ancestors, where
/// `chain[i]` is the section at `base.ancestor(i)`, `masks[i]` its child mask and
/// `neighbours[i]` the planes across its faces (one fewer than `chain`; the top section is
/// not mipped).
///
/// Each step records the child's [`ChildState`] in its parent's mask: a level-0 base is
/// known once sampled ([`ChildState::of_level_zero`]), every other section reports
/// [`ChildMask::summary`]. A step stops the chain only when the parent's words, biomes and
/// mask are all unchanged, which is exact for unchanged neighbour planes. Panics if the
/// chain passes the top level, if `masks` or `neighbours` do not match `chain` in length,
/// or if a level-0 mask is not [`ChildMask::UNKNOWN`].
pub fn mip_chain(
    chain: &mut [DenseSection],
    masks: &mut [ChildMask],
    neighbours: &[Neighbourhood],
    base: SectionPos,
    dirty: VoxelBox,
    classes: &impl MaterialClasses,
) -> ChainOutcome {
    assert_eq!(chain.len(), masks.len(), "one child mask per chain section");
    assert_eq!(
        neighbours.len(),
        chain.len().saturating_sub(1),
        "one neighbourhood per mipped chain section"
    );
    let top = u8::try_from(chain.len().saturating_sub(1)).expect("chain fits the level count");
    assert!(
        base.ancestor(top).is_some(),
        "mip chain from {base:?} passes the top level"
    );
    assert!(
        base.level() > 0 || masks.first().is_none_or(|&mask| mask == ChildMask::UNKNOWN),
        "level-0 sections have no children"
    );
    let mut outcome = ChainOutcome::default();
    let mut dirty = Some(dirty);
    let mut pos = base;
    for level in 0..chain.len().saturating_sub(1) {
        let (fine, coarse) = chain.split_at_mut(level + 1);
        let state = if pos.level() == 0 {
            ChildState::of_level_zero(&fine[level])
        } else {
            masks[level].summary()
        };
        let octant = pos.octant_in_parent();
        let mask_changed = masks[level + 1].set(octant, state);
        let step = match dirty {
            Some(region) => mip_into_parent(
                &fine[level],
                &neighbours[level],
                octant,
                region,
                classes,
                &mut coarse[0],
            ),
            None if mask_changed => MipOutcome::default(),
            None => break,
        };
        outcome.hidden_fallbacks += step.hidden_fallbacks;
        outcome.unresolved_hidden += step.unresolved_hidden;
        if step.changed.is_some() || mask_changed {
            outcome.updated_levels = level + 1;
        }
        dirty = step.changed;
        pos = pos.parent().expect("checked against the top level");
    }
    outcome
}

/// One child of a full re-mip.
#[derive(Debug, Clone, Copy)]
pub enum ChildInput<'a> {
    /// Never sampled or loaded.
    Unknown,
    /// Known to hold only air, with no stored section.
    Empty,
    /// A stored section and its state from its own mask ([`ChildMask::summary`]), or
    /// [`ChildState::of_level_zero`] for a level-0 child; never derived from its voxels,
    /// whose air may be unknown fill.
    Present {
        section: &'a DenseSection,
        state: ChildState,
    },
}

/// What a full re-mip produced besides the voxels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RemipOutcome {
    pub children: ChildMask,
    pub hidden_fallbacks: u32,
    pub unresolved_hidden: u32,
}

/// Rebuilds every voxel and biome of `out` from its eight children, as used for levels
/// above [`LOCAL_MIP_TOP_LEVEL`]; nothing of `out` survives, so one scratch section can
/// serve many re-mips. `neighbours[i]` holds the planes across child `i`'s faces,
/// including faces shared with siblings. Absent children fill their region with [`UNKNOWN_CHILD_FILL`] or
/// [`EMPTY_CHILD_FILL`] and give no biome; columns no present child knows stay
/// [`UNKNOWN_BIOME`]. Equals the local chain over the same children when every child the
/// chain mipped is passed as present.
pub fn remip_from_children(
    children: [ChildInput<'_>; 8],
    neighbours: &[Neighbourhood; 8],
    classes: &impl MaterialClasses,
    out: &mut DenseSection,
) -> RemipOutcome {
    let mut outcome = RemipOutcome {
        children: ChildMask::UNKNOWN,
        hidden_fallbacks: 0,
        unresolved_hidden: 0,
    };
    *out.biomes_mut() = BiomeGrid::uniform(UNKNOWN_BIOME);
    // `Octant::ALL` lists lower octants first, so upper children win stale disagreements.
    for (octant, child) in Octant::ALL.into_iter().zip(children) {
        let state = match child {
            ChildInput::Unknown => {
                out.fill_box(octant.parent_region(), UNKNOWN_CHILD_FILL);
                ChildState::Unknown
            }
            ChildInput::Empty => {
                out.fill_box(octant.parent_region(), EMPTY_CHILD_FILL);
                ChildState::Empty
            }
            ChildInput::Present { section, state } => {
                let step = mip_into_parent(
                    section,
                    &neighbours[octant.index()],
                    octant,
                    VoxelBox::FULL,
                    classes,
                    out,
                );
                outcome.hidden_fallbacks += step.hidden_fallbacks;
                outcome.unresolved_hidden += step.unresolved_hidden;
                state
            }
        };
        outcome.children.set(octant, state);
    }
    outcome
}

/// The smallest box holding both, where `None` is the empty box.
fn union(current: Option<VoxelBox>, added: Option<VoxelBox>) -> Option<VoxelBox> {
    match (current, added) {
        (Some(current), Some(added)) => Some(current.union(added)),
        (current, added) => current.or(added),
    }
}

/// Majority of the known 2×2 fine biome cells under coarse column `(cx, cz)`, ties to the
/// lower id; `None` when all four are [`UNKNOWN_BIOME`].
fn biome_majority(child: &DenseSection, cx: usize, cz: usize) -> Option<u32> {
    let cells = [(0, 0), (1, 0), (0, 1), (1, 1)]
        .map(|(dx, dz)| child.biomes().get(cx * 2 + dx, cz * 2 + dz));
    let count = |biome: u32| cells.iter().filter(|&&cell| cell == biome).count();
    cells
        .into_iter()
        .filter(|&cell| cell != UNKNOWN_BIOME)
        .max_by(|&a, &b| count(a).cmp(&count(b)).then(b.cmp(&a)))
}

/// Classes of a fine section, cached over the region one mip step reads.
struct FineView<'a, C> {
    section: &'a DenseSection,
    neighbours: &'a Neighbourhood,
    classes: &'a C,
    region: VoxelBox,
    cached: Vec<BlockClass>,
}

impl<'a, C: MaterialClasses> FineView<'a, C> {
    /// Caches the classes of `section` inside `region`; reads outside it classify on demand.
    fn new(
        section: &'a DenseSection,
        neighbours: &'a Neighbourhood,
        classes: &'a C,
        region: VoxelBox,
    ) -> Self {
        let [x0, y0, z0] = region.min();
        let [x1, y1, z1] = region.max();
        let mut cached = Vec::with_capacity((x1 - x0) * (y1 - y0) * (z1 - z0));
        let mut last = None;
        for y in y0..y1 {
            for z in z0..z1 {
                for x in x0..x1 {
                    let voxel = section.get(x, y, z);
                    let class = match last {
                        Some((previous, class)) if previous == voxel => class,
                        _ => voxel_class(classes, voxel),
                    };
                    last = Some((voxel, class));
                    cached.push(class);
                }
            }
        }
        Self {
            section,
            neighbours,
            classes,
            region,
            cached,
        }
    }

    /// The [`voxel_class`] of the fine voxel at `[x, y, z]`; panics outside the section.
    fn class(&self, [x, y, z]: [usize; 3]) -> BlockClass {
        if self.region.contains(x, y, z) {
            let [x0, y0, z0] = self.region.min();
            let [x1, _, z1] = self.region.max();
            let (width, depth) = (x1 - x0, z1 - z0);
            self.cached[((y - y0) * depth + (z - z0)) * width + (x - x0)]
        } else {
            voxel_class(self.classes, self.section.get(x, y, z))
        }
    }

    /// True when a face neighbour exposes the voxel. Outside the section the known
    /// neighbour plane decides; across an unknown face non-hidden voxels are exposed and
    /// hidden ones are not (see [`crate::mip_policy`]).
    fn exposed(&self, at: [usize; 3]) -> bool {
        let unknown_exposes = !self.section.get(at[0], at[1], at[2]).is_hidden();
        (0..3).any(|axis| {
            [false, true].into_iter().any(|positive| {
                let step = if positive {
                    Some(at[axis] + 1).filter(|&value| value < SECTION_SIDE)
                } else {
                    at[axis].checked_sub(1)
                };
                match step {
                    Some(value) => {
                        let mut neighbour = at;
                        neighbour[axis] = value;
                        self.class(neighbour).exposes_neighbours()
                    }
                    None => {
                        let face = Face::of_axis(axis, positive);
                        self.neighbours
                            .get(face)
                            .map_or(unknown_exposes, |plane| plane.exposes(face.in_plane(at)))
                    }
                }
            })
        })
    }

    /// The material of an exposed solid voxel: not air, water or hidden.
    fn surface_material(&self, at: [usize; 3]) -> Option<MaterialId> {
        let voxel = self.section.get(at[0], at[1], at[2]);
        let solid = !voxel.is_air() && !voxel.is_hidden() && self.class(at) != BlockClass::Water;
        (solid && self.exposed(at)).then(|| voxel.material())
    }

    /// The coarse voxel of child-octant group `[cx, cy, cz]` under the mip policy,
    /// resolving hidden fallbacks and counting them in `outcome`.
    fn mip_voxel(&self, [cx, cy, cz]: [usize; 3], outcome: &mut MipOutcome) -> Voxel {
        let children = Octant::ALL.map(|octant| {
            let at = [
                cx * 2 + octant.x(),
                cy * 2 + octant.y(),
                cz * 2 + octant.z(),
            ];
            MipChild {
                voxel: self.section.get(at[0], at[1], at[2]),
                class: self.class(at),
                exposed: self.exposed(at),
            }
        });
        match mip_group(&children) {
            GroupDecision::Voxel(voxel, _) => voxel,
            GroupDecision::HiddenFallback(traits) => {
                outcome.hidden_fallbacks += 1;
                let material = HIDDEN_FALLBACK_ORDER
                    .into_iter()
                    .find_map(|step| self.fallback(step, [cx, cy, cz]));
                outcome.unresolved_hidden += u32::from(material.is_none());
                fallback_voxel(material, traits)
            }
        }
    }

    /// The material one [`HiddenFallback`] step finds for group `[cx, cy, cz]`, if any.
    fn fallback(&self, step: HiddenFallback, [cx, cy, cz]: [usize; 3]) -> Option<MaterialId> {
        match step {
            HiddenFallback::ParentGroupMajority => {
                let [gx, gy, gz] = [cx, cy, cz].map(|value| (value & !1) * 2);
                majority(
                    (gy..gy + 4)
                        .flat_map(|y| {
                            (gz..gz + 4).flat_map(move |z| (gx..gx + 4).map(move |x| [x, y, z]))
                        })
                        .filter_map(|at| {
                            self.surface_material(at).map(|material| (material, at[1]))
                        }),
                )
            }
            HiddenFallback::NearestInColumn => {
                let rows = |y: usize| {
                    (cz * 2..cz * 2 + 2)
                        .flat_map(move |z| (cx * 2..cx * 2 + 2).map(move |x| [x, y, z]))
                };
                (0..SECTION_SIDE).find_map(|distance| {
                    let above = cy * 2 + 1 + distance;
                    let below = (cy * 2).checked_sub(distance);
                    let candidates = (above < SECTION_SIDE)
                        .then(|| rows(above))
                        .into_iter()
                        .flatten()
                        .chain(below.into_iter().flat_map(rows))
                        .filter_map(|at| {
                            self.surface_material(at).map(|material| (material, at[1]))
                        });
                    majority(candidates)
                })
            }
        }
    }
}

#[cfg(test)]
mod level_tests;
#[cfg(test)]
mod tests;
