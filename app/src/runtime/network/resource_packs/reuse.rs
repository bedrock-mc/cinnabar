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
use crate::runtime::network::{pack_reload::PackInputs, pack_reload_diff::Changes};

/// A lobby and the game it transfers to.
const KEPT_STACKS: usize = 2;

/// Process tables a compile reads besides the stack and StartGame facts.
#[derive(Clone, Debug)]
pub(in crate::runtime::network) struct CompileEnvironment {
    /// Language and glyph subscribers read the selected UI language's files.
    language: String,
    /// Material keys, terrain aliases, item routes and actor artwork: each is installed once at
    /// startup and never changes afterwards.
    carrier_tables: [bool; 4],
    /// Entity compiles resolve against these vanilla definitions and rasters.
    vanilla_refs: Option<Arc<assets::VanillaEntityRefs>>,
    vanilla_pack_dir: Option<PathBuf>,
}

impl CompileEnvironment {
    /// The tables a compile starting now reads.
    pub(in crate::runtime::network) fn current() -> Self {
        use crate::runtime::network::{entity_pack, entity_texture_reload, item_icons};
        Self {
            language: super::active_language_code(),
            carrier_tables: [
                super::BASE_MATERIAL_KEYS.get().is_some(),
                client_session::pack_textures::base_terrain_catalog_installed(),
                item_icons::vanilla_item_paths_installed(),
                entity_texture_reload::base_actor_artwork_installed(),
            ],
            vanilla_refs: entity_pack::vanilla_refs(),
            vanilla_pack_dir: entity_pack::vanilla_pack_dir(),
        }
    }
}

impl PartialEq for CompileEnvironment {
    /// Refs compare by allocation: a kept environment holds its refs, so the address is unique.
    fn eq(&self, other: &Self) -> bool {
        self.language == other.language
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
    _prepared_ui: Option<Arc<ServerUiPack>>,
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

/// Joins and the post-join reload share these; leaving for the menu releases them.
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
            _prepared_ui: application.server_ui.clone(),
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
    pub(in crate::runtime::network) fn take(&self) -> impl Send + 'static {
        std::mem::take(&mut *self.lock())
    }

    /// Releases every kept stack, as leaving for the menu releases the session's, freeing them
    /// off the calling frame.
    pub(crate) fn release(&self) {
        let released = self.take();
        rayon::spawn(move || drop(released));
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
