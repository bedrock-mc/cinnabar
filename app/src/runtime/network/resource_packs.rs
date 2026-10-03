use std::{collections::HashMap, io::Cursor, sync::Arc};

use bevy::prelude::Resource;
use image::{ImageFormat, ImageReader, Limits};
use resource_pack::{LayeredPackView, PackAdmission, normalize_jsonc};
use serde_json::Value;

use super::{
    block_overlay::{CompiledBlockOverlay, compile_block_overlay},
    glyph_sheets::compile_session_glyphs,
    item_icons::{BlockIcons, compile_session_icons, custom_block_icons, custom_block_items},
};
use crate::ui_runtime::presentation::{ServerUiPack, SessionGlyphSheets, SessionIcons};

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
    pub(crate) item_components: Option<Arc<crate::ui_runtime::item_facts::SessionItemComponents>>,
    pub(crate) glyph_sheets: Option<Arc<SessionGlyphSheets>>,
    pub(crate) entities: Option<Arc<super::entity_pack::SessionEntityPack>>,
    pub(super) entity_artwork: Option<Arc<render::ActorArtworkPages>>,
    pub(crate) prepared_actor_artwork:
        Option<Arc<super::prepared_actor_artwork::PreparedActorArtwork>>,
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
            Arc::new(super::prepared_actor_artwork::PreparedActorArtwork::new(
                base, pack,
            ))
        });
    }
}

/// A server-required pack the client could not apply; vanilla refuses such a join.
#[derive(Debug, thiserror::Error)]
#[error("required resource pack could not be applied ({rejected} of the stack rejected)")]
pub(super) struct RequiredPackRejected {
    rejected: usize,
}

/// Reads StartGame's custom blocks and item icon keys, then applies the stack.
pub(super) fn prepare_session_packs(
    handoff: protocol::ResourcePackHandoff,
    game_data: &protocol::GameData,
) -> Result<(protocol::CustomBlocks, PackApplication), RequiredPackRejected> {
    let custom_blocks = protocol::CustomBlocks::from_game_data(game_data);
    let icon_keys = protocol::item_icon_keys(game_data);
    let block_items = custom_block_items(game_data, &custom_blocks);
    let hashed = game_data.start_game.block_network_ids_are_hashes;
    bevy::log::info!(
        block_network_ids_are_hashes = hashed,
        block_property_count = game_data.start_game.block_properties.len(),
        custom_block_count = custom_blocks.blocks.len(),
        custom_state_count = custom_blocks.total_states(),
        skipped_block_definitions = custom_blocks.skipped,
        "START_GAME_BLOCK_IDS"
    );
    let required = handoff.required();
    let mut packs =
        prepare_pack_application(handoff, &custom_blocks, &icon_keys, &block_items, hashed);
    required_packs_applied(required, &packs.admission)?;
    packs.item_components =
        crate::ui_runtime::item_facts::SessionItemComponents::from_game_data(game_data);
    Ok((custom_blocks, packs))
}

/// Optional packs that fail validation are dropped; a required one ends the join.
fn required_packs_applied(
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

/// `block_items` pairs each custom block item with the block it draws as.
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

/// Bound on the pack UI json handed to the form engine.
const MAX_SERVER_UI_BYTES: usize = 16 * 1024 * 1024;

/// Each pack's `ui/**/*.json` (lowest precedence first), with the stack its
/// textures read from; `None` when no pack carries ui or textures.
fn collect_server_ui(view: &LayeredPackView) -> Option<Arc<ServerUiPack>> {
    let mut total = 0usize;
    let mut pack = ServerUiPack::default();
    for layer in view.layers() {
        let mut files = Vec::new();
        for path in layer
            .files_under("ui/")
            .iter()
            .filter(|path| path.ends_with(".json"))
        {
            let Some(bytes) = layer.read_file(path).ok().flatten() else {
                continue;
            };
            total = total.saturating_add(bytes.len());
            if total > MAX_SERVER_UI_BYTES {
                break;
            }
            files.push(((*path).to_owned(), bytes.into_vec()));
        }
        pack.ui_layers.push(files);
    }
    // Textures are read on first draw, after this compile, so all of their bytes are inputs.
    view.track_contents("textures/");
    // A pack that only restyles textures still overrides vanilla UI art.
    if pack.is_empty() && view.list("textures/").is_empty() {
        return None;
    }
    pack.view = Some(view.clone());
    Some(Arc::new(pack))
}

pub(super) fn install_server_ui(
    runtime: &mut crate::ui_runtime::UiRuntime,
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

/// The selected UI language; unset means the base English table only.
static ACTIVE_LANG_PATH: std::sync::RwLock<Option<String>> = std::sync::RwLock::new(None);

pub(crate) fn set_active_language(code: &str) {
    *ACTIVE_LANG_PATH
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) =
        (code != "en_US").then(|| format!("texts/{code}.lang"));
}

/// Returns the same locale used by the language overlay and its font resources.
pub(crate) fn active_language_code() -> String {
    ACTIVE_LANG_PATH
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_deref()
        .and_then(|path| path.strip_prefix("texts/")?.strip_suffix(".lang"))
        .unwrap_or("en_US")
        .to_owned()
}

pub(super) type StackFingerprint = Vec<(String, String, String, [u8; 32])>;

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

/// Identity of each pack as (uuid, version, subpack, content hash), in stack order.
pub(super) fn stack_fingerprint(stack: &resource_pack::ValidatedPackStack) -> StackFingerprint {
    use sha2::{Digest, Sha256};
    stack
        .packs()
        .iter()
        .map(|pack| {
            (
                pack.pack_id().to_string(),
                pack.version().to_owned(),
                pack.sub_pack_name().to_owned(),
                Sha256::digest(&*pack.archive_bytes()).into(),
            )
        })
        .collect()
}

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

const MAX_TEXTURE_SOURCE_BYTES: usize =
    crate::ui_runtime::presentation::MAX_PACK_TEXTURE_BYTES as usize;
const MAX_TEXTURE_SIDE: u32 = 1024;
const MAX_DECODE_ALLOC: u64 = 16 * 1024 * 1024;
pub(super) const MAX_CATALOG_ENTRIES: usize = 16_384;

/// Straight-alpha RGBA8 pixels decoded from a pack image.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct DecodedTexture {
    pub(super) width: u32,
    pub(super) height: u32,
    pub(super) rgba8: Box<[u8]>,
}

/// Maps each `texture_data` key of a texture catalog (terrain or item) to its
/// image path; a higher pack replaces a key.
pub(super) fn texture_key_paths(view: &LayeredPackView, catalog: &str) -> HashMap<String, String> {
    let mut paths = HashMap::new();
    for layer in view.read_layers(catalog) {
        merge_texture_catalog(&mut paths, &layer);
    }
    paths
}

static BASE_TERRAIN_CATALOG: std::sync::OnceLock<HashMap<String, String>> =
    std::sync::OnceLock::new();

/// Supplies the base texture aliases so a pack can replace rasters without repeating the catalog.
pub(crate) fn set_base_terrain_catalog<'a>(aliases: impl IntoIterator<Item = (&'a str, &'a str)>) {
    let paths = aliases
        .into_iter()
        .map(|(key, path)| (key.to_owned(), path.to_owned()))
        .collect();
    let _ = BASE_TERRAIN_CATALOG.set(paths);
}

/// Immutable aliases from the world carrier's sidecar, below all optional catalog layers.
pub(super) fn base_terrain_catalog() -> HashMap<String, String> {
    BASE_TERRAIN_CATALOG.get().cloned().unwrap_or_default()
}

/// Reads valid entries independently, preserving lower aliases for malformed entries.
fn merge_texture_catalog(paths: &mut HashMap<String, String>, bytes: &[u8]) {
    let Some(Value::Object(data)) =
        parse_pack_json(bytes).map(|mut root| root["texture_data"].take())
    else {
        return;
    };
    for (key, entry) in data {
        if paths.len() >= MAX_CATALOG_ENTRIES && !paths.contains_key(&key) {
            break;
        }
        if let Some(path) = first_texture_path(&entry["textures"]) {
            paths.insert(key, path);
        }
    }
}

const IMAGE_EXTENSIONS: [(&str, ImageFormat); 4] = [
    ("png", ImageFormat::Png),
    ("tga", ImageFormat::Tga),
    ("jpg", ImageFormat::Jpeg),
    ("jpeg", ImageFormat::Jpeg),
];
const MAX_TEXTURE_SET_BYTES: u64 = 64 * 1024;

/// Decodes the winning image at `path`, trying `.png`, `.tga`, and `.jpg` as
/// vanilla does, then the color layer of a `.texture_set.json`.
pub(super) fn decode_pack_texture(view: &LayeredPackView, path: &str) -> Option<DecodedTexture> {
    decode_image_file(view, path).or_else(|| decode_texture_set(view, path))
}

fn decode_image_file(view: &LayeredPackView, path: &str) -> Option<DecodedTexture> {
    // Vanilla's loader also tries the literal path, so a pack path that already
    // names its image (`textures/items/gem.png`) resolves.
    let named = path.rsplit_once('.').and_then(|(_, extension)| {
        IMAGE_EXTENSIONS
            .into_iter()
            .find(|(known, _)| extension.eq_ignore_ascii_case(known))
    });
    if let Some((_, format)) = named
        && let Some(texture) = view
            .read_capped(path, MAX_TEXTURE_SOURCE_BYTES as u64)
            .and_then(|bytes| decode_image(&bytes, format))
    {
        return Some(texture);
    }
    IMAGE_EXTENSIONS
        .into_iter()
        .find_map(|(extension, format)| {
            let bytes = view.read_capped(
                &format!("{path}.{extension}"),
                MAX_TEXTURE_SOURCE_BYTES as u64,
            )?;
            decode_image(&bytes, format)
        })
}

/// A texture set's `color` is a sibling image name or a solid `[r, g, b(, a)]`.
fn decode_texture_set(view: &LayeredPackView, path: &str) -> Option<DecodedTexture> {
    let bytes = view.read_capped(&format!("{path}.texture_set.json"), MAX_TEXTURE_SET_BYTES)?;
    let root = parse_pack_json(&bytes)?;
    match root.get("minecraft:texture_set")?.get("color")? {
        Value::String(name) => {
            let name = name.trim().trim_start_matches("./");
            let sibling = path
                .rsplit_once('/')
                .map_or_else(|| name.to_owned(), |(dir, _)| format!("{dir}/{name}"));
            [sibling, name.to_owned()]
                .into_iter()
                .filter(|target| target != path)
                .find_map(|target| decode_image_file(view, &target))
        }
        Value::Array(channels) if matches!(channels.len(), 3 | 4) => {
            let mut pixel = [0, 0, 0, 255];
            for (slot, channel) in pixel.iter_mut().zip(channels) {
                *slot = channel.as_f64()?.round().clamp(0.0, 255.0) as u8;
            }
            Some(DecodedTexture {
                width: 1,
                height: 1,
                rgba8: pixel.into(),
            })
        }
        _ => None,
    }
}

pub(super) fn parse_pack_json(bytes: &[u8]) -> Option<Value> {
    serde_json::from_slice(&normalize_jsonc(bytes)?).ok()
}

/// A texture entry is a path, an object with `path`, or a variation list whose
/// first element is used.
fn first_texture_path(value: &Value) -> Option<String> {
    let path = match value {
        Value::String(path) => path.as_str(),
        Value::Object(entry) => entry.get("path")?.as_str()?,
        Value::Array(entries) => return first_texture_path(entries.first()?),
        _ => return None,
    };
    let path = path.trim().trim_start_matches("./");
    (!path.is_empty()).then(|| path.to_owned())
}

fn decode_image(bytes: &[u8], format: ImageFormat) -> Option<DecodedTexture> {
    if bytes.is_empty() || bytes.len() > MAX_TEXTURE_SOURCE_BYTES {
        return None;
    }
    let (width, height) = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .ok()?;
    if width == 0 || height == 0 || width > MAX_TEXTURE_SIDE || height > MAX_TEXTURE_SIDE {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_TEXTURE_SIDE);
    limits.max_image_height = Some(MAX_TEXTURE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let rgba8 = reader
        .decode()
        .ok()?
        .into_rgba8()
        .into_raw()
        .into_boxed_slice();
    Some(DecodedTexture {
        width,
        height,
        rgba8,
    })
}

const SERVER_LANG_PATH: &str = "texts/en_US.lang";

/// Merges every pack's language file so a higher-precedence pack overrides a
/// key and keys it does not define still come from lower packs; the UI
/// language's files override en_US. Lowest layers are dropped first if the
/// merged text would exceed the overlay input bound.
fn merged_server_lang(view: &LayeredPackView) -> Option<Arc<assets::ServerLangOverlay>> {
    let mut kept = Vec::new();
    let mut total = 0usize;
    let mut layers = view.read_layers(SERVER_LANG_PATH);
    let active = ACTIVE_LANG_PATH
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(active) = active.as_deref() {
        layers.extend(view.read_layers(active));
    }
    for layer in layers.into_iter().rev() {
        let text = layer
            .strip_prefix(b"\xef\xbb\xbf")
            .unwrap_or(&layer)
            .to_vec();
        let Some(next) = total.checked_add(text.len() + 1) else {
            break;
        };
        if next > assets::MAX_SERVER_LANG_INPUT_BYTES {
            break;
        }
        total = next;
        kept.push(text);
    }
    if kept.is_empty() {
        return None;
    }
    // The overlay keeps the last definition of a key, so write lowest first.
    let mut merged = Vec::with_capacity(total);
    for text in kept.iter().rev() {
        merged.extend_from_slice(text);
        merged.push(b'\n');
    }
    assets::ServerLangOverlay::read(merged.len(), |output| {
        output.copy_from_slice(&merged);
        true
    })
}

pub(super) fn install_session_icons(
    runtime: &mut crate::ui_runtime::UiRuntime,
    generation: u64,
    icons: Option<Arc<SessionIcons>>,
    items: Option<Arc<crate::ui_runtime::item_facts::SessionItemComponents>>,
    setup_succeeded: bool,
) {
    if runtime.session_id() == generation {
        runtime.set_session_icons(icons.filter(|_| setup_succeeded));
        runtime.set_session_items(items.filter(|_| setup_succeeded));
    }
}

pub(super) fn install_session_glyphs(
    runtime: &mut crate::ui_runtime::UiRuntime,
    generation: u64,
    glyphs: Option<Arc<SessionGlyphSheets>>,
    setup_succeeded: bool,
) {
    if runtime.session_id() == generation {
        runtime.set_session_glyphs(glyphs.filter(|_| setup_succeeded));
    }
}

pub(super) fn install_server_language(
    runtime: &mut crate::ui_runtime::UiRuntime,
    generation: u64,
    overlay: Option<std::sync::Arc<assets::ServerLangOverlay>>,
    setup_succeeded: bool,
) {
    if runtime.session_id() == generation {
        runtime.set_server_lang(if setup_succeeded { overlay } else { None });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BootstrapGenerationDisposition {
    Expected,
    Stale,
    Unexpected,
}

pub(crate) const fn classify_bootstrap_generation(
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
#[derive(Debug, Resource)]
pub(crate) struct ResourcePackAdmissionState {
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
    pub(crate) fn begin_generation(&mut self, generation: u64) -> bool {
        if generation <= self.generation {
            return false;
        }
        self.generation = generation;
        self.admission = PackAdmission::None;
        true
    }

    /// Publishes admission only for the pending/current or a newer generation.
    pub(crate) fn replace_for_generation(
        &mut self,
        generation: u64,
        admission: PackAdmission,
    ) -> bool {
        if generation < self.generation {
            return false;
        }
        self.generation = generation;
        self.admission = admission;
        true
    }

    /// Releases admission when the current network session terminates.
    pub(crate) fn clear_current(&mut self) {
        self.admission = PackAdmission::None;
    }

    #[cfg(test)]
    pub(crate) const fn generation(&self) -> u64 {
        self.generation
    }

    #[cfg(test)]
    pub(crate) const fn admission(&self) -> &PackAdmission {
        &self.admission
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod fingerprint_bench;
