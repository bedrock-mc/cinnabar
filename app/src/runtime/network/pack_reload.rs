//! Cinnabar extension: optional resource packs can change without leaving a world.

use super::reload_environment::{
    EnvironmentBase, PreparedEnvironment, apply_biome_overlay, prepare_environment,
};
use super::resource_packs::{self, PackApplication};
use crate::{runtime::world::ClientWorld, ui_runtime::UiRuntime};
use bevy::prelude::{Query, Res, ResMut, Resource};
use resource_pack::{PackAdmission, ValidatedPackStack};
use std::{
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

/// Session facts needed to compile visuals again without another StartGame.
#[derive(Clone, Debug, Default)]
pub(super) struct PackInputs {
    pub(super) blocks: protocol::CustomBlocks,
    pub(super) icons: Vec<(Arc<str>, Arc<str>)>,
    pub(super) block_items: Vec<(Arc<str>, Arc<str>)>,
    pub(super) hashed: bool,
}

struct Prepared {
    revision: u64,
    generation: u64,
    application: PackApplication,
    assets: Arc<assets::RuntimeAssets>,
    environment: Option<PreparedEnvironment>,
    elapsed: Duration,
}

struct Pending {
    revision: u64,
    generation: u64,
    result: Mutex<mpsc::Receiver<Result<Prepared, String>>>,
}

/// A bounded worker transaction; only the newest requested stack may be published.
#[derive(Resource)]
pub(crate) struct PackReload {
    geometry: super::pack_reload_geometry::GeometryPreparation,
    globals: Arc<ValidatedPackStack>,
    server: Arc<ValidatedPackStack>,
    inputs: Arc<PackInputs>,
    items: Option<Arc<crate::ui_runtime::item_facts::SessionItemComponents>>,
    generation: u64,
    revision: u64,
    applied: u64,
    pending: Option<Pending>,
    ready: Option<Prepared>,
    last_duration: Option<Duration>,
    error: Option<String>,
    previous: Option<PackApplication>,
    previous_assets: Option<Arc<assets::RuntimeAssets>>,
}

impl Default for PackReload {
    fn default() -> Self {
        let empty = resource_pack::validate_handoff(protocol::ResourcePackHandoff::default());
        Self {
            geometry: Default::default(),
            globals: empty.clone(),
            server: empty,
            inputs: Arc::default(),
            items: None,
            generation: 0,
            revision: 0,
            applied: 0,
            pending: None,
            ready: None,
            last_duration: None,
            error: None,
            previous: None,
            previous_assets: None,
        }
    }
}

impl PackReload {
    /// Queues a global stack; importing or reordering never touches the network connection.
    pub(crate) fn request_globals(&mut self, stack: Arc<ValidatedPackStack>) -> u64 {
        self.globals = stack;
        self.revision += 1;
        self.error = None;
        self.revision
    }

    /// The settings host shows this while a worker prepares a replacement.
    pub(crate) fn progress(&self) -> Option<&str> {
        (self.applied != self.revision).then_some("Applying resource packs…")
    }

    /// Reports the latest preparation failure separately from pending work.
    pub(crate) fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// The last completed request; `error` distinguishes failed preparation from publication.
    pub(crate) fn completed_revision(&self) -> u64 {
        self.applied
    }

    /// CPU preparation duration for the last successful publication.
    pub(crate) fn last_duration(&self) -> Option<Duration> {
        self.last_duration
    }

    /// Retains server-owned facts while keeping user packs below the server stack.
    pub(super) fn begin_session(&mut self, generation: u64, packs: &PackApplication) {
        self.generation = generation;
        self.previous = None;
        self.server = match &packs.admission {
            PackAdmission::Validated(stack) => stack.clone(),
            PackAdmission::None => {
                resource_pack::validate_handoff(protocol::ResourcePackHandoff::default())
            }
        };
        self.inputs = packs.inputs.clone();
        self.items = packs.item_components.clone();
        self.revision += 1;
    }

    /// Retires server layers; globals remain available in the menus.
    pub(super) fn end_session(&mut self) {
        self.server = resource_pack::validate_handoff(protocol::ResourcePackHandoff::default());
        self.inputs = Arc::default();
        self.items = None;
        self.generation = 0;
        self.previous = None;
        self.revision += 1;
    }

    /// Starts one worker at a time, coalescing edits made during an existing build.
    fn start(
        &mut self,
        base: Arc<assets::RuntimeAssets>,
        catalog: Option<Arc<json_ui::Catalog>>,
        environment_base: Option<EnvironmentBase>,
        current_artwork: Option<render::ActorArtworkPages>,
    ) {
        let globals = self.globals.clone();
        let server = self.server.clone();
        let inputs = self.inputs.clone();
        let items = self.items.clone();
        let previous = self.previous.clone();
        let previous_assets = self.previous_assets.clone();
        let revision = self.revision;
        let generation = self.generation;
        let (tx, rx) = mpsc::sync_channel(1);
        self.pending = Some(Pending {
            revision,
            generation,
            result: Mutex::new(rx),
        });
        std::thread::spawn(move || {
            let start = Instant::now();
            let result = ValidatedPackStack::compose(&globals, &server)
                .map_err(|error| error.to_string())
                .map(|stack| {
                    let changes =
                        super::pack_reload_diff::Changes::between(&stack, previous.as_ref());
                    let stack = Arc::new(stack);
                    let view = resource_pack::LayeredPackView::new(stack.clone());
                    let environment = environment_base
                        .as_ref()
                        .filter(|_| changes.atmosphere || changes.particles)
                        .map(|base| {
                            prepare_environment(&view, base, changes.atmosphere, changes.particles)
                        });
                    let mut application = resource_packs::prepare_changed_application(
                        stack,
                        inputs,
                        previous.as_ref(),
                    );
                    if let Some(environment) = &environment {
                        application
                            .dependencies
                            .extend(environment.dependencies.clone());
                    }
                    application.item_components = items;
                    let artwork = current_artwork.as_ref().map(|current| {
                        application
                            .entity_artwork
                            .as_deref()
                            .filter(|next| next.identity() != current.identity())
                            .unwrap_or(current)
                            .clone()
                    });
                    application.prepare_actor_artwork(artwork.as_ref().filter(|_| generation != 0));
                    if changes.blocks {
                        let mut biome_overlay = assets::BlockOverlay::default();
                        let biome_view =
                            resource_pack::LayeredPackView::tracked(view.shared_stack());
                        apply_biome_overlay(&biome_view, &base, &mut biome_overlay);
                        application
                            .dependencies
                            .entry(super::pack_reload_diff::Subscriber::Blocks)
                            .or_default()
                            .extend(biome_view.dependencies().expect("tracked view").snapshot());
                        if biome_overlay.biomes.is_some() {
                            let overlay = application.block_overlay.get_or_insert_with(|| {
                                Arc::new(super::block_overlay::CompiledBlockOverlay {
                                    overlay: Default::default(),
                                    gaps: Default::default(),
                                })
                            });
                            Arc::make_mut(overlay).overlay.biomes = biome_overlay.biomes;
                        }
                    }
                    if let (Some(pack), Some(catalog)) =
                        (application.server_ui.as_ref(), catalog.as_ref())
                    {
                        let unchanged = previous.as_ref().is_some_and(|old| {
                            same_optional(&old.server_ui, &application.server_ui)
                        });
                        if !unchanged {
                            application.server_ui = Some(pack.prepare_catalog(catalog));
                        }
                    }
                    let ids = application.block_overlay.as_ref().map(|overlay| {
                        let start = base.visual_count() as u32;
                        start..start + overlay.overlay.visuals.len() as u32
                    });
                    let unchanged = previous.as_ref().is_some_and(|old| {
                        same_optional(&old.block_overlay, &application.block_overlay)
                    });
                    let assets = previous_assets.filter(|_| unchanged).unwrap_or_else(|| {
                        resource_packs::session_runtime_assets(
                            &base,
                            ids.as_ref(),
                            application.block_overlay.as_deref(),
                        )
                    });
                    Prepared {
                        revision,
                        generation,
                        application,
                        assets,
                        environment,
                        elapsed: start.elapsed(),
                    }
                });
            let _ = tx.send(result);
        });
    }
}

/// Publishes one complete CPU snapshot before rendering extracts the next frame.
#[allow(clippy::too_many_arguments)]
pub(crate) fn reload_resource_packs(
    mut reload: ResMut<PackReload>,
    mut world: ResMut<ClientWorld>,
    mut ui: ResMut<UiRuntime>,
    mut textures: Option<ResMut<render::ChunkTextureAssets>>,
    presentation: Option<Res<crate::ui_runtime::presentation::UiPresentationRuntime>>,
    environment_base: Option<Res<EnvironmentBase>>,
    mut atmosphere: Option<ResMut<render::AtmosphereTextureAssets>>,
    mut particles: Option<ResMut<render::ParticleSystem>>,
    mut entity_artwork: Option<ResMut<render::ActorArtworkPages>>,
    gpu_reload: Option<Res<render::ChunkTextureReload>>,
    mut chunks: Query<&mut render::ChunkRenderInstance>,
    mut render_queue: Option<ResMut<render::ChunkRenderQueue>>,
    mut biome_tints: Option<ResMut<render::ChunkBiomeTints>>,
    profiler: Option<Res<render::RuntimeStageProfiler>>,
) {
    let _timer = profiler
        .as_deref()
        .map(|profiler| profiler.time(render::RuntimeStage::PackReload));
    let result = reload
        .ready
        .take()
        .map(|prepared| (prepared.revision, prepared.generation, Ok(prepared)))
        .or_else(|| {
            reload.pending.as_ref().and_then(|rx| {
                match rx
                    .result
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .try_recv()
                {
                    Ok(result) => Some((rx.revision, rx.generation, result)),
                    Err(mpsc::TryRecvError::Empty) => None,
                    Err(mpsc::TryRecvError::Disconnected) => Some((
                        rx.revision,
                        rx.generation,
                        Err("Resource pack worker stopped".to_owned()),
                    )),
                }
            })
        });
    if let Some((revision, generation, result)) = result {
        reload.pending = None;
        let result = if revision == reload.revision && generation == reload.generation {
            Some(result)
        } else {
            if let (Some(gpu), Some(current)) = (gpu_reload.as_ref(), textures.as_ref()) {
                gpu.cancel_except(current.identity());
            }
            None
        };
        match result {
            Some(Ok(prepared))
                if prepared.revision == reload.revision
                    && prepared.generation == reload.generation =>
            {
                let candidate = render::ChunkTextureAssets::with_revision(
                    prepared.assets.clone(),
                    prepared.revision,
                );
                if let (Some(gpu), Some(current)) = (gpu_reload.as_ref(), textures.as_ref())
                    && !Arc::ptr_eq(current.assets(), &prepared.assets)
                {
                    if let Err(error) = reload.geometry.request(
                        &candidate,
                        current,
                        world.stream.as_ref(),
                        &chunks,
                        gpu,
                    ) {
                        gpu.cancel_except(current.identity());
                        reload.error = Some(error);
                        reload.applied = reload.revision;
                        return;
                    }
                    match gpu.status(candidate.identity()) {
                        None => {
                            reload.ready = Some(prepared);
                            return;
                        }
                        Some(Err(error)) => {
                            gpu.cancel_except(current.identity());
                            reload.error = Some(error);
                            reload.applied = reload.revision;
                            return;
                        }
                        Some(Ok(())) => {}
                    }
                } else if let (Some(gpu), Some(current)) = (gpu_reload.as_ref(), textures.as_ref())
                {
                    gpu.cancel_except(current.identity());
                }
                if let Some(gpu) = gpu_reload.as_ref()
                    && reload.geometry.publish(&mut chunks, gpu)
                    && textures.as_ref().is_some_and(|current| {
                        !current.assets().has_same_geometry(&prepared.assets)
                            || current.assets().biome_assets() != prepared.assets.biome_assets()
                    })
                    && let Some(queue) = render_queue.as_mut()
                {
                    queue.discard_resource_work();
                }
                let entities_changed = reload.previous.as_ref().is_none_or(|old| {
                    !same_optional(&old.entities, &prepared.application.entities)
                });
                let sounds_changed = reload.previous.as_ref().is_none_or(|old| {
                    !same_optional(&old.server_sounds, &prepared.application.server_sounds)
                });
                let items_changed = reload.previous.as_ref().is_none_or(|old| {
                    !same_optional(&old.item_icons, &prepared.application.item_icons)
                        || !same_optional(
                            &old.item_components,
                            &prepared.application.item_components,
                        )
                });
                reload.previous = Some(prepared.application.clone());
                reload.previous_assets = Some(prepared.assets.clone());
                if let Some(environment) = prepared.environment {
                    if let (Some(atmosphere), Some(next)) =
                        (atmosphere.as_mut(), environment.atmosphere)
                        && atmosphere.identity() != next.identity()
                    {
                        **atmosphere = next;
                    }
                    if let (Some(particles), Some(next)) =
                        (particles.as_mut(), environment.particles)
                    {
                        **particles = next;
                    }
                }
                let packs = prepared.application;
                if let (Some(current), Some(next)) =
                    (entity_artwork.as_mut(), packs.entity_artwork.as_ref())
                    && current.identity() != next.identity()
                {
                    **current = (**next).clone();
                }
                if let Some(stream) = world.stream.as_mut() {
                    stream.reload_resource_assets(prepared.assets.clone());
                    if let Some(tints) = biome_tints.as_mut() {
                        crate::runtime::world::synchronize_biome_tints(stream, tints);
                    }
                    if entities_changed {
                        stream.set_pack_entities(packs.entities.as_ref().map(|pack| {
                            (
                                pack.assets.clone(),
                                pack.bindings
                                    .iter()
                                    .map(|binding| binding.geometry_candidate)
                                    .collect(),
                            )
                        }));
                    }
                }
                if let Some(textures) = textures.as_mut()
                    && !Arc::ptr_eq(textures.assets(), &prepared.assets)
                {
                    **textures = candidate;
                }
                world.pack_entities = packs.entities.clone();
                world.prepared_actor_artwork = packs.prepared_actor_artwork.clone();
                if items_changed {
                    world.session_items = Some(Arc::new(super::entity_pack::SessionItems {
                        components: packs.item_components.clone().unwrap_or_default(),
                        icons: packs.item_icons.clone(),
                    }));
                }
                ui.set_server_lang(packs.server_lang);
                ui.set_session_icons(packs.item_icons);
                ui.set_session_items(packs.item_components);
                ui.set_session_glyphs(packs.glyph_sheets);
                ui.set_server_ui(packs.server_ui);
                if sounds_changed {
                    crate::audio::publish_server_sounds(packs.server_sounds);
                }
                reload.applied = prepared.revision;
                reload.last_duration = Some(prepared.elapsed);
                reload.error = None;
            }
            Some(Ok(_)) | None => {}
            Some(Err(error)) => {
                if let (Some(gpu), Some(current)) = (gpu_reload.as_ref(), textures.as_ref()) {
                    gpu.cancel_except(current.identity());
                }
                reload.error = Some(error);
                reload.applied = reload.revision;
            }
        }
    }
    if reload.pending.is_none() && reload.ready.is_none() && reload.applied != reload.revision {
        reload.start(
            world.runtime_assets.clone(),
            presentation
                .as_deref()
                .and_then(|presentation| presentation.pack_catalog_base()),
            environment_base.as_deref().cloned(),
            entity_artwork.as_deref().cloned(),
        );
    }
}

/// Shared compiled resources retain their identity when their source files are unchanged.
fn same_optional<T>(left: &Option<Arc<T>>, right: &Option<Arc<T>>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => Arc::ptr_eq(left, right),
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
#[path = "pack_reload_failures.rs"]
mod failures;
