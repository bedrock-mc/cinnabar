//! Recently compiled server stacks. A later join, or the reload that follows a join, reuses one
//! whose stack reads exactly alike under the same process tables. Only blocks and icons also read
//! StartGame facts, so only they recompile when those facts differ.

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
    pub(crate) const fn new() -> Self {
        Self(Mutex::new(VecDeque::new()))
    }

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
        self.lock()
            .iter()
            .find(|kept| kept.environment == *environment && kept.stack.same_contents(stack))
            .map(|kept| (Arc::clone(&kept.stack), kept.application.clone()))
    }

    /// Keeps a join's application, newest first, replacing any entry for the same contents.
    /// `source_ui` is the server UI before the worker catalog prepared `application`'s.
    pub(in crate::runtime::network) fn remember(
        &self,
        environment: CompileEnvironment,
        application: &PackApplication,
        source_ui: Option<Arc<ServerUiPack>>,
    ) {
        let PackAdmission::Validated(stack) = &application.admission else {
            return;
        };
        let mut kept = self.lock();
        kept.retain(|kept| !kept.stack.same_contents(stack));
        kept.push_front(Kept {
            environment,
            stack: Arc::clone(stack),
            _prepared_ui: application.server_ui.clone(),
            application: PackApplication {
                server_ui: source_ui,
                ..application.clone()
            },
        });
        kept.truncate(KEPT_STACKS);
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

    /// Keeps `stack` as a join would, with nothing compiled from it.
    #[cfg(test)]
    pub(crate) fn keep_for_test(&self, stack: Arc<ValidatedPackStack>) {
        self.remember(
            CompileEnvironment::current(),
            &PackApplication {
                admission: PackAdmission::Validated(stack),
                ..Default::default()
            },
            None,
        );
    }
}

/// Compiles `stack`, starting from a kept application whose contents and environment match:
/// then only subscribers reading StartGame facts that differ compile, and the result admits the
/// kept stack so one copy of the archives stays alive. `None` once cancelled.
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
