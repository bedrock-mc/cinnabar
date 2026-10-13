//! Every far-terrain bound and default; consumers read these instead of repeating values.

/// Compressed column summaries kept in memory before the least recently used are dropped.
pub const DEFAULT_MEMORY_BUDGET_BYTES: usize = 32 << 20;

/// Water deeper than this many blocks is summarised as open water without a floor.
pub const MAX_WATER_SCAN_DEPTH: u8 = 32;

/// Opacity of one block of water over its floor; each further block absorbs a share more.
pub const WATER_SURFACE_OPACITY: f32 = 0.65;

/// Fraction of the remaining floor colour each further block of water lets through.
pub const WATER_DEPTH_TRANSMITTANCE: f32 = 0.85;

/// Colour of a surface block the active pack cannot describe.
pub const UNKNOWN_SURFACE_RGB: [u8; 3] = [128, 128, 128];

/// Runtime bounds for one far-terrain owner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarTerrainConfig {
    pub memory_budget_bytes: usize,
}

impl Default for FarTerrainConfig {
    fn default() -> Self {
        Self {
            memory_budget_bytes: DEFAULT_MEMORY_BUDGET_BYTES,
        }
    }
}
