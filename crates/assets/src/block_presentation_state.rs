//! Immutable block-state facts used by station UI and ambient presentation.

use std::{collections::BTreeMap, sync::OnceLock};

use crate::{NetworkIdMode, RegistryRecord};

/// Portal orientation, including a state whose axis is absent or unrecognized.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PortalAxis {
    #[default]
    Unknown,
    X,
    Z,
}

/// Presentation facts decoded from one canonical block state at admission.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockPresentationState {
    pub crafter_triggered: Option<bool>,
    pub portal_axis: Option<PortalAxis>,
}

impl BlockPresentationState {
    /// Decodes supported facts while keeping missing or malformed values unknown.
    pub fn decode(identifier: &str, canonical: &str) -> Self {
        if identifier != "minecraft:crafter" && identifier != crate::NETHER_PORTAL_IDENTIFIER {
            return Self::default();
        }
        let states = serde_json::from_str::<serde_json::Value>(canonical).ok();
        let value = |name: &str| {
            let raw = states.as_ref()?.get(name)?;
            Some(raw.get("value").unwrap_or(raw))
        };
        if identifier == "minecraft:crafter" {
            Self {
                crafter_triggered: value("triggered_bit").and_then(|value| {
                    value
                        .as_bool()
                        .or_else(|| value.as_u64().map(|bit| bit != 0))
                }),
                portal_axis: None,
            }
        } else {
            Self {
                crafter_triggered: None,
                portal_axis: Some(
                    match value("portal_axis").and_then(|value| value.as_str()) {
                        Some("x") => PortalAxis::X,
                        Some("z") => PortalAxis::Z,
                        _ => PortalAxis::Unknown,
                    },
                ),
            }
        }
    }
}

/// Typed facts for one immutable registry generation and its two ID spaces.
#[derive(Debug, Default)]
pub struct BlockPresentationStates {
    sequential: BTreeMap<u32, BlockPresentationState>,
    hashed: BTreeMap<u32, BlockPresentationState>,
}

impl BlockPresentationStates {
    /// Builds a fresh generation without retaining source JSON or unrelated blocks.
    pub fn from_records(records: &[RegistryRecord]) -> Self {
        let mut result = Self::default();
        for record in records {
            if record.name.as_ref() != "minecraft:crafter"
                && record.name.as_ref() != crate::NETHER_PORTAL_IDENTIFIER
            {
                continue;
            }
            let state = BlockPresentationState::decode(&record.name, &record.canonical_state);
            result.sequential.insert(record.sequential_id, state);
            result.hashed.insert(record.network_hash, state);
        }
        result
    }

    /// Looks up admitted facts without parsing or allocating on the frame or tick path.
    pub fn get(&self, mode: NetworkIdMode, runtime: u32) -> BlockPresentationState {
        let records = match mode {
            NetworkIdMode::Sequential => &self.sequential,
            NetworkIdMode::Hashed => &self.hashed,
        };
        records.get(&runtime).copied().unwrap_or_default()
    }
}

/// Returns the typed facts for the build's immutable pinned registry generation.
pub fn pinned_block_presentation_states() -> &'static BlockPresentationStates {
    static STATES: OnceLock<BlockPresentationStates> = OnceLock::new();
    STATES.get_or_init(|| {
        let records = crate::read_registry_for_protocol(
            crate::pinned_block_registry_bytes(),
            crate::active_content_registry_protocol(),
        )
        .expect("the pinned block registry must be valid");
        BlockPresentationStates::from_records(&records)
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod test_allocations;
