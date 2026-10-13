//! Far-terrain Video setting values, shared by settings, both menus and far terrain itself.

/// Far terrain is on unless the player turns it off.
pub const DEFAULT_ENABLED: bool = true;

/// Default far-terrain reach beyond the near-terrain edge, in chunks.
pub const DEFAULT_DISTANCE_CHUNKS: u32 = 64;

/// The far-terrain reach the Video setting offers, in chunks.
pub const MIN_DISTANCE_CHUNKS: u32 = 16;
pub const MAX_DISTANCE_CHUNKS: u32 = 128;
pub const DISTANCE_STEP_CHUNKS: u32 = 16;
