//! Sparse explicit size writes accepted from actor metadata.

use protocol::{ActorMetadata, ActorMetadataValue};
use protocol_ids::ActorDataKey;

/// One finite dimension value and the arrival that supplied it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DimensionWrite {
    pub sequence: u64,
    pub value: f32,
}

/// Sparse dimensions for one accepted actor spawn, retaining each component's arrival.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ExplicitDimensions {
    /// Accepted actor spawn revision; absent when no actor has been observed.
    pub spawn_revision: Option<u64>,
    pub width: Option<DimensionWrite>,
    pub height: Option<DimensionWrite>,
}

impl ExplicitDimensions {
    /// Starts an empty component record for one accepted actor spawn.
    pub const fn for_spawn(spawn_revision: u64) -> Self {
        Self {
            spawn_revision: Some(spawn_revision),
            width: None,
            height: None,
        }
    }

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
