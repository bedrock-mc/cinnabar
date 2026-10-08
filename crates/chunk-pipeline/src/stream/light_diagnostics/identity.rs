//! Bounded block identities for lighting observations.

use std::{collections::BTreeMap, fmt::Write, sync::OnceLock};

use assets::{NetworkIdMode, RegistryRecord};
use client_world::ingestion::{CustomBlocks, CustomStateValue, DecodeIds};

use crate::stream::WorldStream;

const MAX_IDENTITY_TEXT: usize = 512;
/// Caps diagnostic work independently of the server's admitted custom palette.
const MAX_CUSTOM_HASH_IDENTITIES: usize = 65_536;

#[derive(Default)]
pub(super) struct BlockIdentities {
    custom: CustomBlocks,
    hashes: BTreeMap<u32, (usize, usize)>,
    custom_identity_index_truncated: bool,
}

impl WorldStream {
    /// Retains server block definitions so lighting logs can name custom states.
    pub fn set_light_diagnostic_custom_blocks(&mut self, custom: CustomBlocks) {
        let hashed = self.decode_ids(self.current_dimension()).mode == NetworkIdMode::Hashed;
        let mut hashes = BTreeMap::new();
        let mut remaining = MAX_CUSTOM_HASH_IDENTITIES;
        let mut custom_identity_index_truncated = false;
        for (block_index, block) in custom.blocks.iter().filter(|_| hashed).enumerate() {
            let generated_count = block.visual.state_axes.iter().fold(1_usize, |total, axis| {
                total.saturating_mul(axis.values.len())
            });
            if (block.state_count as usize).max(generated_count) > remaining {
                custom_identity_index_truncated = true;
                continue;
            }
            let states = block.hashed_states();
            remaining -= states.len();
            hashes.extend(
                states
                    .into_iter()
                    .enumerate()
                    .map(|(state_index, state)| (state.hash, (block_index, state_index))),
            );
        }
        self.light_diagnostics.identities = BlockIdentities {
            custom,
            hashes,
            custom_identity_index_truncated,
        };
    }
}

impl BlockIdentities {
    /// Names the resolved state and recovers its wire identity after palette remapping.
    pub(super) fn describe(&self, ids: &DecodeIds, runtime_id: u32) -> String {
        let wire_id = match ids.mode {
            NetworkIdMode::Sequential => ids.remap.to_wire(runtime_id),
            NetworkIdMode::Hashed => runtime_id,
        };
        let sequential_id = match ids.mode {
            NetworkIdMode::Sequential => Some(runtime_id),
            NetworkIdMode::Hashed => ids.assets.sequential_id_for_hash(runtime_id),
        };
        let identity = self
            .custom_state(ids, runtime_id)
            .or_else(|| pinned_identity(ids.assets.provenance(), sequential_id?));
        format!(
            "{} runtime_id={runtime_id} sequential_id={sequential_id:?} resolved_wire_id={wire_id} id_mode={:?} custom_identity_index_truncated={}",
            identity.unwrap_or_else(|| "name=unavailable states=unavailable".into()),
            ids.mode,
            self.custom_identity_index_truncated
        )
    }

    /// Resolves one custom state without formatting the rest of the server palette.
    fn custom_state(&self, ids: &DecodeIds, runtime_id: u32) -> Option<String> {
        let (block_index, offset) = match ids.mode {
            NetworkIdMode::Hashed => *self.hashes.get(&runtime_id)?,
            NetworkIdMode::Sequential => {
                if !ids.custom_blocks.contains(&runtime_id) {
                    return None;
                }
                let mut offset = runtime_id - ids.custom_blocks.start;
                let block_index = self.custom.blocks.iter().position(|block| {
                    if offset < block.state_count {
                        true
                    } else {
                        offset -= block.state_count;
                        false
                    }
                })?;
                (block_index, offset as usize)
            }
        };
        let block = self.custom.blocks.get(block_index)?;
        let Some(values) = block.state_values(offset as u32) else {
            return Some(format!(
                "name={} states=unavailable(sequential_state_offset={offset})",
                bounded_text(&block.name, MAX_IDENTITY_TEXT / 2)
            ));
        };
        let mut states = String::new();
        for (axis, value) in block.visual.state_axes.iter().zip(values.iter()) {
            if states.len() >= MAX_IDENTITY_TEXT / 2 {
                states.push('…');
                break;
            }
            if !states.is_empty() {
                states.push(',');
            }
            let value = match value {
                CustomStateValue::String(value) => bounded_text(value, MAX_IDENTITY_TEXT / 4),
                CustomStateValue::Int(value) => value.to_string(),
                CustomStateValue::Bool(value) => value.to_string(),
            };
            let _ = write!(
                states,
                "{}={value}",
                bounded_text(&axis.name, MAX_IDENTITY_TEXT / 4)
            );
        }
        Some(format!(
            "name={} states=[{}]",
            bounded_text(&block.name, MAX_IDENTITY_TEXT / 2),
            bounded_text(&states, MAX_IDENTITY_TEXT / 2)
        ))
    }
}

/// Names a vanilla state only when its carrier uses the matching pinned palette.
fn pinned_identity(provenance: &assets::BlobProvenance, sequential_id: u32) -> Option<String> {
    if provenance.block_registry_sha256 != assets::pinned_world_provenance().block_registry_sha256 {
        return None;
    }
    let state = pinned_states().get(sequential_id as usize)?;
    (state.sequential_id == sequential_id).then(|| {
        format!(
            "name={} states={}",
            bounded_text(&state.name, MAX_IDENTITY_TEXT / 2),
            bounded_text(&state.canonical_state, MAX_IDENTITY_TEXT / 2)
        )
    })
}

/// Loads identities once from the same pinned registry used to validate the carrier.
fn pinned_states() -> &'static [RegistryRecord] {
    static STATES: OnceLock<Box<[RegistryRecord]>> = OnceLock::new();
    STATES.get_or_init(|| {
        assets::read_registry_for_protocol(
            assets::pinned_block_registry_bytes(),
            assets::active_content_registry_protocol(),
        )
        .unwrap_or_default()
    })
}

/// Keeps server-provided text on one line and caps its UTF-8 output size.
fn bounded_text(value: &str, limit: usize) -> String {
    let mut result = String::new();
    for character in value.chars() {
        let character = if character.is_control() {
            ' '
        } else {
            character
        };
        if result.len() + character.len_utf8() > limit.saturating_sub('…'.len_utf8()) {
            result.push('…');
            return result;
        }
        result.push(character);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::{
        CustomBlock, CustomBlockVisuals, CustomSelection, CustomStateAxis, WorldBootstrap,
    };
    use std::sync::Arc;

    /// Creates a diagnostic world whose custom palette has one two-valued state axis.
    fn custom_stream(hashed: bool) -> WorldStream {
        let mut stream = WorldStream::new(WorldBootstrap {
            dimension: 0,
            local_player_runtime_id: 1,
            local_player_unique_id: 1,
            player_position: [0.0; 3],
            world_spawn_position: [0; 3],
            air_network_id: 0,
            block_network_ids_are_hashes: hashed,
        });
        stream.set_custom_block_ids(10..12);
        stream.set_sequential_id_remap(assets::SequentialIdRemap::new([(3, 2, 10)]));
        stream.set_light_diagnostic_custom_blocks(CustomBlocks {
            blocks: Arc::from([CustomBlock {
                state_physics: Default::default(),
                name: Arc::from("test:roof"),
                tags: Default::default(),
                state_count: 2,
                collides: true,
                collision_boxes: None,
                selection: CustomSelection::Default,
                visual: Arc::new(CustomBlockVisuals {
                    state_axes: Box::new([CustomStateAxis {
                        name: Arc::from("test:open"),
                        values: Box::new([
                            CustomStateValue::Bool(false),
                            CustomStateValue::Bool(true),
                        ]),
                    }]),
                    ..Default::default()
                }),
            }]),
            vanilla_blocks: Default::default(),
            skipped: 0,
        });
        stream
    }

    #[test]
    fn identities_use_the_pinned_palette_and_bound_untrusted_text() {
        assert!(
            pinned_states()
                .iter()
                .any(|state| state.name.as_ref() == "minecraft:air")
        );
        assert_eq!(bounded_text("stone\nroof", 32), "stone roof");
        let text = bounded_text(&"火".repeat(MAX_IDENTITY_TEXT), MAX_IDENTITY_TEXT);
        assert!(text.len() <= MAX_IDENTITY_TEXT);
        assert!(text.ends_with('…'));
    }

    #[test]
    fn vanilla_identity_formats_pinned_state_and_rejects_mismatched_carrier() {
        let state = pinned_states()
            .iter()
            .find(|state| state.name.as_ref() == "minecraft:oak_stairs")
            .expect("pinned oak stairs state");
        let text = pinned_identity(assets::pinned_world_provenance(), state.sequential_id)
            .expect("matching carrier palette");
        assert_eq!(
            text,
            format!("name=minecraft:oak_stairs states={}", state.canonical_state)
        );
        assert!(!state.canonical_state.is_empty());
        assert!(pinned_identity(&assets::BlobProvenance::ZEROED, state.sequential_id).is_none());
    }

    #[test]
    fn custom_identity_reports_state_and_recovered_sequential_wire_id() {
        let stream = custom_stream(false);
        let text = stream
            .light_diagnostics
            .identities
            .describe(&stream.decode_ids(0), 11);
        assert!(
            text.contains("name=test:roof states=[test:open=true]"),
            "{text}"
        );
        assert!(text.contains("runtime_id=11"), "{text}");
        assert!(text.contains("resolved_wire_id=4"), "{text}");
        let unknown = stream
            .light_diagnostics
            .identities
            .describe(&stream.decode_ids(0), 2);
        assert!(
            unknown.contains("name=unavailable states=unavailable"),
            "{unknown}"
        );
    }

    #[test]
    fn hashed_custom_identity_reports_named_state_and_unchanged_wire_id() {
        let stream = custom_stream(true);
        let identities = &stream.light_diagnostics.identities;
        let hash = identities.custom.blocks[0].hashed_states()[1].hash;
        let text = identities.describe(&stream.decode_ids(0), hash);
        assert!(
            text.contains("name=test:roof states=[test:open=true]"),
            "{text}"
        );
        assert!(text.contains(&format!("resolved_wire_id={hash}")), "{text}");
    }

    #[test]
    fn oversized_custom_hash_palette_is_skipped_before_generation_and_reported() {
        let mut stream = custom_stream(true);
        let mut custom = stream.light_diagnostics.identities.custom.clone();
        let block = &mut Arc::make_mut(&mut custom.blocks)[0];
        let hash = block.hashed_states()[0].hash;
        block.state_count = MAX_CUSTOM_HASH_IDENTITIES as u32 + 1;
        stream.set_light_diagnostic_custom_blocks(custom);
        let identities = &stream.light_diagnostics.identities;
        assert!(identities.hashes.is_empty());
        let text = identities.describe(&stream.decode_ids(0), hash);
        assert!(
            text.contains("name=unavailable states=unavailable"),
            "{text}"
        );
        assert!(
            text.contains("custom_identity_index_truncated=true"),
            "{text}"
        );
    }

    // Both id modes name every axis in palette order; axes that miss states stay unavailable.
    #[test]
    fn multiple_custom_axes_are_exact_for_hashes_and_sequential_ids() {
        let mut hashed = custom_stream(true);
        let mut custom = hashed.light_diagnostics.identities.custom.clone();
        let block = &mut Arc::make_mut(&mut custom.blocks)[0];
        block.state_count = 4;
        let visual = Arc::make_mut(&mut block.visual);
        let mut axes = visual.state_axes.to_vec();
        axes.push(CustomStateAxis {
            name: Arc::from("test:side"),
            values: Box::new([CustomStateValue::Int(1), CustomStateValue::Int(2)]),
        });
        visual.state_axes = axes.into_boxed_slice();
        let states = block.hashed_states();
        hashed.set_light_diagnostic_custom_blocks(custom.clone());
        let text = hashed
            .light_diagnostics
            .identities
            .describe(&hashed.decode_ids(0), states[2].hash);
        assert!(
            text.contains("states=[test:open=false,test:side=2]"),
            "{text}"
        );

        let mut sequential = custom_stream(false);
        sequential.set_light_diagnostic_custom_blocks(custom.clone());
        let text = sequential
            .light_diagnostics
            .identities
            .describe(&sequential.decode_ids(0), 11);
        assert!(
            text.contains("name=test:roof states=[test:open=true,test:side=1]"),
            "{text}"
        );

        Arc::make_mut(&mut custom.blocks)[0].state_count = 8;
        sequential.set_light_diagnostic_custom_blocks(custom);
        let text = sequential
            .light_diagnostics
            .identities
            .describe(&sequential.decode_ids(0), 11);
        assert!(
            text.contains("name=test:roof states=unavailable(sequential_state_offset=1)"),
            "{text}"
        );
    }
}
