#[cfg(target_arch = "wasm32")]
use super::compile_execution::scope;
#[cfg(not(target_arch = "wasm32"))]
use rayon::scope;
use std::sync::Arc;

use resource_pack::{LayeredPackView, PackAdmission};

use super::{
    block_overlay::{CompiledBlockOverlay, compile_block_overlay},
    glyph_sheets::compile_session_glyphs,
    item_icons::{BlockIcons, compile_session_icons, custom_block_icons_with_tint},
};
use client_ui::ui_runtime::presentation::{ServerUiPack, SessionGlyphSheets, SessionIcons};

pub(super) mod reuse;
mod ui;
pub(super) mod ui_catalog;
use ui::collect_server_ui;

#[cfg(feature = "developer-control")]
mod capture;

/// Everything the session applies from its server pack stack.
#[derive(Clone, Debug)]
pub struct PackApplication {
    /// One-shot block state prepared after reusable pack compilation.
    pub(super) registries: Option<super::session_registries::PreparedSessionRegistries>,
    pub(super) dependencies: pack_runtime::reload_diff::Dependencies,
    /// Finalized by the join or reload worker after all terrain reads have been recorded.
    pub(super) terrain_identity: [u8; 32],
    /// Inert metadata only; accepting a pack does not authorize execution.
    pub(crate) extension_marker: Option<Arc<[u8]>>,
    pub(crate) admission: PackAdmission,
    pub(super) inputs: Arc<pack_runtime::PackInputs>,
    pub(crate) server_lang: Option<Arc<assets::ServerLangOverlay>>,
    pub(crate) block_overlay: Option<Arc<CompiledBlockOverlay>>,
    pub(crate) item_icons: Option<Arc<SessionIcons>>,
    /// StartGame item components, applied with the pack stack they present through.
    pub(crate) item_components:
        Option<Arc<client_ui::ui_runtime::item_facts::SessionItemComponents>>,
    pub(crate) glyph_sheets: Option<Arc<SessionGlyphSheets>>,
    pub(crate) block_entity_art: Option<Arc<render_model::PreparedBlockEntityArtwork>>,
    pub(crate) entities: Option<Arc<assets::SessionEntityPack>>,
    pub(super) entity_artwork: Option<Arc<render_model::ActorArtworkPages>>,
    pub(crate) prepared_actor_artwork:
        Option<Arc<client_presentation::prepared_actor_artwork::PreparedActorArtwork>>,
    pub(crate) property_defaults: Vec<(Arc<str>, Vec<client_world::PropertyDefault>)>,
    pub(crate) server_ui: Option<Arc<ServerUiPack>>,
    /// Installed only once the session's Bootstrap is accepted.
    pub(crate) server_sounds: Option<Arc<client_audio::audio::ServerSoundPack>>,
    pub(crate) aim_assist_textures: [Option<Arc<render_scene::AimAssistTexture>>; 2],
}

impl Default for PackApplication {
    fn default() -> Self {
        Self {
            registries: None,
            dependencies: Default::default(),
            terrain_identity: [0; 32],
            extension_marker: None,
            admission: PackAdmission::None,
            inputs: Arc::default(),
            server_lang: None,
            block_overlay: None,
            item_icons: None,
            item_components: None,
            glyph_sheets: None,
            block_entity_art: super::block_entity_art::base().cloned(),
            entities: None,
            entity_artwork: None,
            prepared_actor_artwork: None,
            property_defaults: Vec::new(),
            server_ui: None,
            server_sounds: None,
            aim_assist_textures: Default::default(),
        }
    }
}

impl PackApplication {
    /// Resolves initial pack UI on the session worker before its first frame can use it.
    pub(super) fn prepare_ui_catalog(&mut self, base: Option<&Arc<json_ui::Catalog>>) {
        if let Some((source, base)) = self.server_ui.as_ref().zip(base) {
            self.server_ui = Some(ui_catalog::prepare(source, base));
        }
    }

    /// Prepares against the exact base snapshot that this application will publish over.
    pub(super) fn prepare_actor_artwork(
        &mut self,
        base: Option<&render_model::ActorArtworkPages>,
        blocks: Option<Arc<client_presentation::presentation::equipment::PreparedBlockItems>>,
    ) {
        let Some(base) = base.filter(|_| {
            self.entities.is_some() || self.block_entity_art.is_some() || blocks.is_some()
        }) else {
            self.prepared_actor_artwork = None;
            return;
        };
        if let Some(previous) = &self.prepared_actor_artwork
            && previous.matches_sources(
                base,
                self.entities.as_ref(),
                self.block_entity_art.as_ref(),
            )
            && previous.matches_block_items(blocks.as_ref())
        {
            return;
        }
        self.prepared_actor_artwork = Some(Arc::new(
            client_presentation::prepared_actor_artwork::PreparedActorArtwork::new_with_block_items(
                base,
                self.entities.as_ref(),
                self.block_entity_art.as_ref(),
                blocks,
            ),
        ));
    }
}

#[cfg(test)]
use pack_runtime::required_packs_applied;

/// The carrier snapshots a join's presentation is prepared over on the session worker.
#[derive(Clone, Copy, Default)]
pub(super) struct JoinBases<'a> {
    pub(super) actor_artwork: Option<&'a render_model::ActorArtworkPages>,
    pub(super) ui_catalog: Option<&'a Arc<json_ui::Catalog>>,
}

/// Prepares a validated join on idle world cores, reusing an equivalent recent stack.
/// Keeps the compiled result for the next join; returns None after cancellation.
pub(super) fn prepare_join(
    preparation: &pack_runtime::PackPreparation,
    game_data: &protocol::GameData,
    cancelled: &(dyn Fn() -> bool + Sync),
    bases: JoinBases<'_>,
) -> Option<Result<PackApplication, pack_runtime::RequiredPackRejected>> {
    let profile = bridge::join_profile::JoinProfile::current();
    let prepare = || {
        profile.run(|| prepare_join_with(&reuse::LATEST, preparation, game_data, cancelled, bases))
    };
    #[cfg(not(target_arch = "wasm32"))]
    if preparation.has_applied_packs() {
        return chunk_pipeline::on_idle_world_cores(prepare);
    }
    prepare()
}

/// [`prepare_join`] against an explicit set of kept stacks.
fn prepare_join_with(
    kept: &reuse::CompiledStacks,
    preparation: &pack_runtime::PackPreparation,
    game_data: &protocol::GameData,
    cancelled: &(dyn Fn() -> bool + Sync),
    bases: JoinBases<'_>,
) -> Option<Result<PackApplication, pack_runtime::RequiredPackRejected>> {
    let environment = reuse::CompileEnvironment::current();
    let packs = preparation.prepare_application(
        |preparation| match &preparation.admission {
            PackAdmission::None => Some(PackApplication {
                inputs: Arc::clone(&preparation.inputs),
                ..Default::default()
            }),
            PackAdmission::Validated(stack) => reuse::compile_reusing(
                kept,
                Arc::clone(stack),
                Arc::clone(&preparation.inputs),
                &environment,
                cancelled,
            ),
        },
        |packs| {
            packs.item_components =
                client_ui::ui_runtime::item_facts::SessionItemComponents::from_game_data(game_data);
        },
    )?;
    Some(packs.map(|mut packs| {
        if let PackAdmission::Validated(stack) = &packs.admission {
            packs.terrain_identity = terrain_identity(stack, &packs.dependencies);
        }
        packs.prepare_actor_artwork(bases.actor_artwork, None);
        let source_ui = packs.server_ui.clone();
        packs.prepare_ui_catalog(bases.ui_catalog);
        let now = reuse::CompileEnvironment::current();
        kept.keep_join(environment, &now, &packs, source_ui, cancelled);
        packs
    }))
}

/// The recently compiled stacks joins and the post-join reload share.
pub(crate) fn compiled_stacks() -> &'static reuse::CompiledStacks {
    &reuse::LATEST
}

/// `block_items` pairs each custom block item with the block it draws as.
#[cfg(test)]
pub(super) fn prepare_pack_application(
    handoff: protocol::ResourcePackHandoff,
    custom_blocks: &protocol::CustomBlocks,
    icon_keys: &[(Arc<str>, Arc<str>)],
    block_items: &[(Arc<str>, Arc<str>)],
    hashed_block_ids: bool,
) -> PackApplication {
    let stack = resource_pack::validate_handoff(handoff);
    let inputs = Arc::new(pack_runtime::PackInputs {
        blocks: custom_blocks.clone(),
        icons: icon_keys.to_vec(),
        block_items: block_items.to_vec(),
        hashed: hashed_block_ids,
    });
    if stack.packs().is_empty() && stack.rejections().is_empty() {
        return PackApplication {
            inputs,
            ..Default::default()
        };
    }
    prepare_validated_application(stack, inputs)
}

/// Compiles an already admitted optional stack for a menu or an existing world.
#[cfg(test)]
pub(super) fn prepare_validated_application(
    stack: Arc<resource_pack::ValidatedPackStack>,
    inputs: Arc<pack_runtime::PackInputs>,
) -> PackApplication {
    prepare_changed_application(&reuse::CompiledStacks::new(), stack, inputs, None)
}

/// Diffs `stack` against `previous` and prepares it as [`prepare_diffed_application`] does.
#[cfg(test)]
pub(super) fn prepare_changed_application(
    kept: &reuse::CompiledStacks,
    stack: Arc<resource_pack::ValidatedPackStack>,
    inputs: Arc<pack_runtime::PackInputs>,
    previous: Option<&PackApplication>,
) -> PackApplication {
    let previous = previous.map(|previous| {
        let changes = pack_runtime::reload_diff::Changes::between(
            &stack,
            Some((&previous.admission, &previous.dependencies)),
        );
        (previous, changes)
    });
    prepare_diffed_application(kept, stack, inputs, previous)
}

/// Reuses each subscriber of the previous application that `changes`, taken against it, leaves
/// unchanged. Without a previous application, a `kept` stack with the same contents stands in.
pub(super) fn prepare_diffed_application(
    kept: &reuse::CompiledStacks,
    stack: Arc<resource_pack::ValidatedPackStack>,
    inputs: Arc<pack_runtime::PackInputs>,
    previous: Option<(&PackApplication, pack_runtime::reload_diff::Changes)>,
) -> PackApplication {
    let compiled = match previous {
        Some((previous, changes)) => {
            compile_application(stack, inputs, Some(previous), changes, &|| false)
        }
        None => reuse::compile_reusing(
            kept,
            stack,
            inputs,
            &reuse::CompileEnvironment::current(),
            &|| false,
        ),
    };
    compiled.expect("an uncancellable compile completes")
}

/// Compiles changed subscribers concurrently, keeping the rest; icons follow their block thumbnails.
/// Cancellation skips unstarted work and returns None.
fn compile_application(
    stack: Arc<resource_pack::ValidatedPackStack>,
    inputs: Arc<pack_runtime::PackInputs>,
    previous: Option<&PackApplication>,
    changes: pack_runtime::reload_diff::Changes,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<PackApplication> {
    use pack_runtime::reload_diff::{Subscriber, compile_part};
    let mut dependencies = previous
        .map(|old| old.dependencies.clone())
        .unwrap_or_default();
    let custom_blocks = &inputs.blocks;
    let icon_keys = &inputs.icons;
    let hashed_block_ids = inputs.hashed;
    for rejection in stack.rejections() {
        bevy::log::warn!(
            stack_index = rejection.stack_index,
            reason = %rejection.reason,
            "server resource pack dropped"
        );
    }
    let max_layers = overlay_texture_layer_limit();
    let view = LayeredPackView::new(Arc::clone(&stack));
    // Hashing every archive is needed only by the block and entity caches, so only when one of
    // those subscribers compiles; whichever needs it first computes it.
    let fingerprint = std::sync::OnceLock::new();
    let fingerprint = || fingerprint.get_or_init(|| stack_fingerprint(&stack));
    let (mut blocks, mut icons, mut language, mut glyphs) = (None, None, None, None);
    let (mut entities, mut artwork, mut ui, mut sounds) = (None, None, None, None);
    let mut aim_assist = None;
    let mut block_entity_art = None;
    // The entity compile checks cancellation between its own steps, as it outlasts the rest.
    let mut entities_stopped = false;
    let profile = bridge::join_profile::JoinProfile::current();
    scope(|scope| {
        scope.spawn(|_| {
            let _binding = profile.enter();
            block_entity_art = compile_part(
                &stack,
                cancelled,
                changes.block_entity_art,
                Subscriber::BlockEntityArt,
                previous.and_then(|old| old.block_entity_art.clone()),
                super::block_entity_art::prepare,
            );
        });
        scope.spawn(|_| {
            let _binding = profile.enter();
            aim_assist = compile_part(
                &stack,
                cancelled,
                changes.aim_assist,
                Subscriber::AimAssist,
                previous
                    .map(|old| old.aim_assist_textures.clone())
                    .unwrap_or_default(),
                crate::camera::aim_highlight::prepare_pack_textures,
            );
        });
        scope.spawn(|_| {
            let _binding = profile.enter();
            blocks = compile_part(
                &stack,
                cancelled,
                changes.blocks,
                Subscriber::Blocks,
                previous.and_then(|old| old.block_overlay.clone()),
                |view| {
                    cached_block_overlay(
                        fingerprint(),
                        view,
                        custom_blocks,
                        hashed_block_ids,
                        max_layers,
                        || {
                            compile_block_overlay(
                                view,
                                custom_blocks,
                                hashed_block_ids,
                                BASE_MATERIAL_KEYS.get().map(|(keys, _)| keys),
                                max_layers,
                                base_world_assets(),
                            )
                            .map(Arc::new)
                        },
                    )
                },
            );
            let Some((overlay, _)) = &blocks else { return };
            if let Some(compiled) = overlay
                && compiled.gaps != Default::default()
            {
                bevy::log::warn!(gaps = ?compiled.gaps, "server block visuals are incomplete");
            }
            icons = compile_icons(
                &stack,
                cancelled,
                changes.icons || changes.blocks,
                previous.and_then(|old| old.item_icons.clone()),
                icon_keys,
                |view| {
                    let item_overlay = super::block_overlay::compile_block_item_overlay(
                        view,
                        custom_blocks,
                        BASE_MATERIAL_KEYS.get().map(|(keys, _)| keys),
                        max_layers,
                        base_world_assets(),
                    );
                    let grass = super::item_icons::carried_grass_tint(
                        view,
                        BASE_MATERIAL_KEYS.get().map(|(keys, _)| keys),
                    );
                    let mut block_icons = if let Some((compiled, definitions)) = &item_overlay {
                        custom_block_icons_with_tint(
                            &compiled.overlay,
                            definitions,
                            false,
                            &inputs.block_items,
                            base_world_assets(),
                            grass,
                        )
                    } else {
                        overlay
                            .as_deref()
                            .map_or_else(BlockIcons::default, |compiled| {
                                custom_block_icons_with_tint(
                                    &compiled.overlay,
                                    custom_blocks,
                                    hashed_block_ids,
                                    &inputs.block_items,
                                    base_world_assets(),
                                    grass,
                                )
                            })
                    };
                    if let (Some(compiled), Some((_, base))) =
                        (overlay.as_deref(), BASE_MATERIAL_KEYS.get())
                    {
                        super::item_icons::vanilla_blocks::append(base, compiled, &mut block_icons);
                    }
                    block_icons
                },
            );
        });
        scope.spawn(|_| {
            let _binding = profile.enter();
            language = compile_part(
                &stack,
                cancelled,
                changes.language,
                Subscriber::Language,
                previous.and_then(|old| old.server_lang.clone()),
                merged_server_lang,
            );
        });
        scope.spawn(|_| {
            let _binding = profile.enter();
            glyphs = compile_part(
                &stack,
                cancelled,
                changes.glyphs,
                Subscriber::Glyphs,
                previous.and_then(|old| old.glyph_sheets.clone()),
                compile_session_glyphs,
            );
        });
        scope.spawn(|_| {
            let _binding = profile.enter();
            entities = compile_part(
                &stack,
                cancelled,
                changes.entities,
                Subscriber::Entities,
                previous.and_then(|old| old.entities.clone()),
                |view| {
                    let compiled = pack_runtime::entity_pack::compile_session_entities(
                        fingerprint(),
                        view,
                        cancelled,
                    );
                    entities_stopped = compiled.is_none();
                    compiled.flatten()
                },
            );
        });
        scope.spawn(|_| {
            let _binding = profile.enter();
            artwork = compile_part(
                &stack,
                cancelled,
                changes.entities,
                Subscriber::Entities,
                previous.and_then(|old| old.entity_artwork.clone()),
                super::entity_texture_reload::prepare,
            );
        });
        scope.spawn(|_| {
            let _binding = profile.enter();
            ui = compile_part(
                &stack,
                cancelled,
                changes.ui,
                Subscriber::Ui,
                previous.and_then(|old| old.server_ui.clone()),
                collect_server_ui,
            );
        });
        scope.spawn(|_| {
            let _binding = profile.enter();
            sounds = compile_part(
                &stack,
                cancelled,
                changes.sounds,
                Subscriber::Sounds,
                previous.and_then(|old| old.server_sounds.clone()),
                |view| client_audio::audio::ServerSoundPack::from_view(view).map(Arc::new),
            );
        });
    });
    fn record<T>(
        dependencies: &mut pack_runtime::reload_diff::Dependencies,
        subscriber: Subscriber,
        (output, inputs): (
            T,
            Option<std::collections::BTreeSet<resource_pack::PackDependency>>,
        ),
    ) -> T {
        if let Some(inputs) = inputs {
            dependencies.insert(subscriber, inputs);
        }
        output
    }
    let block_entity_art = record(
        &mut dependencies,
        Subscriber::BlockEntityArt,
        block_entity_art?,
    );
    let aim_assist_textures = record(&mut dependencies, Subscriber::AimAssist, aim_assist?);
    let block_overlay = record(&mut dependencies, Subscriber::Blocks, blocks?);
    let item_icons = record(&mut dependencies, Subscriber::Icons, icons?);
    let server_lang = record(&mut dependencies, Subscriber::Language, language?);
    let glyph_sheets = record(&mut dependencies, Subscriber::Glyphs, glyphs?);
    let server_ui = record(&mut dependencies, Subscriber::Ui, ui?);
    let server_sounds = record(&mut dependencies, Subscriber::Sounds, sounds?);
    let entities = record(&mut dependencies, Subscriber::Entities, entities?);
    // Artwork reads join the entity subscriber's rather than replacing them.
    let (entity_artwork, artwork_inputs) = artwork?;
    if let Some(inputs) = artwork_inputs {
        dependencies
            .entry(Subscriber::Entities)
            .or_default()
            .extend(inputs);
    }
    let terrain_identity = previous.map_or([0; 32], |previous| previous.terrain_identity);
    client_runtime::item_diagnostics::session_icons(icon_keys, item_icons.as_deref());
    if entities_stopped || cancelled() {
        return None;
    }
    #[cfg(feature = "developer-control")]
    capture::write(&view, server_ui.as_deref());
    // Whole-stack facts read every layer, so they carry over only from identical contents.
    let (extension_marker, property_defaults) = match previous {
        Some(old) if !changes.contents => {
            (old.extension_marker.clone(), old.property_defaults.clone())
        }
        _ => (
            view.read_capped(
                server_experience::policy::MARKER_PATH,
                server_experience::policy::MAX_MARKER_BYTES as u64,
            )
            .map(Arc::from),
            super::entity_pack::pack_property_defaults(&view),
        ),
    };
    Some(PackApplication {
        registries: None,
        extension_marker,
        property_defaults,
        prepared_actor_artwork: previous.and_then(|old| old.prepared_actor_artwork.clone()),
        inputs,
        server_lang,
        item_icons,
        item_components: None,
        glyph_sheets,
        block_entity_art,
        entities,
        entity_artwork,
        server_ui,
        server_sounds,
        aim_assist_textures,
        admission: PackAdmission::Validated(stack),
        block_overlay,
        dependencies,
        terrain_identity,
    })
}

/// Separates cached terrain colors using the base pack and the block compiler's tracked reads.
pub(super) fn terrain_identity(
    stack: &resource_pack::ValidatedPackStack,
    dependencies: &pack_runtime::reload_diff::Dependencies,
) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let inputs = &dependencies[&pack_runtime::reload_diff::Subscriber::Blocks];
    Sha256::new()
        .chain_update(assets::vanilla_source_manifest_sha256())
        .chain_update(pack_runtime::reload_diff::dependency_fingerprint(
            stack, inputs,
        ))
        .finalize()
        .into()
}

pub(super) fn install_server_ui(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    generation: u64,
    pack: Option<Arc<ServerUiPack>>,
    setup_succeeded: bool,
) {
    if runtime.session_id() == generation {
        runtime.set_server_ui(pack.filter(|_| setup_succeeded));
    }
}

/// The renderer's requested device limit, installed before a session can compile packs.
static OVERLAY_TEXTURE_LAYER_LIMIT: std::sync::OnceLock<std::sync::OnceLock<u32>> =
    std::sync::OnceLock::new();

/// Makes direct-connect pack workers wait until startup publishes the actual device limit.
pub(crate) fn await_overlay_texture_device() {
    OVERLAY_TEXTURE_LAYER_LIMIT.get_or_init(std::sync::OnceLock::new);
}

/// Publishes the active device's array limit to pack preparation workers at startup.
pub(crate) fn initialize_overlay_texture_limit(
    device: Option<bevy::prelude::Res<bevy::render::renderer::RenderDevice>>,
    memory: Option<bevy::prelude::Res<crate::host_memory::HostMemory>>,
) {
    let max_dimension_2d = device.as_ref().map_or_else(
        || wgpu::Limits::default().max_texture_dimension_2d,
        |device| device.limits().max_texture_dimension_2d,
    );
    assets::texture::policy::set_device_policy(max_dimension_2d);
    assets::texture::policy::set_terrain_device_policy(
        max_dimension_2d,
        resource_pack::subpacks::device_memory_tier(memory.map_or_else(
            || crate::host_memory::HostMemory::default().0,
            |memory| memory.0,
        )),
    );
    let limit = device.map_or_else(
        || wgpu::Limits::default().max_texture_array_layers,
        |device| device.limits().max_texture_array_layers,
    );
    let _ = OVERLAY_TEXTURE_LAYER_LIMIT
        .get_or_init(std::sync::OnceLock::new)
        .set(limit);
}

/// Waits for a requested device; fixtures without renderer setup use conservative limits.
pub(super) fn overlay_texture_layer_limit() -> u32 {
    let limit = OVERLAY_TEXTURE_LAYER_LIMIT.get().map_or_else(
        || wgpu::Limits::default().max_texture_array_layers,
        |limit| *limit.wait(),
    );
    limit.min(assets::MAX_TEXTURE_LAYERS as u32)
}

/// Texture keys of the vanilla carrier's materials, set once at startup when the sidecar loads.
static BASE_MATERIAL_KEYS: std::sync::OnceLock<(
    assets::material_keys::MaterialKeys,
    Arc<assets::RuntimeAssets>,
)> = std::sync::OnceLock::new();

static BASE_WORLD_ASSETS: std::sync::OnceLock<Arc<assets::RuntimeAssets>> =
    std::sync::OnceLock::new();

/// Keeps material admission bounded by the world table even when its optional sidecar is absent.
pub(crate) fn set_base_world_assets(base: Arc<assets::RuntimeAssets>) {
    let _ = BASE_WORLD_ASSETS.set(base);
}

/// Uses the sidecar's matching world, or the startup world when no sidecar was available.
fn base_world_assets() -> Option<&'static assets::RuntimeAssets> {
    BASE_MATERIAL_KEYS
        .get()
        .map(|(_, base)| base.as_ref())
        .or_else(|| BASE_WORLD_ASSETS.get().map(AsRef::as_ref))
}

/// Installs immutable source facts and pixels from the same world carrier.
pub(crate) fn set_base_material_keys(
    keys: assets::material_keys::MaterialKeys,
    base: Arc<assets::RuntimeAssets>,
) {
    let _ = BASE_MATERIAL_KEYS.set((keys, base));
}

/// Icon output and optional dependency reads returned by the subscriber gate.
type CompiledIcons = Option<(
    Option<Arc<SessionIcons>>,
    Option<std::collections::BTreeSet<resource_pack::PackDependency>>,
)>;

/// Bakes block art only when the icon subscriber compiles, preserving reused icon outputs.
fn compile_icons(
    stack: &Arc<resource_pack::ValidatedPackStack>,
    cancelled: &(dyn Fn() -> bool + Sync),
    changed: bool,
    reuse: Option<Arc<SessionIcons>>,
    icon_keys: &[(Arc<str>, Arc<str>)],
    bake_blocks: impl FnOnce(&LayeredPackView) -> BlockIcons,
) -> CompiledIcons {
    pack_runtime::reload_diff::compile_part(
        stack,
        cancelled,
        changed,
        pack_runtime::reload_diff::Subscriber::Icons,
        reuse,
        |view| compile_session_icons(view, icon_keys, bake_blocks(view)),
    )
}

use pack_runtime::pack_language::merged_server_lang;

use pack_runtime::{StackFingerprint, stack_fingerprint};

struct CachedOverlay {
    dependencies: Option<std::collections::BTreeSet<resource_pack::PackDependency>>,
    stack: StackFingerprint,
    hashed: bool,
    max_layers: u32,
    blocks: protocol::CustomBlocks,
    overlay: Option<Arc<CompiledBlockOverlay>>,
}

/// The current and previous stacks, so switching packs back reuses the earlier compile.
const KEPT_OVERLAYS: usize = 2;

/// Recently compiled overlays, newest first, reused when the same pack contents and block
/// definitions return in a later session or reload.
static OVERLAY_CACHE: std::sync::Mutex<std::collections::VecDeque<CachedOverlay>> =
    std::sync::Mutex::new(std::collections::VecDeque::new());

/// Reuses compiled blocks with the fingerprint already computed for this admission.
fn cached_block_overlay(
    fingerprint: &StackFingerprint,
    view: &LayeredPackView,
    blocks: &protocol::CustomBlocks,
    hashed: bool,
    max_layers: u32,
    compile: impl FnOnce() -> Option<Arc<CompiledBlockOverlay>>,
) -> Option<Arc<CompiledBlockOverlay>> {
    let mut cache = OVERLAY_CACHE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if let Some(index) = cache.iter().position(|cached| {
        cached.hashed == hashed
            && cached.max_layers == max_layers
            && cached.stack == *fingerprint
            && cached.blocks == *blocks
            && (view.dependencies().is_none() || cached.dependencies.is_some())
    }) {
        let cached = cache.remove(index).expect("matched entry");
        if let (Some(dependencies), Some(inputs)) = (view.dependencies(), &cached.dependencies) {
            dependencies.extend(inputs.clone());
        }
        let overlay = cached.overlay.clone();
        cache.push_front(cached);
        return overlay;
    }
    let overlay = compile();
    cache.push_front(CachedOverlay {
        dependencies: view
            .dependencies()
            .map(|dependencies| dependencies.snapshot()),
        stack: fingerprint.clone(),
        hashed,
        max_layers,
        blocks: blocks.clone(),
        overlay: overlay.clone(),
    });
    cache.truncate(KEPT_OVERLAYS);
    overlay
}

/// Installs the materialized custom prefix while leaving the rest of the palette unbacked.
/// Returns the base carrier when there is no overlay or its visuals exceed the custom range.
pub(super) fn session_runtime_assets(
    base: &Arc<assets::RuntimeAssets>,
    custom_ids: Option<&std::ops::Range<u32>>,
    compiled: Option<&Arc<CompiledBlockOverlay>>,
) -> Arc<assets::RuntimeAssets> {
    let Some(compiled) = compiled else {
        return Arc::clone(base);
    };
    let empty_ids = base.visual_count() as u32..base.visual_count() as u32;
    let ids = custom_ids.unwrap_or(&empty_ids);
    if compiled.overlay.visuals.len() > ids.len() {
        bevy::log::warn!("server block visuals exceed the custom block ids");
        return Arc::clone(base);
    }
    static LAST: OverlaidCarrier = OverlaidCarrier::new();
    match LAST.extend(base, ids.start, compiled) {
        Ok(assets) => assets,
        Err(error) => {
            bevy::log::warn!(%error, "server block visuals were not applied");
            Arc::clone(base)
        }
    }
}

/// Weakly retains the last block-overlay carrier while a session or renderer owns it.
/// Equivalent live inputs share that carrier and its rendered atlas.
struct OverlaidCarrier(std::sync::Mutex<Option<Overlaid>>);

struct Overlaid {
    base: std::sync::Weak<assets::RuntimeAssets>,
    overlay: std::sync::Weak<CompiledBlockOverlay>,
    first_id: u32,
    assets: std::sync::Weak<assets::RuntimeAssets>,
}

impl OverlaidCarrier {
    /// Creates an empty weak cache for shared block overlays.
    const fn new() -> Self {
        Self(std::sync::Mutex::new(None))
    }

    /// Extends base with compiled blocks from first_id, sharing equal immutable live inputs.
    /// The cache lock is released before extending the carrier.
    fn extend(
        &self,
        base: &Arc<assets::RuntimeAssets>,
        first_id: u32,
        compiled: &Arc<CompiledBlockOverlay>,
    ) -> Result<Arc<assets::RuntimeAssets>, assets::AssetError> {
        let lock = || self.0.lock().unwrap_or_else(|poison| poison.into_inner());
        if let Some(last) = lock().as_ref()
            && last.first_id == first_id
            && last
                .base
                .upgrade()
                .is_some_and(|last| Arc::ptr_eq(&last, base))
            && last
                .overlay
                .upgrade()
                .is_some_and(|last| Arc::ptr_eq(&last, compiled))
            && let Some(assets) = last.assets.upgrade()
        {
            return Ok(assets);
        }
        let assets = Arc::new(base.with_block_overlay(first_id, &compiled.overlay)?);
        *lock() = Some(Overlaid {
            base: Arc::downgrade(base),
            overlay: Arc::downgrade(compiled),
            first_id,
            assets: Arc::downgrade(&assets),
        });
        Ok(assets)
    }
}

/// Points the chunk renderer at the session's assets. Every switch takes a
/// fresh revision so the GPU tables re-upload even if an allocation is reused.
pub(super) fn install_chunk_textures(
    textures: &mut render_scene::ChunkTextureAssets,
    assets: &Arc<assets::RuntimeAssets>,
) {
    static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    if !Arc::ptr_eq(textures.assets(), assets) {
        let revision = REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        *textures = render_scene::ChunkTextureAssets::with_revision(Arc::clone(assets), revision);
    }
}

pub(super) fn install_session_icons(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    generation: u64,
    icons: Option<Arc<SessionIcons>>,
    items: Option<Arc<client_ui::ui_runtime::item_facts::SessionItemComponents>>,
    setup_succeeded: bool,
) {
    if runtime.session_id() == generation {
        runtime.set_session_icons(icons.filter(|_| setup_succeeded));
        runtime.set_session_items(items.filter(|_| setup_succeeded));
    }
}

pub(super) fn install_session_glyphs(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    generation: u64,
    glyphs: Option<Arc<SessionGlyphSheets>>,
    setup_succeeded: bool,
) {
    if runtime.session_id() == generation {
        runtime.set_session_glyphs(glyphs.filter(|_| setup_succeeded));
    }
}

pub(super) fn install_server_language(
    runtime: &mut client_ui::ui_runtime::UiRuntime,
    generation: u64,
    overlay: Option<std::sync::Arc<assets::ServerLangOverlay>>,
    setup_succeeded: bool,
) {
    if runtime.session_id() == generation {
        runtime.set_server_lang(if setup_succeeded { overlay } else { None });
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod fingerprint_bench;
