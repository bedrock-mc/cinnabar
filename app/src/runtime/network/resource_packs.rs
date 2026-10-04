use std::sync::Arc;

use bevy::prelude::Resource;
use resource_pack::{LayeredPackView, PackAdmission};

use super::{
    block_overlay::{CompiledBlockOverlay, compile_block_overlay},
    glyph_sheets::compile_session_glyphs,
    item_icons::{BlockIcons, compile_session_icons, custom_block_icons},
};
use client_ui::ui_runtime::presentation::{ServerUiPack, SessionGlyphSheets, SessionIcons};

mod ui;
use ui::collect_server_ui;

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
        }
    }
}

impl PackApplication {
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

/// Compiles presentation from already validated session-owned pack inputs.
pub(super) fn prepare_session_presentation(
    preparation: &client_session::PackPreparation,
    game_data: &protocol::GameData,
) -> Result<PackApplication, client_session::RequiredPackRejected> {
    preparation.prepare_application(
        |preparation| match &preparation.admission {
            PackAdmission::None => PackApplication {
                inputs: Arc::clone(&preparation.inputs),
                ..Default::default()
            },
            PackAdmission::Validated(stack) => {
                prepare_validated_application(Arc::clone(stack), Arc::clone(&preparation.inputs))
            }
        },
        |packs| {
            packs.item_components =
                client_ui::ui_runtime::item_facts::SessionItemComponents::from_game_data(game_data);
        },
    )
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
pub(super) fn prepare_validated_application(
    stack: Arc<resource_pack::ValidatedPackStack>,
    inputs: Arc<super::pack_reload::PackInputs>,
) -> PackApplication {
    prepare_changed_application(stack, inputs, None)
}

/// Reuses each compiled subscriber whose contributing files have not changed.
pub(super) fn prepare_changed_application(
    stack: Arc<resource_pack::ValidatedPackStack>,
    inputs: Arc<super::pack_reload::PackInputs>,
    previous: Option<&PackApplication>,
) -> PackApplication {
    let changes = super::pack_reload_diff::Changes::between(&stack, previous);
    use super::pack_reload_diff::{Subscriber, compile};
    let mut dependencies = previous
        .map(|old| old.dependencies.clone())
        .unwrap_or_default();
    let custom_blocks = &inputs.blocks;
    let icon_keys = &inputs.icons;
    let block_items = &inputs.block_items;
    let hashed_block_ids = inputs.hashed;
    for rejection in stack.rejections() {
        bevy::log::warn!(
            stack_index = rejection.stack_index,
            reason = %rejection.reason,
            "server resource pack dropped"
        );
    }
    let view = LayeredPackView::new(Arc::clone(&stack));
    let fingerprint = stack_fingerprint(&stack);
    let block_overlay = if !changes.blocks {
        previous.and_then(|old| old.block_overlay.clone())
    } else {
        compile(Subscriber::Blocks, &stack, &mut dependencies, |view| {
            cached_block_overlay(&fingerprint, view, custom_blocks, hashed_block_ids, || {
                compile_block_overlay(
                    view,
                    custom_blocks,
                    hashed_block_ids,
                    BASE_MATERIAL_KEYS.get(),
                )
                .map(Arc::new)
            })
        })
    };
    if let Some(compiled) = &block_overlay
        && compiled.gaps != Default::default()
    {
        bevy::log::warn!(gaps = ?compiled.gaps, "server block visuals are incomplete");
    }
    let block_icons = block_overlay
        .as_deref()
        .map_or_else(BlockIcons::default, |compiled| {
            custom_block_icons(
                &compiled.overlay,
                custom_blocks,
                hashed_block_ids,
                block_items,
            )
        });
    let item_icons = if changes.icons || changes.blocks {
        compile(Subscriber::Icons, &stack, &mut dependencies, |view| {
            compile_session_icons(view, icon_keys, block_icons)
        })
    } else {
        previous.and_then(|old| old.item_icons.clone())
    };
    super::item_diagnostics::session_icons(icon_keys.len(), item_icons.as_deref());
    PackApplication {
        inputs,
        server_lang: if changes.language {
            compile(
                Subscriber::Language,
                &stack,
                &mut dependencies,
                merged_server_lang,
            )
        } else {
            previous.and_then(|old| old.server_lang.clone())
        },
        extension_marker: view
            .read_capped(
                server_experience::policy::MARKER_PATH,
                server_experience::policy::MAX_MARKER_BYTES as u64,
            )
            .map(Arc::from),
        item_icons,
        item_components: None,
        glyph_sheets: if changes.glyphs {
            compile(
                Subscriber::Glyphs,
                &stack,
                &mut dependencies,
                compile_session_glyphs,
            )
        } else {
            previous.and_then(|old| old.glyph_sheets.clone())
        },
        entities: if changes.entities {
            compile(Subscriber::Entities, &stack, &mut dependencies, |view| {
                super::entity_pack::compile_session_entities(&fingerprint, view)
            })
        } else {
            previous.and_then(|old| old.entities.clone())
        },
        entity_artwork: if changes.entities {
            {
                let artwork_view = LayeredPackView::tracked(stack.clone());
                let artwork = super::entity_texture_reload::prepare(&artwork_view);
                dependencies
                    .entry(Subscriber::Entities)
                    .or_default()
                    .extend(
                        artwork_view
                            .dependencies()
                            .expect("tracked view")
                            .snapshot(),
                    );
                artwork
            }
        } else {
            previous.and_then(|old| old.entity_artwork.clone())
        },
        prepared_actor_artwork: previous.and_then(|old| old.prepared_actor_artwork.clone()),
        property_defaults: super::entity_pack::pack_property_defaults(&view),
        server_ui: if changes.ui {
            compile(Subscriber::Ui, &stack, &mut dependencies, collect_server_ui)
        } else {
            previous.and_then(|old| old.server_ui.clone())
        },
        server_sounds: if changes.sounds {
            compile(Subscriber::Sounds, &stack, &mut dependencies, |view| {
                crate::audio::ServerSoundPack::from_view(view).map(Arc::new)
            })
        } else {
            previous.and_then(|old| old.server_sounds.clone())
        },
        admission: PackAdmission::Validated(stack),
        block_overlay,
        dependencies,
    }
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
    compiled: Option<&CompiledBlockOverlay>,
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
    match base.with_block_overlay(ids.start, &compiled.overlay) {
        Ok(assets) => Arc::new(assets),
        Err(error) => {
            bevy::log::warn!(%error, "server block visuals were not applied");
            Arc::clone(base)
        }
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
