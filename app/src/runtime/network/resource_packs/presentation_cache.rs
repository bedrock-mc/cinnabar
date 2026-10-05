//! Bounded reuse of immutable pack subscribers across session admissions.

use super::{PackApplication, active_language_code, prepare_changed_application};
use resource_pack::{PackAdmission, ValidatedPackStack};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

static CONTEXT_GENERATION: AtomicU64 = AtomicU64::new(0);
static CACHE: PresentationCache = PresentationCache(Mutex::new(None));
static ARTWORK_CACHE: ArtworkCache = ArtworkCache(Mutex::new(None));

#[derive(Default)]
struct ArtworkCache(
    Mutex<Option<Arc<client_presentation::prepared_actor_artwork::PreparedActorArtwork>>>,
);

impl ArtworkCache {
    fn prepare(
        &self,
        base: &render::ActorArtworkPages,
        pack: &Arc<assets::SessionEntityPack>,
    ) -> Arc<client_presentation::prepared_actor_artwork::PreparedActorArtwork> {
        {
            let cached = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(cached) = cached.as_ref()
                && cached.pages_for(base, pack).is_some()
            {
                return cached.clone();
            }
        }
        let prepared = Arc::new(
            client_presentation::prepared_actor_artwork::PreparedActorArtwork::new(base, pack),
        );
        *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(prepared.clone());
        prepared
    }
}

pub(super) fn prepare_artwork(
    base: &render::ActorArtworkPages,
    pack: &Arc<assets::SessionEntityPack>,
) -> Arc<client_presentation::prepared_actor_artwork::PreparedActorArtwork> {
    ARTWORK_CACHE.prepare(base, pack)
}

#[derive(Clone)]
struct Context {
    generation: u64,
    locale: String,
    vanilla: Option<Arc<assets::VanillaEntityRefs>>,
    material_keys_ready: bool,
}

impl Context {
    fn capture() -> Self {
        Self {
            generation: CONTEXT_GENERATION.load(Ordering::Acquire),
            locale: active_language_code(),
            vanilla: super::super::entity_pack::vanilla_refs(),
            material_keys_ready: super::BASE_MATERIAL_KEYS.get().is_some(),
        }
    }

    fn matches(&self, other: &Self) -> bool {
        self.generation == other.generation
            && self.locale == other.locale
            && self.material_keys_ready == other.material_keys_ready
            && match (&self.vanilla, &other.vanilla) {
                (Some(left), Some(right)) => Arc::ptr_eq(left, right) || left == right,
                (None, None) => true,
                _ => false,
            }
    }
}

struct CachedPresentation {
    stack: Vec<[u8; 32]>,
    context: Context,
    application: PackApplication,
}

#[derive(Default)]
struct PresentationCache(Mutex<Option<CachedPresentation>>);

impl PresentationCache {
    fn prepare(
        &self,
        stack: Arc<ValidatedPackStack>,
        inputs: Arc<client_session::PackInputs>,
        context: Context,
        context_is_current: impl Fn(&Context) -> bool,
        compile: impl FnOnce(
            Arc<ValidatedPackStack>,
            Arc<client_session::PackInputs>,
        ) -> PackApplication,
    ) -> (PackApplication, bool) {
        if !stack.rejections().is_empty() {
            return (compile(stack, inputs), false);
        }
        let identity = stack
            .packs()
            .iter()
            .map(resource_pack::ValidatedPack::compilation_identity)
            .collect::<Vec<_>>();
        {
            let cached = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(cached) = cached.as_ref()
                && cached.stack == identity
                && cached.context.matches(&context)
                && same_inputs(&cached.application.inputs, &inputs)
                && context_is_current(&context)
            {
                let mut application = cached.application.clone();
                application.inputs = inputs;
                application.admission = PackAdmission::Validated(stack);
                return (application, true);
            }
        }
        let application = compile(stack, inputs);
        if context_is_current(&context) {
            let mut compiled = application.clone();
            compiled.admission = PackAdmission::None;
            compiled.item_components = None;
            compiled.prepared_actor_artwork = None;
            *self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(CachedPresentation {
                stack: identity,
                context,
                application: compiled,
            });
        }
        (application, false)
    }
}

fn same_inputs(left: &client_session::PackInputs, right: &client_session::PackInputs) -> bool {
    left.hashed == right.hashed
        && left.blocks == right.blocks
        && left.icons == right.icons
        && left.block_items == right.block_items
}

pub(super) fn invalidate_context() {
    CONTEXT_GENERATION.fetch_add(1, Ordering::Release);
}

pub(super) fn prepare(
    stack: Arc<ValidatedPackStack>,
    inputs: Arc<client_session::PackInputs>,
) -> PackApplication {
    let started = std::time::Instant::now();
    let (application, cache_hit) = CACHE.prepare(
        stack,
        inputs,
        Context::capture(),
        |context| context.matches(&Context::capture()),
        |stack, inputs| prepare_changed_application(stack, inputs, None),
    );
    bevy::log::debug!(
        cache_hit,
        elapsed_ms = started.elapsed().as_secs_f64() * 1000.0,
        "session pack presentation prepared"
    );
    application
}

#[cfg(test)]
mod tests;
