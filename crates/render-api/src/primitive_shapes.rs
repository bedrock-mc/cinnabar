//! Network-driven debug drawing contracts, independent of the rendering backend.

use std::sync::Arc;

/// The dimension selector that displays a shape in every dimension.
pub const PRIMITIVE_ALL_DIMENSIONS: i32 = 3;
/// Circle and axial sphere resolution when no segment payload was supplied.
pub const PRIMITIVE_DEFAULT_SEGMENTS: u8 = 20;
/// Arrow head resolution when no segment payload was supplied.
pub const PRIMITIVE_DEFAULT_ARROW_SEGMENTS: u8 = 4;
/// Arrow geometry enforces a closed head with at least three sides.
pub const PRIMITIVE_MIN_ARROW_SEGMENTS: u8 = 3;
/// Largest arrow head resolution accepted by the renderer.
pub const PRIMITIVE_MAX_ARROW_SEGMENTS: u8 = 128;

/// Arrows shorter than this squared endpoint distance produce no geometry.
pub const PRIMITIVE_ARROW_MIN_LENGTH_SQUARED: f32 = 0.001;

/// Supported server-driven primitive geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PrimitiveShapeKind {
    Line,
    Box,
    Sphere,
    Circle,
    Text,
    Arrow,
}

/// Ordered changes from one packet, with a count of entries rejected during normalization.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PrimitiveShapesEvent {
    pub changes: Vec<PrimitiveShapeChange>,
    pub skipped_entries: u32,
}

/// A missing shape type removes the network id; all other entries patch or create it.
#[derive(Debug, Clone, PartialEq)]
pub enum PrimitiveShapeChange {
    Remove { network_id: u64 },
    Upsert(PrimitiveShapeUpdate),
}

/// Omitted fields retain their previous values, or the creation defaults for a new id.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimitiveShapeUpdate {
    pub network_id: u64,
    pub kind: PrimitiveShapeKind,
    pub location: Option<[f32; 3]>,
    pub rotation: Option<[f32; 3]>,
    pub scale: Option<f32>,
    pub color: Option<[f32; 4]>,
    /// Zero disables expiry; omission preserves the existing remaining lifetime.
    pub total_time_left: Option<f32>,
    /// A negative value removes the distance limit; omission preserves it.
    pub maximum_render_distance: Option<f32>,
    pub dimension: Option<i32>,
    /// The actor unique id; -1 detaches the shape, and omission preserves attachment.
    pub attached_actor: Option<i64>,
    pub data: PrimitiveShapeData,
}

/// Type-specific patches; `None` preserves existing geometry settings.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum PrimitiveShapeData {
    #[default]
    None,
    Line {
        end: [f32; 3],
    },
    Box {
        bounds: [f32; 3],
    },
    Segments(u8),
    Arrow {
        end: Option<[f32; 3]>,
        head_length: Option<f32>,
        head_radius: Option<f32>,
        segments: Option<u8>,
    },
    Text(PrimitiveText),
}

/// Text rendering options replace the previous text payload together.
#[derive(Debug, Clone, PartialEq)]
pub struct PrimitiveText {
    pub text: Arc<str>,
    pub use_rotation: bool,
    pub background_color: Option<[f32; 4]>,
    pub line_gap_height: f32,
    pub depth_test: bool,
    pub show_backface: bool,
    pub show_text_backface: bool,
}
