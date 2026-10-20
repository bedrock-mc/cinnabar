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

impl PackInputs {
    /// Whether block visuals compiled for `other` also hold for these facts.
    #[must_use]
    pub fn same_blocks(&self, other: &Self) -> bool {
        self.hashed == other.hashed && self.blocks == other.blocks
    }

    /// Whether item icons compiled for `other` also hold for these facts; icons draw custom
    /// blocks, so they follow the blocks too. Registry icon keys use the compiler's
    /// last-value-per-identifier map; block item order still controls bounded artwork selection.
    #[must_use]
    pub fn same_icons(&self, other: &Self) -> bool {
        self.same_blocks(other)
            && self.block_items == other.block_items
            && (self.icons == other.icons || {
                let ours: std::collections::BTreeMap<_, _> =
                    self.icons.iter().map(|(id, key)| (id, key)).collect();
                let theirs: std::collections::BTreeMap<_, _> =
                    other.icons.iter().map(|(id, key)| (id, key)).collect();
                ours == theirs
            })
    }
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
    /// Whether the server's own handed-off packs were applied, apart from relay packs.
    server_packs: bool,
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

    /// Reports whether the session must bracket its pump with pack-application reports: the
    /// server's handed-off packs were applied.
    pub fn has_applied_packs(&self) -> bool {
        self.server_packs
    }

    /// Whether any pack, the server's or a relay's, applies to this session.
    pub fn has_packs(&self) -> bool {
        matches!(&self.admission, PackAdmission::Validated(stack) if !stack.packs().is_empty())
    }
}

/// Reads StartGame pack facts and validates the one-shot login handoff.
pub fn prepare_session_packs(
    mut handoff: bedrock_packs::ResourcePackHandoff,
    game_data: &protocol::GameData,
    physical_memory_bytes: u64,
) -> PackPreparation {
    let mut blocks = game_data.custom_blocks.clone();
    crate::block_physics::resolve(&mut blocks);
    let block_items = custom_block_items(&game_data.items, &blocks);
    let hashed = game_data.world.block_network_ids_are_hashes;
    tracing::info!(
        block_network_ids_are_hashes = hashed,
        block_property_count = game_data.block_definition_count,
        custom_block_count = blocks.blocks.len(),
        custom_state_count = blocks.total_states(),
        skipped_block_definitions = blocks.skipped,
        "START_GAME_BLOCK_IDS"
    );
    let required = handoff.required();
    let added = handoff.take_relay_packs();
    let stack = resource_pack::validate_handoff_for_device(handoff, physical_memory_bytes);
    let server_packs = !stack.packs().is_empty();
    let stack = with_relay_packs(stack, added, physical_memory_bytes);
    let admission = if stack.packs().is_empty() && stack.rejections().is_empty() {
        PackAdmission::None
    } else {
        PackAdmission::Validated(stack)
    };
    PackPreparation {
        inputs: Arc::new(PackInputs {
            blocks,
            icons: game_data.items.icon_keys.to_vec(),
            block_items: block_items.to_vec(),
            hashed,
        }),
        admission,
        required,
        server_packs,
    }
}

/// Layers a packet relay's packs around the server stack. They are admitted apart, so one that
/// fails is only logged and never rejects a required server stack.
fn with_relay_packs(
    server: Arc<resource_pack::ValidatedPackStack>,
    added: Vec<bedrock_packs::RelayPack>,
    physical_memory_bytes: u64,
) -> Arc<resource_pack::ValidatedPackStack> {
    if added.is_empty() {
        return server;
    }
    let added = added
        .into_iter()
        .map(|pack| resource_pack::AddedPack {
            archive: pack.archive,
            above_server: pack.above_server,
        })
        .collect();
    let (stack, refused) =
        resource_pack::compose_added_packs(&server, added, physical_memory_bytes);
    for error in refused {
        tracing::warn!(%error, "skipping a resource pack the packet mod added");
    }
    Arc::new(stack)
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
    items: &protocol::StartupItems,
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
    let mut pairs = items
        .identifiers
        .iter()
        .filter_map(|identifier| names.get(identifier).cloned())
        .map(|block| (Arc::clone(&block), block))
        .collect::<Vec<_>>();
    for (identifier, facts) in items.components.iter() {
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

use resource_pack::StackFingerprint;

/// Copies the admission-time pack identities and digests in stack order.
pub fn stack_fingerprint(stack: &resource_pack::ValidatedPackStack) -> StackFingerprint {
    stack.fingerprint().clone()
}

#[cfg(test)]
mod icon_input_tests {
    use super::*;

    #[test]
    fn reconnect_icon_order_does_not_recompile_icons() {
        let first = PackInputs {
            icons: vec![("test:a".into(), "a".into()), ("test:b".into(), "b".into())],
            ..Default::default()
        };
        let mut next = first.clone();
        next.icons.reverse();
        assert!(!crate::reload_diff::Changes::for_inputs(&first, &next).icons);
    }

    #[test]
    fn icon_key_comparison_preserves_duplicate_precedence_and_block_order() {
        let first = PackInputs {
            icons: vec![
                ("test:a".into(), "first".into()),
                ("test:a".into(), "last".into()),
            ],
            block_items: vec![
                ("test:a".into(), "test:a".into()),
                ("test:b".into(), "test:b".into()),
            ],
            ..Default::default()
        };
        let mut next = first.clone();
        next.icons.reverse();
        assert!(!first.same_icons(&next));
        next = first.clone();
        next.icons.remove(0);
        assert!(first.same_icons(&next));
        next.block_items.reverse();
        assert!(!first.same_icons(&next));
    }
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

    /// How many server resource packs the current session admitted.
    pub fn pack_count(&self) -> usize {
        match &self.admission {
            PackAdmission::Validated(stack) => stack.packs().len(),
            _ => 0,
        }
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
            let archive = bedrock_packs::ResourcePackArchive::unencrypted(
                "11111111-2222-3333-4444-555555555555".parse().unwrap(),
                "1.2.3".into(),
                String::new(),
                vec![0; 32],
            );
            let preparation = PackPreparation {
                inputs: Arc::default(),
                admission: PackAdmission::Validated(resource_pack::validate_handoff(
                    bedrock_packs::ResourcePackHandoff::from_archives(vec![archive]),
                )),
                required,
                server_packs: false,
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
    // A presentation that reuses an earlier compile holds its own copy of the archives, so the
    // session must not also keep the copy this login validated.
    #[test]
    fn preparing_a_session_releases_the_archives_it_validated() {
        let archive = bedrock_packs::ResourcePackArchive::unencrypted(
            "11111111-2222-3333-4444-555555555555".parse().unwrap(),
            "1.2.3".into(),
            String::new(),
            vec![0; 32],
        );
        let game_data = protocol::GameData::default();
        let mut validated = None;
        let preparation = prepare_session_packs(
            bedrock_packs::ResourcePackHandoff::from_archives(vec![archive]),
            &game_data,
            u64::MAX,
        );
        let packs = preparation.prepare_application(
            |preparation| {
                let resource_pack::PackAdmission::Validated(stack) = &preparation.admission else {
                    panic!("the handoff admits a stack");
                };
                validated = Some(Arc::downgrade(stack));
                Some(())
            },
            |_| {},
        );
        assert!(matches!(packs, Some(Ok(()))));
        assert!(
            !preparation.has_applied_packs(),
            "the only pack was rejected"
        );
        assert!(preparation.inputs.blocks.blocks.is_empty());
        drop(preparation);
        assert!(validated.unwrap().upgrade().is_none());
    }

    /// A one-file resource pack under `id`, version 1.0.0.
    fn pack_zip(id: &str) -> Vec<u8> {
        use std::io::Write;
        let manifest = format!(
            r#"{{"format_version":2,"header":{{"uuid":"{id}","version":[1,0,0]}},"modules":[{{"type":"resources"}}]}}"#
        );
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (path, bytes) in [("manifest.json", manifest.as_bytes()), ("a.txt", b"a")] {
            writer
                .start_file(path, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    // A relay's packs layer around a required server stack, and one that fails admission never
    // refuses the join or counts as the server's packs.
    #[test]
    fn relay_packs_layer_around_a_required_server_stack() {
        let server = "11111111-2222-3333-4444-555555555555";
        let relay = "66666666-2222-3333-4444-555555555555";
        let handoff = bedrock_packs::ResourcePackHandoff::from_archives(vec![
            bedrock_packs::ResourcePackArchive::unencrypted(
                server.parse().unwrap(),
                "1.0.0".into(),
                String::new(),
                pack_zip(server),
            ),
        ])
        .with_required(true)
        .with_relay_packs(vec![
            bedrock_packs::RelayPack {
                archive: b"not a pack".to_vec(),
                above_server: false,
            },
            bedrock_packs::RelayPack {
                archive: pack_zip(relay),
                above_server: true,
            },
        ]);
        let preparation = prepare_session_packs(handoff, &protocol::GameData::default(), u64::MAX);
        let PackAdmission::Validated(stack) = &preparation.admission else {
            panic!("the stack is validated");
        };
        let order: Vec<_> = stack
            .packs()
            .iter()
            .map(|pack| pack.pack_id().to_string())
            .collect();
        assert_eq!(order, [server, relay]);
        assert!(preparation.has_applied_packs() && preparation.has_packs());
        let applied = preparation.prepare_application(|_| Some(()), |_| {});
        assert!(matches!(applied, Some(Ok(()))));
    }

    #[test]
    fn relay_only_packs_prepare_without_server_application_reports() {
        let id = "66666666-2222-3333-4444-555555555555";
        let handoff = bedrock_packs::ResourcePackHandoff::from_archives(Vec::new())
            .with_relay_packs(vec![bedrock_packs::RelayPack {
                archive: pack_zip(id),
                above_server: true,
            }]);
        let preparation = prepare_session_packs(handoff, &protocol::GameData::default(), u64::MAX);
        assert!(preparation.has_packs());
        assert!(!preparation.has_applied_packs());
        let applied = preparation.prepare_application(|_| Some(()), |_| {});
        assert!(matches!(applied, Some(Ok(()))));
    }
}
