//! Bounded reuse of immutable pack subscribers across session admissions.

use super::{PackApplication, active_language_code, prepare_changed_application};
use resource_pack::{PackAdmission, ValidatedPackStack};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

mod compilation_inputs;
use compilation_inputs::inputs_mismatch;

static CONTEXT_GENERATION: AtomicU64 = AtomicU64::new(0);
const PRESENTATION_CACHE_CAPACITY: usize = 3;
static CACHE: PresentationCache = PresentationCache(Mutex::new(Vec::new()));
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
struct PresentationCache(Mutex<Vec<CachedPresentation>>);

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
        let miss_reason = {
            let mut cached = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !context_is_current(&context) {
                "context"
            } else {
                let had_obsolete_context = cached
                    .iter()
                    .any(|entry| entry.context.generation != context.generation);
                cached.retain(|entry| entry.context.generation == context.generation);
                if let Some(index) = cached.iter().rposition(|entry| {
                    entry.stack == identity
                        && entry.context.matches(&context)
                        && inputs_mismatch(&entry.application.inputs, &inputs).is_none()
                }) {
                    let entry = cached.remove(index);
                    let mut application = entry.application.clone();
                    application.inputs = inputs;
                    application.admission = PackAdmission::Validated(stack);
                    cached.push(entry);
                    drop(cached);
                    bevy::log::info!(cache_hit = true, "session pack presentation cache");
                    return (application, true);
                }
                cached.last().map_or(
                    if had_obsolete_context {
                        "context"
                    } else {
                        "empty"
                    },
                    |entry| {
                        if entry.stack != identity {
                            "pack_stack"
                        } else if !entry.context.matches(&context) {
                            "context"
                        } else {
                            inputs_mismatch(&entry.application.inputs, &inputs).unwrap_or("context")
                        }
                    },
                )
            }
        };
        bevy::log::info!(
            cache_hit = false,
            miss_reason,
            "session pack presentation cache"
        );
        let application = compile(stack, inputs);
        if context_is_current(&context) {
            let mut compiled = application.clone();
            compiled.admission = PackAdmission::None;
            compiled.item_components = None;
            compiled.prepared_actor_artwork = None;
            let mut cached = self
                .0
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            cached.retain(|entry| {
                entry.context.generation == context.generation
                    && !(entry.stack == identity
                        && entry.context.matches(&context)
                        && inputs_mismatch(&entry.application.inputs, &compiled.inputs).is_none())
            });
            if cached.len() == PRESENTATION_CACHE_CAPACITY {
                cached.remove(0);
            }
            cached.push(CachedPresentation {
                stack: identity,
                context,
                application: compiled,
            });
        }
        (application, false)
    }
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
