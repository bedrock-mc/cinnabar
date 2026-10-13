//! The 2×2×2 → 1 mip policy: which rule picks a coarse voxel from its eight children.
//!
//! Rules apply in [`MIP_RULES`] order; the first that matches decides:
//!
//! | Rule | Matches when | Material |
//! | --- | --- | --- |
//! | [`MipRule::AllAir`] | every child is air | air |
//! | [`MipRule::WaterSurface`] | most non-air top-layer children are water | majority top-layer water |
//! | [`MipRule::ExposedMajority`] | a non-hidden child is exposed | majority exposed |
//! | [`MipRule::HiddenFallback`] | only hidden children are exposed | [`HIDDEN_FALLBACK_ORDER`] |
//! | [`MipRule::EnclosedMajority`] | no child is exposed | majority non-hidden non-air |
//! | [`MipRule::AllHidden`] | every non-air child is hidden | [`Voxel::HIDDEN`] |
//!
//! Air is decided by the voxel's material, never its class, so occupancy is conservative:
//! the coarse voxel is non-air whenever any child is, and hidden children count as opaque.
//! A child is exposed when a face neighbour exposes neighbours
//! ([`BlockClass::exposes_neighbours`]); leaves and hidden voxels do not. Neighbours outside
//! the fine section come from the caller's [`crate::exposure::Neighbourhood`], which gives
//! full 6-neighbour data. Across a face whose neighbour is unknown, non-hidden
//! children count as exposed (unloaded is not opaque, as for the level-0 hidden pass) and
//! hidden ones do not, since they were enclosed when sampled. Majorities break ties by the higher child Y, then the lower material id. Light
//! is the mean over see-through children, coverage the mean over all eight, and water is
//! set when any child has water.

use crate::{
    classify::BlockClass,
    geometry::Octant,
    voxel::{Light, MaterialId, Voxel},
};

/// The rule that decided one coarse voxel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MipRule {
    AllAir,
    WaterSurface,
    ExposedMajority,
    HiddenFallback,
    EnclosedMajority,
    AllHidden,
}

/// Mip rules in evaluation order.
pub const MIP_RULES: [MipRule; 6] = [
    MipRule::AllAir,
    MipRule::WaterSurface,
    MipRule::ExposedMajority,
    MipRule::HiddenFallback,
    MipRule::EnclosedMajority,
    MipRule::AllHidden,
];

/// How an integer mean rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rounding {
    /// Half rounds up.
    Nearest,
    Up,
    Down,
}

impl Rounding {
    /// `sum / count` rounded; zero for an empty set.
    #[must_use]
    pub const fn mean(self, sum: u32, count: u32) -> u32 {
        if count == 0 {
            return 0;
        }
        match self {
            Self::Nearest => (sum + count / 2) / count,
            Self::Up => sum.div_ceil(count),
            Self::Down => sum / count,
        }
    }
}

/// Rounding of the light mean over see-through children, sky and block separately.
pub const LIGHT_MEAN_ROUNDING: Rounding = Rounding::Nearest;
/// Rounding of the coverage mean over all eight children.
pub const COVERAGE_MEAN_ROUNDING: Rounding = Rounding::Nearest;

/// Where an exposed group of only hidden children finds a material, in order. Solid means
/// neither air, water nor hidden, so a hidden opaque group never turns into water.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HiddenFallback {
    /// Majority exposed solid material of the enclosing 4×4×4 fine group.
    ParentGroupMajority,
    /// Majority exposed solid material at the nearest Y within the group's fine columns.
    NearestInColumn,
}

/// Fallback order for [`MipRule::HiddenFallback`]; if every step fails the voxel stays
/// [`Voxel::HIDDEN`] and is counted as unresolved. Both steps read only the group's own
/// fine columns, so an incremental mip stays exact.
pub const HIDDEN_FALLBACK_ORDER: [HiddenFallback; 2] = [
    HiddenFallback::ParentGroupMajority,
    HiddenFallback::NearestInColumn,
];

/// One child of a mip group, indexed by [`Octant`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MipChild {
    pub voxel: Voxel,
    /// The voxel's class from [`crate::classify::voxel_class`].
    pub class: BlockClass,
    /// Whether a face neighbour exposes this child, as the module documentation defines.
    pub exposed: bool,
}

/// The outcome of [`mip_group`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupDecision {
    /// The coarse voxel and the rule that chose it.
    Voxel(Voxel, MipRule),
    /// Only hidden children are exposed; the caller resolves the material with
    /// [`HIDDEN_FALLBACK_ORDER`] and builds the voxel with [`fallback_voxel`].
    HiddenFallback(GroupTraits),
}

/// Light, coverage and water of a group, independent of its material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GroupTraits {
    pub light: Light,
    pub coverage: u8,
    pub water: bool,
}

/// Builds the voxel for a resolved hidden fallback, or hidden when nothing resolved.
#[must_use]
pub fn fallback_voxel(material: Option<MaterialId>, traits: GroupTraits) -> Voxel {
    material.map_or(Voxel::HIDDEN, |material| {
        Voxel::new(material, traits.light, traits.coverage, traits.water)
    })
}

/// Applies the policy to one group of eight children.
#[must_use]
pub fn mip_group(children: &[MipChild; 8]) -> GroupDecision {
    let traits = group_traits(children);
    let build = |material, rule| {
        GroupDecision::Voxel(
            Voxel::new(material, traits.light, traits.coverage, traits.water),
            rule,
        )
    };
    let indexed = || Octant::ALL.into_iter().zip(children.iter());
    if children.iter().all(|child| child.voxel.is_air()) {
        return build(MaterialId::AIR, MipRule::AllAir);
    }
    let top = || indexed().filter(|(octant, child)| octant.y() == 1 && !child.voxel.is_air());
    let is_water = |child: &MipChild| child.class == BlockClass::Water;
    let top_water = top().filter(|(_, child)| is_water(child)).count();
    if top_water * 2 > top().count() {
        let material = majority(
            top()
                .filter(|(_, child)| is_water(child))
                .map(|(octant, child)| (child.voxel.material(), octant.y())),
        )
        .expect("the top layer holds water");
        return build(material, MipRule::WaterSurface);
    }
    let solid = |child: &MipChild| !child.voxel.is_air() && !child.voxel.is_hidden();
    let exposed = majority(
        indexed()
            .filter(|(_, child)| solid(child) && child.exposed)
            .map(|(octant, child)| (child.voxel.material(), octant.y())),
    );
    if let Some(material) = exposed {
        return build(material, MipRule::ExposedMajority);
    }
    if children
        .iter()
        .any(|child| child.voxel.is_hidden() && child.exposed)
    {
        return GroupDecision::HiddenFallback(traits);
    }
    let enclosed = majority(
        indexed()
            .filter(|(_, child)| solid(child))
            .map(|(octant, child)| (child.voxel.material(), octant.y())),
    );
    match enclosed {
        Some(material) => build(material, MipRule::EnclosedMajority),
        None => GroupDecision::Voxel(Voxel::HIDDEN, MipRule::AllHidden),
    }
}

/// Light, coverage and water flag shared by every rule.
fn group_traits(children: &[MipChild; 8]) -> GroupTraits {
    let (mut sky, mut block, mut lit, mut coverage) = (0, 0, 0, 0);
    for child in children {
        coverage += u32::from(child.voxel.coverage());
        if child.class.is_see_through() {
            sky += u32::from(child.voxel.light().sky());
            block += u32::from(child.voxel.light().block());
            lit += 1;
        }
    }
    GroupTraits {
        light: Light::new(
            LIGHT_MEAN_ROUNDING.mean(sky, lit) as u8,
            LIGHT_MEAN_ROUNDING.mean(block, lit) as u8,
        ),
        coverage: COVERAGE_MEAN_ROUNDING.mean(coverage, 8) as u8,
        water: children.iter().any(|child| child.voxel.has_water()),
    }
}

/// Most candidates [`majority`] tallies: one 4×4×4 fine group, the largest any rule reads.
pub(crate) const MAX_MAJORITY_CANDIDATES: usize = 64;

/// The most frequent material; ties go to the higher `y`, then the lower material id.
/// Tallies on the stack; panics past [`MAX_MAJORITY_CANDIDATES`] distinct materials.
#[must_use]
pub(crate) fn majority(
    candidates: impl IntoIterator<Item = (MaterialId, usize)>,
) -> Option<MaterialId> {
    let mut tallies = [(MaterialId::AIR, 0_usize, 0_usize); MAX_MAJORITY_CANDIDATES];
    let mut len = 0;
    for (material, y) in candidates {
        match tallies[..len]
            .iter_mut()
            .find(|(entry, _, _)| *entry == material)
        {
            Some((_, count, top)) => {
                *count += 1;
                *top = (*top).max(y);
            }
            None => {
                tallies[len] = (material, 1, y);
                len += 1;
            }
        }
    }
    tallies[..len]
        .iter()
        .max_by(|a, b| (a.1, a.2).cmp(&(b.1, b.2)).then(b.0.cmp(&a.0)))
        .map(|&(material, _, _)| material)
}
