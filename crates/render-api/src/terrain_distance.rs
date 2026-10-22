//! Optional terrain distance policy shared by settings and streaming.

/// Keeps the complete height of every retained column.
pub const VERTICAL_DISTANCE_UNLIMITED: u32 = 0;

/// Largest optional vertical radius offered by the settings slider, in sections.
pub const MAX_VERTICAL_DISTANCE_CHUNKS: u32 = 64;
