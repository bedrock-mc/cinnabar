use std::sync::Arc;

use bevy::prelude::Resource;
use resource_pack::{LayeredPackView, PackAdmission};

use super::{
    block_overlay::{CompiledBlockOverlay, compile_block_overlay},
    glyph_sheets::compile_session_glyphs,
    item_icons::{BlockIcons, compile_session_icons, custom_block_icons},
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
    pub(super) dependencies: super::pack_reload_diff::Dependencies,
    /// Inert metadata only; accepting a pack does not authorize execution.
    pub(crate) extension_marker: Option<Arc<[u8]>>,
    pub(crate) admission: PackAdmission,
    pub(super) inputs: Arc<super::pack_reload::PackInputs>,
    pub(crate) server_lang: Option<Arc<assets::ServerLangOverlay>>,
    pub(crate) block_overlay: Option<Arc<CompiledBlockOverlay>>,
    pub(crate) item_icons: Option<Arc<SessionIcons>>,
    /// StartGame item components, applied with the pack stack they present through.
    pub(crate) item_components:
        Option<Arc<client_ui::ui_runtime::item_facts::SessionItemComponents>>,
    pub(crate) glyph_sheets: Option<Arc<SessionGlyphSheets>>,
    pub(crate) entities: Option<Arc<super::entity_pack::SessionEntityPack>>,
    pub(super) entity_artwork: Option<Arc<render::ActorArtworkPages>>,
    pub(crate) prepared_actor_artwork:
        Option<Arc<client_presentation::prepared_actor_artwork::PreparedActorArtwork>>,
    pub(crate) property_defaults: Vec<(Arc<str>, Vec<client_world::PropertyDefault>)>,
    pub(crate) server_ui: Option<Arc<ServerUiPack>>,
    /// Installed only once the session's Bootstrap is accepted.
    pub(crate) server_sounds: Option<Arc<crate::audio::ServerSoundPack>>,
    pub(crate) aim_assist_textures: [Option<Arc<render::AimAssistTexture>>; 2],
}

impl Default for PackApplication {
    fn default() -> Self {
        Self {
            dependencies: Default::default(),
            extension_marker: None,
            admission: PackAdmission::None,
            inputs: Arc::default(),
            server_lang: None,
            block_overlay: None,
            item_icons: None,
            item_components: None,
            glyph_sheets: None,
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
    pub(super) fn prepare_actor_artwork(&mut self, base: Option<&render::ActorArtworkPages>) {
        self.prepared_actor_artwork = base.zip(self.entities.as_ref()).map(|(base, pack)| {
            if let Some(previous) = &self.prepared_actor_artwork
                && previous.pages_for(base, pack).is_some()
            {
                return previous.clone();
            }
            Arc::new(
                client_presentation::prepared_actor_artwork::PreparedActorArtwork::new(base, pack),
            )
        });
    }
}

#[cfg(test)]
use client_session::required_packs_applied;

/// The carrier snapshots a join's presentation is prepared over on the session worker.
#[derive(Clone, Copy, Default)]
pub(super) struct JoinBases<'a> {
    pub(super) actor_artwork: Option<&'a render::ActorArtworkPages>,
    pub(super) ui_catalog: Option<&'a Arc<json_ui::Catalog>>,
}

/// Prepares a validated join on idle world cores, reusing an equivalent recent stack.
/// Keeps the compiled result for the next join; returns None after cancellation.
pub(super) fn prepare_join(
    preparation: &client_session::PackPreparation,
    game_data: &protocol::GameData,
    cancelled: &(dyn Fn() -> bool + Sync),
    bases: JoinBases<'_>,
) -> Option<Result<PackApplication, client_session::RequiredPackRejected>> {
    let prepare = || prepare_join_with(&reuse::LATEST, preparation, game_data, cancelled, bases);
    if preparation.has_applied_packs() {
        chunk_pipeline::on_idle_world_cores(prepare)
    } else {
        prepare()
    }
}

/// [`prepare_join`] against an explicit set of kept stacks.
fn prepare_join_with(
    kept: &reuse::CompiledStacks,
    preparation: &client_session::PackPreparation,
    game_data: &protocol::GameData,
    cancelled: &(dyn Fn() -> bool + Sync),
    bases: JoinBases<'_>,
) -> Option<Result<PackApplication, client_session::RequiredPackRejected>> {
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
        packs.prepare_actor_artwork(bases.actor_artwork);
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
    let inputs = Arc::new(super::pack_reload::PackInputs {
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
    inputs: Arc<super::pack_reload::PackInputs>,
) -> PackApplication {
    prepare_changed_application(&reuse::CompiledStacks::new(), stack, inputs, None)
}

/// Reuses each compiled subscriber whose contributing files have not changed. Without a
/// previous application, a `kept` stack with the same contents stands in for it.
pub(super) fn prepare_changed_application(
    kept: &reuse::CompiledStacks,
    stack: Arc<resource_pack::ValidatedPackStack>,
    inputs: Arc<super::pack_reload::PackInputs>,
    previous: Option<&PackApplication>,
) -> PackApplication {
    let compiled = match previous {
        Some(previous) => {
            let changes = super::pack_reload_diff::Changes::between(&stack, Some(previous));
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
    inputs: Arc<super::pack_reload::PackInputs>,
    previous: Option<&PackApplication>,
    changes: super::pack_reload_diff::Changes,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Option<PackApplication> {
    use super::pack_reload_diff::{Subscriber, compile_part};
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
    let view = LayeredPackView::new(Arc::clone(&stack));
    // Hashing every archive is needed only by the block and entity caches, so only when one of
    // those subscribers compiles; whichever needs it first computes it.
    let fingerprint = std::sync::OnceLock::new();
    let fingerprint = || fingerprint.get_or_init(|| stack_fingerprint(&stack));
    let (mut blocks, mut icons, mut language, mut glyphs) = (None, None, None, None);
    let (mut entities, mut artwork, mut ui, mut sounds) = (None, None, None, None);
    let mut aim_assist = None;
    // The entity compile checks cancellation between its own steps, as it outlasts the rest.
    let mut entities_stopped = false;
    rayon::scope(|scope| {
        scope.spawn(|_| {
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
                        || {
                            compile_block_overlay(
                                view,
                                custom_blocks,
                                hashed_block_ids,
                                BASE_MATERIAL_KEYS.get(),
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
            let block_icons = overlay
                .as_deref()
                .map_or_else(BlockIcons::default, |compiled| {
                    custom_block_icons(
                        &compiled.overlay,
                        custom_blocks,
                        hashed_block_ids,
                        &inputs.block_items,
                    )
                });
            icons = compile_part(
                &stack,
                cancelled,
                changes.icons || changes.blocks,
                Subscriber::Icons,
                previous.and_then(|old| old.item_icons.clone()),
                |view| compile_session_icons(view, icon_keys, block_icons),
            );
        });
        scope.spawn(|_| {
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
            entities = compile_part(
                &stack,
                cancelled,
                changes.entities,
                Subscriber::Entities,
                previous.and_then(|old| old.entities.clone()),
                |view| {
                    let compiled = super::entity_pack::compile_session_entities(
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
            sounds = compile_part(
                &stack,
                cancelled,
                changes.sounds,
                Subscriber::Sounds,
                previous.and_then(|old| old.server_sounds.clone()),
                |view| crate::audio::ServerSoundPack::from_view(view).map(Arc::new),
            );
        });
    });
    fn record<T>(
        dependencies: &mut super::pack_reload_diff::Dependencies,
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
    super::item_diagnostics::session_icons(icon_keys.len(), item_icons.as_deref());
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
        extension_marker,
        property_defaults,
        prepared_actor_artwork: previous.and_then(|old| old.prepared_actor_artwork.clone()),
        inputs,
        server_lang,
        item_icons,
        item_components: None,
        glyph_sheets,
        entities,
        entity_artwork,
        server_ui,
        server_sounds,
        aim_assist_textures,
        admission: PackAdmission::Validated(stack),
        block_overlay,
        dependencies,
    })
}

/// Server-pack archives and compiled join results persist here; unset, nothing is written.
static COMPILE_CACHE: std::sync::OnceLock<client_session::compile_cache::CompileCache> =
    std::sync::OnceLock::new();

/// Bounds the persisted results; Zeqa's compiled entity pack is about 150 MB.
const COMPILE_CACHE_BYTES: u64 = 1024 * 1024 * 1024;

pub(crate) fn set_compile_cache_dir(dir: std::path::PathBuf) {
    let _ = COMPILE_CACHE.set(client_session::compile_cache::CompileCache::new(
        dir,
        COMPILE_CACHE_BYTES,
    ));
}

pub(super) fn compile_cache() -> Option<&'static client_session::compile_cache::CompileCache> {
    COMPILE_CACHE.get()
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

/// Texture keys of the vanilla carrier's materials, set once at startup when the sidecar loads.
static BASE_MATERIAL_KEYS: std::sync::OnceLock<assets::MaterialKeys> = std::sync::OnceLock::new();

pub(crate) fn set_base_material_keys(keys: assets::MaterialKeys) {
    let _ = BASE_MATERIAL_KEYS.set(keys);
}

use client_session::pack_language::merged_server_lang;
pub(crate) use client_session::pack_language::{active_language_code, set_active_language};

pub(super) use client_session::{StackFingerprint, stack_fingerprint};

struct CachedOverlay {
    dependencies: Option<std::collections::BTreeSet<resource_pack::PackDependency>>,
    stack: StackFingerprint,
    hashed: bool,
    blocks: protocol::CustomBlocks,
    overlay: Option<Arc<CompiledBlockOverlay>>,
}

/// The previous session's compiled overlay, reused when the same pack stack and
/// block definitions rejoin.
static OVERLAY_CACHE: std::sync::Mutex<Option<CachedOverlay>> = std::sync::Mutex::new(None);

/// Reuses compiled blocks with the fingerprint already computed for this admission.
fn cached_block_overlay(
    fingerprint: &StackFingerprint,
    view: &LayeredPackView,
    blocks: &protocol::CustomBlocks,
    hashed: bool,
    compile: impl FnOnce() -> Option<Arc<CompiledBlockOverlay>>,
) -> Option<Arc<CompiledBlockOverlay>> {
    let mut cache = OVERLAY_CACHE
        .lock()
        .unwrap_or_else(|poison| poison.into_inner());
    if let Some(cached) = cache.as_ref()
        && cached.hashed == hashed
        && cached.stack == *fingerprint
        && cached.blocks == *blocks
        && (view.dependencies().is_none() || cached.dependencies.is_some())
    {
        if let (Some(dependencies), Some(inputs)) = (view.dependencies(), &cached.dependencies) {
            dependencies.extend(inputs.clone());
        }
        return cached.overlay.clone();
    }
    let overlay = compile();
    *cache = Some(CachedOverlay {
        dependencies: view
            .dependencies()
            .map(|dependencies| dependencies.snapshot()),
        stack: fingerprint.clone(),
        hashed,
        blocks: blocks.clone(),
        overlay: overlay.clone(),
    });
    overlay
}

/// Returns the carrier extended with this session's custom block visuals, or the
/// carrier itself when there is nothing to apply or the overlay does not fit.
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
    if compiled.overlay.visuals.len() != ids.len() {
        bevy::log::warn!("server block visuals do not match the custom block ids");
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
    textures: &mut render::ChunkTextureAssets,
    assets: &Arc<assets::RuntimeAssets>,
) {
    static REVISION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    if !Arc::ptr_eq(textures.assets(), assets) {
        let revision = REVISION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        *textures = render::ChunkTextureAssets::with_revision(Arc::clone(assets), revision);
    }
}

pub(crate) use client_session::pack_textures::set_base_terrain_catalog;
pub(super) use client_session::pack_textures::{
    DecodedTexture, MAX_CATALOG_ENTRIES, base_terrain_catalog, decode_pack_texture,
    parse_pack_json, texture_key_paths,
};

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

pub(crate) use client_session::{BootstrapGenerationDisposition, classify_bootstrap_generation};

/// Bevy adapter for the generation-bound admitted server stack.
#[derive(Debug, Default, Resource, bevy::prelude::Deref, bevy::prelude::DerefMut)]
pub(crate) struct ResourcePackAdmissionState(client_session::ResourcePackAdmissionState);

#[cfg(test)]
mod tests;

#[cfg(test)]
mod fingerprint_bench;
