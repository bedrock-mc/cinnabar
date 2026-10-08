//! StartGame pack facts, validation, and generation ownership.

use resource_pack::PackAdmission;
use std::{collections::HashSet, sync::Arc};

/// Session facts needed to compile visuals again without another StartGame.
#[derive(Clone, Debug, Default)]
pub struct PackInputs {
    pub blocks: protocol::CustomBlocks,
    pub icons: Vec<(Arc<str>, Arc<str>)>,
    pub block_items: Vec<(Arc<str>, Arc<str>)>,
    pub hashed: bool,
}

/// A server-required pack the client could not apply; vanilla refuses such a join.
#[derive(Debug, thiserror::Error)]
#[error("required resource pack could not be applied ({rejected} of the stack rejected)")]
pub struct RequiredPackRejected {
    rejected: usize,
}

/// Validated transport pack data, before presentation compiles its subscriber payloads.
#[derive(Clone, Debug)]
pub struct PackPreparation {
    pub inputs: Arc<PackInputs>,
    pub admission: PackAdmission,
    required: bool,
}

impl PackPreparation {
    /// Compiles subscribers before required-pack rejection, then completes accepted payloads.
    /// `None` when the compile was cancelled.
    pub fn prepare_application<P>(
        &self,
        compile: impl FnOnce(&Self) -> Option<P>,
        complete: impl FnOnce(&mut P),
    ) -> Option<Result<P, RequiredPackRejected>> {
        let mut application = compile(self)?;
        if let Err(rejected) = required_packs_applied(self.required, &self.admission) {
            return Some(Err(rejected));
        }
        complete(&mut application);
        Some(Ok(application))
    }

    /// Reports whether the session must bracket its pump with pack-application reports.
    pub fn has_applied_packs(&self) -> bool {
        matches!(&self.admission, PackAdmission::Validated(stack) if !stack.packs().is_empty())
    }
}

/// Reads StartGame pack facts and validates the one-shot login handoff.
pub fn prepare_session_packs(
    handoff: protocol::ResourcePackHandoff,
    game_data: &protocol::GameData,
) -> PackPreparation {
    let mut blocks = protocol::CustomBlocks::from_game_data(game_data);
    crate::block_physics::resolve(&mut blocks);
    let icons = protocol::item_icon_keys(game_data);
    let block_items = custom_block_items(game_data, &blocks);
    let hashed = game_data.start_game.block_network_ids_are_hashes;
    tracing::info!(
        block_network_ids_are_hashes = hashed,
        block_property_count = game_data.start_game.block_properties.len(),
        custom_block_count = blocks.blocks.len(),
        custom_state_count = blocks.total_states(),
        skipped_block_definitions = blocks.skipped,
        "START_GAME_BLOCK_IDS"
    );
    let required = handoff.required();
    let stack = resource_pack::validate_handoff(handoff);
    let admission = if stack.packs().is_empty() && stack.rejections().is_empty() {
        PackAdmission::None
    } else {
        PackAdmission::Validated(stack)
    };
    PackPreparation {
        inputs: Arc::new(PackInputs {
            blocks,
            icons: icons.to_vec(),
            block_items: block_items.to_vec(),
            hashed,
        }),
        admission,
        required,
    }
}

/// Optional packs that fail validation are dropped; a required one ends the join.
pub fn required_packs_applied(
    required: bool,
    admission: &PackAdmission,
) -> Result<(), RequiredPackRejected> {
    match admission {
        PackAdmission::Validated(stack) if required && !stack.rejections().is_empty() => {
            Err(RequiredPackRejected {
                rejected: stack.rejections().len(),
            })
        }
        _ => Ok(()),
    }
}

/// Pairs each registry item that draws as a custom block with that block: the block's own
/// item (a `BlockItem`, which vanilla always renders as its block), or a `block_placer` item
/// declaring no icon of its own.
pub fn custom_block_items(
    game_data: &protocol::GameData,
    blocks: &protocol::CustomBlocks,
) -> Box<[(Arc<str>, Arc<str>)]> {
    let names = blocks
        .blocks
        .iter()
        .map(|block| Arc::clone(&block.name))
        .collect::<HashSet<_>>();
    if names.is_empty() {
        return Box::new([]);
    }
    let components = protocol::item_components(game_data);
    let mut pairs = game_data
        .item_registry
        .item_data
        .iter()
        .filter_map(|item| names.get(item.item_name.as_str()).cloned())
        .map(|block| (Arc::clone(&block), block))
        .collect::<Vec<_>>();
    for (identifier, facts) in components.iter() {
        if facts.icon.is_some() || names.contains(identifier) {
            continue;
        }
        if let Some(block) = facts
            .block_placer
            .as_deref()
            .and_then(|block| names.get(block))
        {
            pairs.push((Arc::clone(identifier), Arc::clone(block)));
        }
    }
    pairs.into_boxed_slice()
}

pub type StackFingerprint = Vec<(String, String, String, [u8; 32])>;

/// Identity of each pack as (uuid, version, subpack, content hash), in stack order.
pub fn stack_fingerprint(stack: &resource_pack::ValidatedPackStack) -> StackFingerprint {
    use sha2::{Digest, Sha256};
    stack
        .packs()
        .iter()
        .map(|pack| {
            (
                pack.pack_id().to_string(),
                pack.version().to_owned(),
                pack.sub_pack_name().to_owned(),
                Sha256::digest(&*pack.archive_bytes()).into(),
            )
        })
        .collect()
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BootstrapGenerationDisposition {
    Expected,
    Stale,
    Unexpected,
}

/// Classifies a bootstrap against the UI intent and the committed world generation.
pub const fn classify_bootstrap_generation(
    ui_generation: u64,
    world_generation: u64,
    incoming_generation: u64,
) -> BootstrapGenerationDisposition {
    let directly_next = ui_generation == world_generation
        && matches!(
            world_generation.checked_add(1),
            Some(expected) if expected == incoming_generation
        );
    let pending_ui_generation =
        incoming_generation == ui_generation && incoming_generation > world_generation;
    if directly_next || pending_ui_generation {
        BootstrapGenerationDisposition::Expected
    } else if incoming_generation <= world_generation || incoming_generation < ui_generation {
        BootstrapGenerationDisposition::Stale
    } else {
        BootstrapGenerationDisposition::Unexpected
    }
}

/// Generation-bound admission for the current session's optional pack stack.
/// This owns validated bytes independently of optional language application.
#[derive(Debug)]
pub struct ResourcePackAdmissionState {
    generation: u64,
    admission: PackAdmission,
}

impl Default for ResourcePackAdmissionState {
    fn default() -> Self {
        Self {
            generation: 0,
            admission: PackAdmission::None,
        }
    }
}

impl ResourcePackAdmissionState {
    /// Starts ownership for a pending generation and releases the prior stack.
    pub fn begin_generation(&mut self, generation: u64) -> bool {
        if generation <= self.generation {
            return false;
        }
        self.generation = generation;
        self.admission = PackAdmission::None;
        true
    }

    /// Publishes admission only for the pending/current or a newer generation.
    pub fn replace_for_generation(&mut self, generation: u64, admission: PackAdmission) -> bool {
        if generation < self.generation {
            return false;
        }
        self.generation = generation;
        self.admission = admission;
        true
    }

    /// Releases admission when the current network session terminates.
    pub fn clear_current(&mut self) {
        self.admission = PackAdmission::None;
    }

    /// Reports the owned generation for downstream adapter tests.
    #[cfg(any(test, feature = "test-support"))]
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Reports admitted bytes for downstream adapter tests.
    #[cfg(any(test, feature = "test-support"))]
    pub const fn admission(&self) -> &PackAdmission {
        &self.admission
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[test]
    fn required_pack_rejection_stays_between_subscriber_compile_and_completion() {
        for required in [false, true] {
            let archive = protocol::ResourcePackArchive::unencrypted(
                "11111111-2222-3333-4444-555555555555".parse().unwrap(),
                "1.2.3".into(),
                String::new(),
                vec![0; 32],
            );
            let preparation = PackPreparation {
                inputs: Arc::default(),
                admission: PackAdmission::Validated(resource_pack::validate_handoff(
                    protocol::ResourcePackHandoff::from_archives(vec![archive]),
                )),
                required,
            };
            let order = RefCell::new(Vec::new());
            let result = preparation
                .prepare_application(
                    |_| {
                        order.borrow_mut().push("compile");
                        Some(())
                    },
                    |_| order.borrow_mut().push("complete"),
                )
                .unwrap();
            assert_eq!(result.is_err(), required);
            assert_eq!(
                order.into_inner(),
                if required {
                    vec!["compile"]
                } else {
                    vec!["compile", "complete"]
                }
            );
        }
    }
}
