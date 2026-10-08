//! Admission shared by bamboo geometry and position-dependent interaction shapes.
use crate::{BlockFlags, ContributorRole, RegistryRecord};
use serde::Deserialize;

/// Registered identifier of the built-in stalk block.
pub const BLOCK_NAME: &str = "minecraft:bamboo";

/// Leaf geometry selected by an admitted stalk state.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LeafSize {
    None,
    Small,
    Large,
}

/// Built-in state whose model and shapes receive the bamboo column transform.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BambooState {
    pub thick: bool,
    pub leaves: LeafSize,
}

/// Canonical registry atoms preserve their NBT type as well as their value.
#[derive(Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
enum Atom {
    Byte(u8),
    String(String),
}

/// Only the three built-in properties admit the default stalk component.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    age_bit: Atom,
    bamboo_stalk_thickness: Atom,
    bamboo_leaf_size: Atom,
}

impl BambooState {
    /// Admits primary, non-air stalk records with the complete typed vanilla state.
    /// Malformed records and other block names do not inherit displacement.
    pub fn from_record(record: &RegistryRecord) -> Option<Self> {
        if record.name.as_ref() != BLOCK_NAME
            || record.contributor_role != ContributorRole::Primary
            || record.flags.contains(BlockFlags::AIR)
        {
            return None;
        }
        let state: State = serde_json::from_str(&record.canonical_state).ok()?;
        if !matches!(state.age_bit, Atom::Byte(0 | 1)) {
            return None;
        }
        let Atom::String(thickness) = state.bamboo_stalk_thickness else {
            return None;
        };
        let thick = match thickness.as_str() {
            "thin" => false,
            "thick" => true,
            _ => return None,
        };
        let Atom::String(leaves) = state.bamboo_leaf_size else {
            return None;
        };
        let leaves = match leaves.as_str() {
            "no_leaves" => LeafSize::None,
            "small_leaves" => LeafSize::Small,
            "large_leaves" => LeafSize::Large,
            _ => return None,
        };
        Some(Self { thick, leaves })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads the public pinned registry using its manifest's protocol authority.
    fn records() -> Box<[RegistryRecord]> {
        let target: serde_json::Value =
            serde_json::from_slice(include_bytes!("../../../assets/bedrock-target.json")).unwrap();
        crate::read_registry_for_protocol(
            include_bytes!("../data/block-registry-v2193.bin"),
            target["wire_protocol"].as_u64().unwrap() as u32,
        )
        .unwrap()
    }

    #[test]
    fn admission_covers_every_stalk_state_and_rejects_other_registered_blocks() {
        let records = records();
        let mut seen = [[false; 3]; 2];
        for record in &records {
            let state = BambooState::from_record(record);
            assert_eq!(state.is_some(), record.name.as_ref() == BLOCK_NAME);
            if let Some(state) = state {
                let leaf = match state.leaves {
                    LeafSize::None => 0,
                    LeafSize::Small => 1,
                    LeafSize::Large => 2,
                };
                seen[usize::from(state.thick)][leaf] = true;
            }
        }
        assert_eq!(seen, [[true; 3]; 2]);
    }

    #[test]
    fn malformed_or_extended_states_do_not_acquire_the_default_component() {
        let mut record = records()
            .into_vec()
            .into_iter()
            .find(|record| record.name.as_ref() == BLOCK_NAME)
            .unwrap();
        let original: serde_json::Value = serde_json::from_str(&record.canonical_state).unwrap();
        for (property, atom) in [
            ("age_bit", serde_json::json!({"type":"int","value":0})),
            ("age_bit", serde_json::json!({"type":"byte","value":2})),
            (
                "bamboo_leaf_size",
                serde_json::json!({"type":"string","value":"unknown"}),
            ),
            (
                "bamboo_stalk_thickness",
                serde_json::json!({"type":"string","value":"thin","extra":0}),
            ),
            (
                "random_offset",
                serde_json::json!({"type":"byte","value":0}),
            ),
        ] {
            let mut state = original.clone();
            state[property] = atom;
            record.canonical_state = serde_json::to_string(&state).unwrap().into();
            assert_eq!(BambooState::from_record(&record), None, "{property}");
        }
    }
}
