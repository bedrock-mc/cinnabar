//! Retains equivalent server stacks under the same process tables for joins and reloads.
//! Only blocks and icons recompile when their StartGame inputs change.

use std::{
    collections::VecDeque,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use client_ui::ui_runtime::presentation::ServerUiPack;
use resource_pack::{PackAdmission, ValidatedPackStack};

use super::{PackApplication, compile_application};
use pack_runtime::PackInputs;
use pack_runtime::reload_diff::Changes;

/// A lobby and the game it transfers to.
const KEPT_STACKS: usize = 2;

/// Bytes the newest stack may retain after leaving for the menu: a sixteenth of physical memory,
/// 512 MiB on an 8 GiB device. A larger stack is released and a rejoin compiles it again.
#[cfg(not(target_arch = "wasm32"))]
fn rejoin_budget() -> usize {
    usize::try_from(launcher_host::memory::physical_bytes() / 16).unwrap_or(usize::MAX)
}

/// Browsers report no physical memory; keep what a small device could afford.
#[cfg(target_arch = "wasm32")]
fn rejoin_budget() -> usize {
    128 * 1024 * 1024
}

/// Bytes keeping `application` costs: its archives plus every distinct large buffer its compiled
/// outputs own, including startup pages and carriers they share.
fn retained_bytes(
    application: &PackApplication,
    others: &[&dyn assets::compiled::RetainedBuffers],
) -> usize {
    use assets::compiled::RetainedBuffers;
    /// The admitted archives, which every stack view and sound pack reads from, and the
    /// extension marker read from them.
    struct Sources<'a>(&'a PackAdmission, Option<&'a [u8]>);
    impl RetainedBuffers for Sources<'_> {
        fn visit_buffers(&self, visit: &mut assets::compiled::BufferVisitor<'_>) {
            if let PackAdmission::Validated(stack) = self.0 {
                for pack in stack.packs() {
                    assets::compiled::visit_slice(pack.archive_bytes().as_slice(), visit);
                }
            }
            if let Some(marker) = self.1 {
                assets::compiled::visit_slice(marker, visit);
            }
        }
    }
    // Registries, inputs, language, item components, property defaults and sound definitions
    // are tables built over the archives and startup carriers, small beside these buffers.
    let PackApplication {
        registries: _,
        dependencies: _,
        terrain_identity: _,
        extension_marker,
        admission,
        inputs: _,
        server_lang: _,
        block_overlay,
        item_icons,
        item_components: _,
        glyph_sheets,
        block_entity_art,
        entities,
        entity_artwork,
        prepared_actor_artwork,
        property_defaults: _,
        server_ui,
        server_sounds: _,
        aim_assist_textures,
    } = application;
    let owned: [&dyn assets::compiled::RetainedBuffers; 10] = [
        &Sources(admission, extension_marker.as_deref()),
        block_overlay,
        item_icons,
        glyph_sheets,
        block_entity_art,
        entities,
        entity_artwork,
        prepared_actor_artwork,
        server_ui,
        aim_assist_textures,
    ];
    assets::compiled::retained_bytes(
        &owned
            .into_iter()
            .chain(others.iter().copied())
            .collect::<Vec<_>>(),
    )
}

/// Process tables a compile reads besides the stack and StartGame facts.
#[derive(Clone, Debug)]
pub(in crate::runtime::network) struct CompileEnvironment {
    /// Language and glyph subscribers read the selected UI language's files.
    language: String,
    /// Device capacity affects which block texture routes can be retained.
    max_layers: u32,
    /// Material keys, terrain aliases, item routes and actor artwork: each is installed once at
    /// startup and never changes afterwards.
    carrier_tables: [bool; 5],
    /// Entity compiles resolve against these vanilla definitions and rasters.
    vanilla_refs: Option<Arc<assets::VanillaEntityRefs>>,
    vanilla_pack_dir: Option<PathBuf>,
}

impl CompileEnvironment {
    /// The tables a compile starting now reads.
    pub(in crate::runtime::network) fn current() -> Self {
        use crate::runtime::network::{entity_texture_reload, item_icons};
        Self {
            language: pack_runtime::pack_language::active_language_code(),
            max_layers: super::overlay_texture_layer_limit(),
            carrier_tables: [
                super::BASE_MATERIAL_KEYS.get().is_some(),
                crate::runtime::network::block_entity_art::base().is_some(),
                pack_runtime::pack_textures::base_terrain_catalog_installed(),
                item_icons::vanilla_item_paths_installed(),
                entity_texture_reload::base_actor_artwork_installed(),
            ],
            vanilla_refs: pack_runtime::entity_pack::vanilla_refs(),
            vanilla_pack_dir: pack_runtime::entity_pack::vanilla_pack_dir(),
        }
    }
}

impl PartialEq for CompileEnvironment {
    /// Refs compare by allocation: a kept environment holds its refs, so the address is unique.
    fn eq(&self, other: &Self) -> bool {
        self.language == other.language
            && self.max_layers == other.max_layers
            && self.carrier_tables == other.carrier_tables
            && match (&self.vanilla_refs, &other.vanilla_refs) {
                (Some(ours), Some(theirs)) => Arc::ptr_eq(ours, theirs),
                (None, None) => true,
                _ => false,
            }
            && self.vanilla_pack_dir == other.vanilla_pack_dir
    }
}

struct Kept {
    environment: CompileEnvironment,
    stack: Arc<ValidatedPackStack>,
    /// Admits `stack`; `server_ui` is the source the worker catalog prepares, not its result.
    application: PackApplication,
    /// Keeps the worker catalog cache's prepared UI alive for the next join over the same carrier.
    prepared_ui: Option<Arc<ServerUiPack>>,
}

impl Kept {
    /// Bytes keeping this entry costs; buffers its source and prepared UI share count once.
    fn retained_bytes(&self) -> usize {
        retained_bytes(&self.application, &[&self.prepared_ui])
    }
}

/// The most recently joined stacks, newest first.
pub(crate) struct CompiledStacks(Mutex<VecDeque<Kept>>);

impl std::fmt::Debug for CompiledStacks {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CompiledStacks")
            .field("kept", &self.lock().len())
            .finish()
    }
}

/// Joins and the post-join reload share these; leaving for the menu keeps only the newest.
pub(in crate::runtime::network) static LATEST: CompiledStacks = CompiledStacks::new();

impl CompiledStacks {
    /// Creates an empty bounded cache of compiled server stacks.
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(VecDeque::new()))
    }

    /// Borrows the retained stacks, recovering their ownership after a poisoned lock.
    fn lock(&self) -> MutexGuard<'_, VecDeque<Kept>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// The kept stack and application compiled from contents every read of `stack` matches,
    /// under `environment`.
    fn matching(
        &self,
        stack: &ValidatedPackStack,
        environment: &CompileEnvironment,
    ) -> Option<(Arc<ValidatedPackStack>, PackApplication)> {
        self.matching_by(stack, environment, ValidatedPackStack::same_contents)
    }

    /// [`Self::matching`] with `same` deciding whether two stacks read alike. Archives compare
    /// outside the lock, so a release from the frame never waits behind a comparison.
    fn matching_by(
        &self,
        stack: &ValidatedPackStack,
        environment: &CompileEnvironment,
        same: impl Fn(&ValidatedPackStack, &ValidatedPackStack) -> bool,
    ) -> Option<(Arc<ValidatedPackStack>, PackApplication)> {
        let candidates: Vec<_> = self
            .lock()
            .iter()
            .filter(|kept| kept.environment == *environment)
            .map(|kept| (Arc::clone(&kept.stack), kept.application.clone()))
            .collect();
        candidates.into_iter().find(|(kept, _)| same(kept, stack))
    }

    /// Retains an active join, newest first. `cancelled` must not access this cache.
    /// `source_ui` is the server UI before the worker catalog prepared `application`'s.
    pub(in crate::runtime::network) fn remember(
        &self,
        environment: CompileEnvironment,
        application: &PackApplication,
        source_ui: Option<Arc<ServerUiPack>>,
        cancelled: &dyn Fn() -> bool,
    ) {
        self.remember_by(
            environment,
            application,
            source_ui,
            ValidatedPackStack::same_contents,
            cancelled,
        );
    }

    /// [`Self::remember`] with `same` deciding whether two stacks read alike. Archives compare,
    /// and replaced entries drop, outside the lock.
    fn remember_by(
        &self,
        environment: CompileEnvironment,
        application: &PackApplication,
        source_ui: Option<Arc<ServerUiPack>>,
        same: impl Fn(&ValidatedPackStack, &ValidatedPackStack) -> bool,
        cancelled: &dyn Fn() -> bool,
    ) {
        let PackAdmission::Validated(stack) = &application.admission else {
            return;
        };
        let kept_stacks: Vec<_> = self
            .lock()
            .iter()
            .map(|kept| Arc::clone(&kept.stack))
            .collect();
        let replaced: Vec<_> = kept_stacks
            .into_iter()
            .filter(|kept| same(kept, stack))
            .collect();
        let entry = Kept {
            environment,
            stack: Arc::clone(stack),
            prepared_ui: application.server_ui.clone(),
            application: PackApplication {
                server_ui: source_ui,
                ..application.clone()
            },
        };
        let evicted = {
            let mut kept = self.lock();
            if cancelled() {
                return;
            }
            let (mut evicted, retained): (Vec<Kept>, Vec<Kept>) = kept
                .drain(..)
                .partition(|kept| replaced.iter().any(|stack| Arc::ptr_eq(stack, &kept.stack)));
            *kept = retained.into();
            kept.push_front(entry);
            if kept.len() > KEPT_STACKS {
                evicted.extend(kept.split_off(KEPT_STACKS));
            }
            evicted
        };
        drop(evicted);
    }

    /// Keeps a finished join compiled under `compiled_under` unless it was cancelled, having left
    /// the server, or the tables are `now` different, so the compile may have read either value.
    pub(in crate::runtime::network) fn keep_join(
        &self,
        compiled_under: CompileEnvironment,
        now: &CompileEnvironment,
        application: &PackApplication,
        source_ui: Option<Arc<ServerUiPack>>,
        cancelled: &dyn Fn() -> bool,
    ) {
        if !cancelled() && compiled_under == *now {
            self.remember(compiled_under, application, source_ui, cancelled);
        }
    }

    /// Takes every kept stack; dropping the result releases their archives and compiled outputs.
    #[cfg(test)]
    pub(in crate::runtime::network) fn take(&self) -> impl Send + 'static {
        std::mem::take(&mut *self.lock())
    }

    /// Releases every kept stack, freeing them off the calling frame.
    #[cfg(test)]
    pub(crate) fn release(&self) {
        let released = self.take();
        #[cfg(not(target_arch = "wasm32"))]
        rayon::spawn(move || drop(released));
        #[cfg(target_arch = "wasm32")]
        client_runtime::session::release_off_frame(released);
    }

    /// Keeps only the newest stack, for a rejoin from the menu to reuse, unless what it retains
    /// exceeds the rejoin budget; the rest are freed off the calling frame.
    pub(crate) fn keep_for_rejoin(&self) {
        self.keep_newest_within(rejoin_budget());
    }

    /// [`Self::keep_for_rejoin`] under an explicit byte budget.
    fn keep_newest_within(&self, budget: usize) {
        let released: Vec<Kept> = {
            let mut kept = self.lock();
            let keep = kept.front().is_some_and(|newest| {
                newest.stack.release_cached_reads();
                newest.retained_bytes() <= budget
            });
            kept.drain(usize::from(keep)..).collect()
        };
        #[cfg(not(target_arch = "wasm32"))]
        rayon::spawn(move || drop(released));
        #[cfg(target_arch = "wasm32")]
        client_runtime::session::release_off_frame(released);
    }

    /// What keeping the newest stack costs, if one is kept.
    #[cfg(test)]
    fn newest_retained_bytes(&self) -> Option<usize> {
        self.lock().front().map(Kept::retained_bytes)
    }

    /// How many stacks are kept.
    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.lock().len()
    }
}

#[cfg(test)]
impl From<Arc<ValidatedPackStack>> for CompiledStacks {
    /// Builds an isolated fixture with one admitted stack and no compiled presentation.
    fn from(stack: Arc<ValidatedPackStack>) -> Self {
        let kept = Self::new();
        kept.remember(
            CompileEnvironment::current(),
            &PackApplication {
                admission: PackAdmission::Validated(stack),
                ..Default::default()
            },
            None,
            &|| false,
        );
        kept
    }
}

/// Compiles from an equivalent retained stack, rebuilding only changed StartGame subscribers.
/// Keeps one archive copy and returns None after cancellation.
pub(in crate::runtime::network) fn compile_reusing(
    kept: &CompiledStacks,
    stack: Arc<ValidatedPackStack>,
    inputs: Arc<PackInputs>,
    environment: &CompileEnvironment,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<PackApplication> {
    match kept.matching(&stack, environment) {
        Some((compiled, previous)) => {
            let changes = Changes::for_inputs(&previous.inputs, &inputs);
            bevy::log::info!(
                packs = compiled.packs().len(),
                recompile_blocks = changes.blocks,
                recompile_icons = changes.icons,
                icon_pairs_changed = previous.inputs.icons != inputs.icons,
                block_item_pairs_changed = previous.inputs.block_items != inputs.block_items,
                "reusing a recent compile of the same server packs"
            );
            compile_application(compiled, inputs, Some(&previous), changes, cancelled)
        }
        None => compile_application(stack, inputs, None, Changes::all(), cancelled),
    }
}

#[cfg(test)]
#[path = "reuse_tests.rs"]
mod tests;
