//! World queries the particle simulation needs, implemented by the host.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fluid {
    #[default]
    None,
    Water,
    Lava,
}

/// Exact namespaced block identity, independent of fluid and collision classification.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockIdentity<'a>(pub &'a str);

/// Sorted authored identities used by a block-dependent lifetime condition.
#[derive(Default)]
pub struct BlockList(Box<[Box<str>]>);

impl BlockList {
    /// Compiles exact identifiers once and removes duplicate membership entries.
    pub(crate) fn new(mut names: Vec<Box<str>>) -> Self {
        names.sort_unstable();
        names.dedup();
        Self(names.into_boxed_slice())
    }

    /// Whether the condition has no authored members.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Tests exact membership without allocations or fluid-name heuristics.
    pub fn contains(&self, identity: BlockIdentity<'_>) -> bool {
        self.0
            .binary_search_by(|name| name.as_ref().cmp(identity.0))
            .is_ok()
    }
}

/// Read-only world access for collision, lighting and fluid checks.
pub trait ParticleWorld {
    /// Appends collision boxes (`[min_x, min_y, min_z, max_x, max_y, max_z]`) overlapping `min..max`.
    fn solid_boxes(&self, min: [f32; 3], max: [f32; 3], out: &mut Vec<[f32; 6]>);

    /// `(block_light, sky_light)` in `0..=15` at a block cell; dark when unloaded.
    fn light(&self, block: [i32; 3]) -> (u8, u8);

    fn fluid(&self, block: [i32; 3]) -> Fluid;

    /// Exact primary block identity at the cell, or `None` when unavailable.
    fn block_identity(&self, _block: [i32; 3]) -> Option<BlockIdentity<'_>> {
        None
    }
}

/// A world with no blocks, used before a session exists and in tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyWorld;

impl ParticleWorld for EmptyWorld {
    fn block_identity(&self, _block: [i32; 3]) -> Option<BlockIdentity<'_>> {
        Some(BlockIdentity("minecraft:air"))
    }

    fn solid_boxes(&self, _min: [f32; 3], _max: [f32; 3], _out: &mut Vec<[f32; 6]>) {}

    fn light(&self, _block: [i32; 3]) -> (u8, u8) {
        (0, 15)
    }

    fn fluid(&self, _block: [i32; 3]) -> Fluid {
        Fluid::None
    }
}
