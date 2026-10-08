//! World queries the particle simulation needs, implemented by the host.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fluid {
    #[default]
    None,
    Water,
    Lava,
}

/// Read-only world access for collision, lighting and fluid checks.
pub trait ParticleWorld {
    /// Appends collision boxes (`[min_x, min_y, min_z, max_x, max_y, max_z]`) overlapping `min..max`.
    fn solid_boxes(&self, min: [f32; 3], max: [f32; 3], out: &mut Vec<[f32; 6]>);

    /// `(block_light, sky_light)` in `0..=15` at a block cell; dark when unloaded.
    fn light(&self, block: [i32; 3]) -> (u8, u8);

    fn fluid(&self, block: [i32; 3]) -> Fluid;
}

/// A world with no blocks, used before a session exists and in tests.
#[derive(Clone, Copy, Debug, Default)]
pub struct EmptyWorld;

impl ParticleWorld for EmptyWorld {
    fn solid_boxes(&self, _min: [f32; 3], _max: [f32; 3], _out: &mut Vec<[f32; 6]>) {}

    fn light(&self, _block: [i32; 3]) -> (u8, u8) {
        (0, 15)
    }

    fn fluid(&self, _block: [i32; 3]) -> Fluid {
        Fluid::None
    }
}
