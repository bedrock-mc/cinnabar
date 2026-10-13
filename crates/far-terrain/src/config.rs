//! Every far-terrain bound and default; consumers read these instead of repeating values.

/// Compressed column summaries kept in memory before the least recently used are dropped.
pub const DEFAULT_MEMORY_BUDGET_BYTES: usize = 32 << 20;

/// Disk cache cap for one server.
pub const DEFAULT_SERVER_DISK_CAP_BYTES: u64 = 64 << 20;

/// Disk cache cap across every server.
pub const DEFAULT_GLOBAL_DISK_CAP_BYTES: u64 = 512 << 20;

/// A region's waiting changes are written once this many columns wait.
pub const REGION_FLUSH_COLUMNS: usize = 128;

/// A region's waiting changes are written once the oldest is this old.
pub const REGION_FLUSH_AFTER: std::time::Duration = std::time::Duration::from_secs(5);

/// Non-empty columns compared against a cached world before reusing it.
pub const WORLD_PROBE_COLUMNS: usize = 4;

/// Share of a probe column's cells that must match the cache for the world to be the same.
pub const WORLD_PROBE_SIMILARITY: f32 = 0.9;

/// Most recent epochs of one world identity compared against the probes.
pub const WORLD_PROBE_EPOCHS: usize = 3;

/// Columns held while the world's cache namespace is being resolved.
pub const MAX_UNRESOLVED_COLUMNS: usize = 4096;

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
    pub server_disk_cap_bytes: u64,
    pub global_disk_cap_bytes: u64,
}

impl Default for FarTerrainConfig {
    fn default() -> Self {
        Self {
            memory_budget_bytes: DEFAULT_MEMORY_BUDGET_BYTES,
            server_disk_cap_bytes: DEFAULT_SERVER_DISK_CAP_BYTES,
            global_disk_cap_bytes: DEFAULT_GLOBAL_DISK_CAP_BYTES,
        }
    }
}
