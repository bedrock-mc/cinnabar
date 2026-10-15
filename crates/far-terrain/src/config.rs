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

/// Cached columns compared with their live summaries each visit after the namespace resolves,
/// catching a different map whose probe area matched, such as another lobby instance.
pub const WORLD_VERIFY_COLUMNS: u32 = 64;

/// Of the cached columns compared, at least this many, and at least a quarter, differing from
/// what arrives live means the cache holds another map.
pub const WORLD_DIVERGED_COLUMNS: u32 = 4;

/// Most recent epochs of one world identity compared against the probes.
pub const WORLD_PROBE_EPOCHS: usize = 3;

/// Columns held while the world's cache namespace is being resolved.
pub const MAX_UNRESOLVED_COLUMNS: usize = 4096;

/// A world that has not shown enough non-empty columns to probe by now resolves with the
/// probes it has, so sparse worlds are still persisted.
pub const WORLD_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// Cells along one side of every far-terrain tile, at any level.
pub const TILE_CELLS: usize = 64;

/// Detail levels; level `l` has cells `2^l` blocks wide (1 to 16 blocks).
pub const LEVEL_COUNT: u8 = 5;

/// A tile splits into four finer ones while it is closer to the near-terrain edge than this
/// many of its own widths.
pub const LEVEL_SPLIT_DISTANCE: f32 = 1.0;

/// Finest tiles are kept built this far either side of the near-terrain edge, so a column
/// changing drawer already has its far tile.
pub const EDGE_MARGIN_BLOCKS: f32 = 4.0 * 16.0;

/// Blocks tile-edge skirts reach below a tile's lowest surface, hiding cracks between levels.
pub const SKIRT_DEPTH_BLOCKS: i16 = 16;

/// A tile whose columns changed is rebuilt at most this often.
pub const TILE_REBUILD_INTERVAL: std::time::Duration = std::time::Duration::from_millis(250);

/// Tile meshes built concurrently on worker threads.
pub const MAX_TILE_BUILDS: usize = 4;

/// Columns gathered for new tile builds per update, bounding the owner thread's share.
pub const TILE_BUILD_COLUMNS_PER_UPDATE: usize = 1024;

/// Encoded bytes retained for tile builds per update.
pub const TILE_BUILD_BYTES_PER_UPDATE: usize = 1 << 20;

/// Completed summary columns applied per frame, including stale results.
pub const SUMMARY_COLUMNS_PER_UPDATE: usize = 256;

/// Encoded summary bytes applied per frame. One oversized column may make progress alone.
pub const SUMMARY_BYTES_PER_UPDATE: usize = 256 << 10;

/// Disk events started per update, including empty and obsolete events.
pub const CACHE_EVENTS_PER_UPDATE: usize = 32;

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
