//! Sparse explicit size writes accepted from actor metadata.

use protocol::{ActorMetadata, ActorMetadataValue};
use protocol_ids::ActorDataKey;

/// One finite dimension value and the arrival that supplied it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DimensionWrite {
    pub sequence: u64,
    pub value: f32,
}

/// Latest supplied width and height, retaining each component's own arrival.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ExplicitDimensions {
    pub width: Option<DimensionWrite>,
    pub height: Option<DimensionWrite>,
}

impl ExplicitDimensions {
    /// Records a finite supplied dimension without replacing the absent component.
    pub fn record(&mut self, metadata: &ActorMetadata, sequence: u64) {
        let ActorMetadataValue::Float(value) = metadata.value else {
            return;
        };
        if !value.is_finite() {
            return;
        }
        let component = match metadata.key {
            key if key == ActorDataKey::Width.wire_value() => &mut self.width,
            key if key == ActorDataKey::Height.wire_value() => &mut self.height,
            _ => return,
        };
        *component = Some(DimensionWrite { sequence, value });
    }
}
