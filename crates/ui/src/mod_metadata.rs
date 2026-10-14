//! What a personal mod component says about itself, for its Mods library card.

use std::sync::Arc;

/// Bounded single-line text from the component's standard metadata, plus a validated icon.
/// Absent fields fall back to the file name or a placeholder.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModMetadata {
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    /// The standard section's freeform contact text.
    pub authors: Option<String>,
    pub licenses: Option<String>,
    /// Encoded PNG source that decoded within the icon section's limits.
    pub icon: Option<Arc<[u8]>>,
}
