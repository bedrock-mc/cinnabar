//! Installed OreUI artwork is read once at runtime; no vanilla images are shipped.

mod catalog;
pub(crate) mod dimensions;
mod packing;

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::{Cursor, Read},
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use image::{AnimationDecoder, ImageDecoder, ImageReader, Limits};
use serde::Deserialize;

/// Side of the packed OreUI page.
pub const OREUI_PAGE_SIDE: u32 = 3072;
const MAX_ATLAS_JSON_BYTES: u64 = 1024 * 1024;
const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_IMAGE_SIDE: u32 = render_model::MAX_UI_TEXTURE_SIDE;
const MAX_ANIMATION_FRAMES: usize = 256;
const MAX_ANIMATION_PIXELS_BYTES: usize = 64 * 1024 * 1024;
const MAX_BUNDLE_FILES: usize = 4096;
/// Create-world artwork resolves from the installed native bundle.
pub(crate) const WORLD_PREVIEW: &str = "assets/world-preview-default-d0210bba13d939ca9e72.jpg";
pub(crate) const WORLD_CATEGORY_ICONS: [&str; 7] = [
    "assets/general-icon-8ce31666e00cf3491940.png",
    "assets/advanced-icon-1b6c5b572a777772a78e.png",
    "assets/multiplayer-icon-d7d04b2a5d3ae87f5ed5.png",
    "assets/cheats-icon-33f132756175c43752b8.png",
    "assets/resource-packs-icon-e08f710fbdadb2f3a5a5.png",
    "assets/behaviour-packs-icon-2f4af74c62b42f1fb412.png",
    "assets/experimental-features-icon-dca93f83a503a8e269eb.png",
];
pub(crate) const HARDCORE_ICON: &str = "assets/hardcore-heart-engraved-75556ce94d9bfdecca12.png";
/// The pixelated loading animation selected by OreUI `ep`.
pub(crate) const LOADING_ANIMATION: &str = "assets/animation-074ed0ba8c16bb30e36c.gif";
pub(crate) const SWITCH_ON_IMAGE: &str = "assets/onImage-b40d0be137ba09eb7464.png";
pub(crate) const SWITCH_OFF_IMAGE: &str = "assets/offImage-cc9095b148d166ec7212.png";
pub(crate) const CHEVRON_LEFT_IMAGE: &str =
    "assets/chevron-left@0.5x.icon-bd82feed3671c96ec201568975cb9b29.png";
pub(crate) const CHEVRON_UP_IMAGE: &str =
    "assets/chevron-up@0.5x.icon-bb53390a6964767e019a190f32bf1661.png";
pub(crate) const CHEVRON_DOWN_IMAGE: &str =
    "assets/chevron-down@0.5x.icon-0034ec5c85502861a7fba88e1f17681e.png";
pub(crate) const BASE_PACK_IMAGE: &str = "assets/minecraft-texture-pack-4c96be5bfdd5a55edf09.png";
pub(crate) const MISSING_PACK_IMAGE: &str = "assets/missing-pack-icon-010c87c773e1a21c8ac7.png";
pub(crate) const OVERWORLD_BLOCK_IMAGE: &str = "assets/grass_block-fbea7d7f754c51b4ea00.png";
pub(crate) const SETTINGS_ICON_HIGHLIGHT_IMAGE: &str =
    "assets/icon-highlight-spritesheet-87ec62988bf89f63558d.png";
/// Servers status artwork: low, medium, high and pending ping.
pub(crate) const SERVER_PING_IMAGES: [&str; 4] = [
    "assets/pingGreen-7f77ca04def817211b2a.png",
    "assets/pingYellow-62e787f8117760a86807.png",
    "assets/pingRed-9496c1bbb587d63e1dbe.png",
    "assets/pingAnimation-4d7a029e5f864843c958.png",
];
pub(crate) const SERVER_PLAYERS_IMAGE: &str = "assets/player-online-icon-b63b81863545d7c6a444.png";
pub(crate) const SERVER_ADD_IMAGE: &str =
    "assets/plus@1x.icon-bb0896fb794b5ac908344f0de1cc54f0.png";
/// Play-tab artwork in Worlds, Realms, Servers order.
pub(crate) const PLAY_TAB_ICONS: [&str; 3] = [
    "assets/UI_Menu_WorldsTab-dfd408c83c4cf07814b8.png",
    "assets/UI_Menu_RealmsTab-c7419af9abd527149fcc.png",
    "assets/UI_Menu_ServerTab-e7c3c035b7d6ba5a414b.png",
];
/// Standalone category images from the installed OreUI bundle, in sidebar order.
pub const INBOX_ICONS: [&str; 5] = [
    "assets/News-f81489154ff3c38f5b5f.png",
    "assets/Realms-c7419af9abd527149fcc.png",
    "assets/Invites-6a62211ac071c98b2994.png",
    "assets/MarketplacePass-8eb08ee1dd714dd15307.png",
    "assets/Feedback-ecfce4d670046c25d3df.png",
];

/// Empty-message illustrations, in the same order as the category icons.
pub(crate) const INBOX_EMPTY_IMAGES: [&str; 5] = [
    "assets/Inbox_NoNews-c959e7726f6bab89cf50.png",
    "assets/Inbox_NoRealmsNews-714f9275b786d6ffa804.png",
    "assets/Inbox_NoInvites-d82f3107d60a8e227c18.png",
    "assets/Inbox_NoMarketplacePass-a2fc4019bcd55acf2183.png",
    "assets/Inbox_NoFeedback-85b4273f92f8e5f1b44a.png",
];

/// Settings category art, read only from the optional installed bundle.
pub(crate) const SETTINGS_ICONS: [&str; 14] = [
    "assets/accessibility-41a033eee6f8f8726f5e.png",
    "assets/keyboard-mouse-58d767999df54b704830.png",
    "assets/controls-5b4a0c8bc7ac0539b349.png",
    "assets/touch-e5c085e0d79f26deb0c9.png",
    "assets/party-cc4b74f40b6ecac45aaf.png",
    "assets/work-bench-e18a3804944464e2d854.png",
    "assets/painting-c9fe53a4df86136c1e14.png",
    "assets/sound-block-0abbefc33b0871e358ac.png",
    "assets/account-46c198f87391d9c79cf7.png",
    "assets/subscriptions-ad5676c80eb176fff46a.png",
    "assets/chest-67b965a3181be3553287.png",
    "assets/storage-101411b344fe42108e57.png",
    "assets/language-47127a2e274f540f09de.png",
    "assets/command-block-0c312a997f65ebda04a3.png",
];

/// Standalone Profile assets named by the version-matched OreUI components.
pub(crate) const PROFILE_STAT_ICONS: [&str; 4] = [
    "assets/IconClockGrey-ea5642bd84ad58714dd4.png",
    "assets/IconPickaxeGrey-cb118f7d544ff4fce2e2.png",
    "assets/IconSwordGrey-9087f66e9056b3b959aa.png",
    "assets/IconBootsGrey-142a375cbe9eecc3879d.png",
];
/// The deterministic fallback banner choices used by OreUI `mZ`.
pub(crate) const PROFILE_BANNERS: [&str; 8] = [
    "assets/screenshot_1-48404d5a8f0097356091.jpg",
    "assets/screenshot_2-7c721593cd419875cf29.jpg",
    "assets/screenshot_3-9dd41022b2ee8d8c2c08.jpg",
    "assets/screenshot_4-72ff980d133dc323944c.jpg",
    "assets/screenshot_5-5636d6b539bf7f23759d.jpg",
    "assets/screenshot_6-380c0205af54e86438ac.jpg",
    "assets/screenshot_7-6cfec1e1e1009ea73447.jpg",
    "assets/screenshot_8-b5f240429090c0ddd613.jpg",
];
/// Profile error art, loaded only from an install at runtime.
pub(crate) const PROFILE_ERRORS: [&str; 3] = [
    "assets/nothing_to_see-1107fc6902173eb98f28.png",
    "assets/generic_error-aa90619c4c1746eb7ac0.png",
    "assets/connection_error-01bc20883b3a2f7e5fb3.png",
];
/// The Overview row art selected by OreUI `g2`.
pub(crate) const PROFILE_SUMMARY_ICONS: [&str; 4] = [
    "assets/friends-9e435522a799e248f3c5.png",
    "assets/followers-bec3d954895866890570.png",
    "assets/gallery-10d21b3ca655e32b1ac8.png",
    "assets/achievements-a42ab3d4b8e49c24b217.png",
];
/// Gamerscore art shared by Overview and achievement cards.
pub(crate) const PROFILE_GAMERSCORE: &str = "assets/gamerscore_icon-53b10130cb6f6c0271cb.png";
/// Native action icons in ArrowBack, Cross, Search, Player, Pencil, Check and Filter order.
pub(crate) const ACTION_ICONS: [&str; 7] = [
    "assets/arrow-left@0.5x.icon-32012c9c1d3e6debaaf8ce01da9815de.png",
    "assets/cross@0.5x.icon-a30f9556f5895c0d6996af977c6fbb91.png",
    "assets/search@0.5x.icon-57e5a707535fd5959a31271ce38e0b7f.png",
    "assets/player@0.5x.icon-5f2efe885c1189f09a5388b6e6b07c9f.png",
    "assets/edit@0.5x.icon-a786502003e9894de25c9a2b274fbcbb.png",
    "assets/checkmark@0.5x.icon-3a1f3dd3866716c7bc33ad20204c68ca.png",
    "assets/filter@0.5x.icon-4d527e67d899c241675338c67277d157.png",
];
/// Disabled elevated button face.
pub(crate) const BUTTON_DISABLED_IMAGE: &str =
    "assets/pressable_elevated_disabled-d849d4185836b2229b5a.png";
/// Elevated button artwork in default, hovered, focused and pressed order.
pub(crate) const BUTTON_PRIMARY_IMAGES: [&str; 4] = [
    "assets/pressable_elevated_primary_default-7a6e2edf98a626b182c2.png",
    "assets/pressable_elevated_primary_hovered-bcb21ab092446cbcecb6.png",
    "assets/pressable_elevated_primary_focused-cc7df0bf833f92bb08dd.png",
    "assets/pressable_primary_pressed-13b0f01c6c00e07322ca.png",
];
/// Elevated button artwork in default, hovered, focused and pressed order.
pub(crate) const BUTTON_SECONDARY_IMAGES: [&str; 4] = [
    "assets/pressable_elevated_secondary_default-0aacffbfa726a8184fc5.png",
    "assets/pressable_elevated_secondary_hovered-758970b376da9eb299da.png",
    "assets/pressable_elevated_secondary_focused-05d59cd4654230c9b01d.png",
    "assets/pressable_secondary_pressed-87bba1faba891ebfa7fc.png",
];
/// Elevated button artwork in default, hovered, focused and pressed order.
pub(crate) const BUTTON_NEUTRAL_IMAGES: [&str; 4] = [
    "assets/pressable_elevated_neutral_default-48cdf8535bfd48b47c2a.png",
    "assets/pressable_elevated_neutral_hovered-dcbd873ad9e83d24dc7a.png",
    "assets/pressable_elevated_neutral_focused-b28fc482cadde858740c.png",
    "assets/pressable_neutral_pressed-aad8b64fc0f155676218.png",
];
/// Elevated button artwork in default, hovered, focused and pressed order.
pub(crate) const BUTTON_DESTRUCTIVE_IMAGES: [&str; 4] = [
    "assets/pressable_elevated_destructive_default-a5fb3f173720065fcc13.png",
    "assets/pressable_elevated_destructive_hovered-63736c555d7fc6479755.png",
    "assets/pressable_elevated_destructive_focused-cfa496a1b2e3a06a9db4.png",
    "assets/pressable_destructive_pressed-d5d4943d1b774ef42f85.png",
];
/// Play tab faces in default, hovered, pressed, focused and pressed-focused order.
pub(crate) const PLAY_TAB_IMAGES: [&str; 5] = [
    "assets/tabBar_neutral_default-40e26ac9318c12909f42.png",
    "assets/tabBar_neutral_hovered-b73d7029874500714b0f.png",
    "assets/tabBar_neutral_pressed-9d14d8d1343a1fc3836d.png",
    "assets/tabBar_neutral_default_focused-812a9fcda5a4e49d93d5.png",
    "assets/tabBar_neutral_pressed_focused-c05012af3863c527b0a2.png",
];
/// Settings support links use the native external-link mask.
pub(crate) const EXTERNAL_LINK_ICON: &str =
    "assets/external-link@0.5x.icon-28016636e9d767b57dffe6e45fa749aa.png";
/// Settings binding reset mask.
pub(crate) const RESET_ICON: &str = "assets/reset@0.5x.icon-4fc2a8ff6f440b4d14293bb8e833a1bc.png";
/// Sleep animations use a hashed bundle name after this stable prefix.
pub(crate) const SLEEP_ANIMATION_PREFIX: &str = "assets/sleep_";

const REFERENCED_KEY_GROUPS: &[&[&str]] = &[
    &[
        WORLD_PREVIEW,
        HARDCORE_ICON,
        LOADING_ANIMATION,
        SWITCH_ON_IMAGE,
        SWITCH_OFF_IMAGE,
        CHEVRON_LEFT_IMAGE,
        CHEVRON_UP_IMAGE,
        CHEVRON_DOWN_IMAGE,
        BASE_PACK_IMAGE,
        MISSING_PACK_IMAGE,
        OVERWORLD_BLOCK_IMAGE,
        SETTINGS_ICON_HIGHLIGHT_IMAGE,
        SERVER_PLAYERS_IMAGE,
        SERVER_ADD_IMAGE,
        PROFILE_GAMERSCORE,
        BUTTON_DISABLED_IMAGE,
        EXTERNAL_LINK_ICON,
        RESET_ICON,
    ],
    &WORLD_CATEGORY_ICONS,
    &SERVER_PING_IMAGES,
    &PLAY_TAB_ICONS,
    &INBOX_ICONS,
    &INBOX_EMPTY_IMAGES,
    &SETTINGS_ICONS,
    &PROFILE_STAT_ICONS,
    &PROFILE_BANNERS,
    &PROFILE_ERRORS,
    &PROFILE_SUMMARY_ICONS,
    &ACTION_ICONS,
    &BUTTON_PRIMARY_IMAGES,
    &BUTTON_SECONDARY_IMAGES,
    &BUTTON_NEUTRAL_IMAGES,
    &BUTTON_DESTRUCTIVE_IMAGES,
    &PLAY_TAB_IMAGES,
];

/// Lists exact native artwork keys used by screens; hashed sleep names match the prefix above.
pub(crate) fn referenced_keys() -> impl Iterator<Item = &'static str> {
    REFERENCED_KEY_GROUPS
        .iter()
        .flat_map(|group| group.iter().copied())
}

/// A sprite keeps its original pixel rectangle on one packed page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OreUiSprite {
    pub page: u16,
    pub bounds: [u16; 4],
}

/// Original pixels and bundle paths are shared by every presentation instance.
#[derive(Clone)]
pub struct OreUiImages {
    pub pages: Vec<OreUiPage>,
    pub sprites: Arc<HashMap<String, OreUiSprite>>,
    /// Sprite keys and frame durations in milliseconds, in playback order.
    pub loading_frames: Arc<Vec<(String, u32)>>,
    pub animations: Arc<HashMap<String, Vec<(String, u32)>>>,
    pub source: Option<Arc<catalog::SourceCatalog>>,
}

#[derive(Clone)]
pub struct OreUiPage {
    pub dimensions: [u32; 2],
    pub pixels: Arc<[u8]>,
}

impl OreUiImages {
    /// Finds both prepared artwork and every discoverable native raster.
    pub fn contains(&self, key: &str) -> bool {
        self.sprites.contains_key(key)
            || self
                .source
                .as_ref()
                .is_some_and(|source| source.contains(key))
    }

    /// Prepares a screen's additional artwork without changing source resolution.
    pub fn with_artwork(&self, keys: &[&str]) -> Result<Self, String> {
        self.with_artwork_budget(keys, render_model::MAX_UI_FIXED_TEXTURE_BYTES)
    }

    pub(crate) fn with_artwork_budget(
        &self,
        keys: &[&str],
        byte_limit: usize,
    ) -> Result<Self, String> {
        let mut missing: Vec<_> = keys
            .iter()
            .filter(|key| !self.sprites.contains_key(**key))
            .map(|key| (*key).to_owned())
            .collect();
        missing.sort_unstable();
        missing.dedup();
        if missing.is_empty() {
            return Ok(self.clone());
        }
        let source = self
            .source
            .as_ref()
            .ok_or("OreUI source artwork is unavailable")?;
        let resident = self
            .pages
            .iter()
            .map(|page| page.pixels.len())
            .sum::<usize>();
        let available = byte_limit
            .checked_sub(resident)
            .ok_or("OreUI resident artwork exceeds the texture budget")?;
        let prepared = source.prepare(&missing, available)?;
        let offset = u16::try_from(self.pages.len()).map_err(|_| "OreUI texture page overflow")?;
        let mut sprites = (*self.sprites).clone();
        for (key, sprite) in &prepared.sprites {
            let mut sprite = *sprite;
            sprite.page = sprite
                .page
                .checked_add(offset)
                .ok_or("OreUI texture page overflow")?;
            sprites.insert(key.clone(), sprite);
        }
        let mut pages = self.pages.clone();
        pages.extend(prepared.pages.iter().cloned());
        let mut animations = (*self.animations).clone();
        animations.extend(prepared.animations.clone());
        Ok(Self {
            pages,
            sprites: Arc::new(sprites),
            animations: Arc::new(animations),
            loading_frames: self.loading_frames.clone(),
            source: self.source.clone(),
        })
    }
}

#[derive(Deserialize)]
struct AtlasFile {
    name: String,
    width: u32,
    height: u32,
    coordinates: HashMap<String, AtlasRect>,
}

#[derive(Deserialize)]
struct AtlasRect {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

/// Finds the installed bundle, with an explicit path taking precedence.
pub fn bundle_dir() -> Option<PathBuf> {
    if let Some(root) = std::env::var_os("CINNABAR_OREUI_LOCAL_ASSETS") {
        return bundle_at(Path::new(&root));
    }
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        roots.push(home.join(
            "Library/Containers/io.playcover.PlayCover/Applications/com.mojang.minecraftpe.app",
        ));
        roots.push(home.join("Applications/Minecraft.app"));
    }
    roots.push(PathBuf::from("/Applications/Minecraft.app"));
    roots.push(PathBuf::from(".local/assets/oreui"));
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        let packages = PathBuf::from(program_files).join("WindowsApps");
        if let Ok(entries) = std::fs::read_dir(packages) {
            roots.extend(entries.take(256).flatten().filter_map(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .filter(|name| name.starts_with("Microsoft.MinecraftUWP_"))
                    .map(|_| entry.path())
            }));
        }
    }
    roots.into_iter().find_map(|root| bundle_at(&root))
}

fn bundle_at(root: &Path) -> Option<PathBuf> {
    [
        root.to_path_buf(),
        root.join("data/gui/dist/hbui"),
        root.join("gui/dist/hbui"),
    ]
    .into_iter()
    .find(|dir| dir.join("atlas.json").is_file())
}

/// Decodes an installed bundle once; subsequent hosts share the original pixels.
pub fn load_optional_oreui_images() -> Option<OreUiImages> {
    static IMAGES: OnceLock<Option<OreUiImages>> = OnceLock::new();
    IMAGES
        .get_or_init(|| {
            let native = bundle_dir().and_then(|dir| match load(&dir) {
                Ok(images) => {
                    eprintln!(
                        "OreUI installed assets: {} ({} sprites, {} pages)",
                        dir.display(),
                        images.sprites.len(),
                        images.pages.len()
                    );
                    Some(images)
                }
                Err(reason) => {
                    eprintln!("OreUI bundle at {} unusable ({reason})", dir.display());
                    None
                }
            });
            let mut images = native.unwrap_or_else(|| OreUiImages {
                pages: Vec::new(),
                sprites: Default::default(),
                loading_frames: Default::default(),
                animations: Default::default(),
                source: None,
            });
            dimensions::install(&mut images);
            (!images.pages.is_empty()).then_some(images)
        })
        .clone()
}

fn load(dir: &Path) -> Result<OreUiImages, String> {
    let json = read_bounded(&dir.join("atlas.json"), MAX_ATLAS_JSON_BYTES)?;
    let atlases: Vec<AtlasFile> =
        serde_json::from_slice(&json).map_err(|error| format!("atlas.json: {error}"))?;
    let mut pack = packing::Pages::default();
    let mut sprites = HashMap::new();
    let required: HashSet<_> = referenced_keys().collect();
    let referenced = |key: &str| required.contains(key) || key.starts_with(SLEEP_ANIMATION_PREFIX);
    for atlas in &atlases {
        let mut coordinates: Vec<_> = atlas
            .coordinates
            .iter()
            .filter(|(key, _)| referenced(key))
            .collect();
        if coordinates.is_empty() {
            continue;
        }
        let (width, height, pixels) = decode(&bundle_path(dir, &atlas.name)?)?;
        if width != atlas.width || height != atlas.height {
            return Err(format!("{} does not match atlas.json", atlas.name));
        }
        coordinates.sort_unstable_by(|a, b| a.0.cmp(b.0));
        for (path, rect) in coordinates {
            if rect
                .x
                .checked_add(rect.width)
                .is_none_or(|right| right > width)
                || rect
                    .y
                    .checked_add(rect.height)
                    .is_none_or(|bottom| bottom > height)
                || rect.width == 0
                || rect.height == 0
            {
                continue;
            }
            let mut cropped = Vec::with_capacity((rect.width * rect.height * 4) as usize);
            for y in rect.y..rect.y + rect.height {
                let start = ((y * width + rect.x) * 4) as usize;
                cropped.extend_from_slice(&pixels[start..start + rect.width as usize * 4]);
            }
            sprites.insert(
                path.clone(),
                pack.insert(&cropped, rect.width, rect.height)?,
            );
            if is_mask(path) {
                sprites.insert(
                    format!("@mask/{path}"),
                    pack.insert(&alpha_mask(&cropped), rect.width, rect.height)?,
                );
            }
        }
    }
    let mut files = Vec::new();
    collect_rasters(dir, &dir.join("assets"), &mut files)?;
    let source = Arc::new(catalog::SourceCatalog::new(files.iter().cloned().collect()));
    let mut dimensions = Vec::new();
    for (key, path) in files {
        if !referenced(&key) || sprites.contains_key(&key) {
            continue;
        }
        let reader = image_reader(&path)?;
        let (width, height) = reader
            .into_dimensions()
            .map_err(|error| format!("{}: {error}", path.display()))?;
        dimensions.push((width, height, key, path));
    }
    dimensions.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)).then(a.2.cmp(&b.2)));
    let mut animations = HashMap::new();
    for (_, _, key, path) in dimensions {
        if path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("gif"))
        {
            let frames = decode_animation(&read_bounded(&path, MAX_IMAGE_BYTES)?)?;
            let mut animation = Vec::new();
            for (index, (width, height, pixels, millis)) in frames.into_iter().enumerate() {
                let sprite = pack.insert(&pixels, width, height)?;
                if index == 0 {
                    sprites.insert(key.clone(), sprite);
                }
                let frame = format!("{key}#{index}");
                sprites.insert(frame.clone(), sprite);
                animation.push((frame, millis));
            }
            animations.insert(key, animation);
        } else {
            let (width, height, pixels) = decode(&path)?;
            let sprite = pack.insert(&pixels, width, height)?;
            if is_mask(&key) {
                let mask = alpha_mask(&pixels);
                sprites.insert(format!("@mask/{key}"), pack.insert(&mask, width, height)?);
            }
            sprites.insert(key, sprite);
        }
    }
    let loading_frames = animations
        .get(LOADING_ANIMATION)
        .cloned()
        .unwrap_or_default();
    Ok(OreUiImages {
        pages: pack.finish(),
        sprites: Arc::new(sprites),
        loading_frames: Arc::new(loading_frames),
        animations: Arc::new(animations),
        source: Some(source),
    })
}

fn bundle_path(dir: &Path, key: &str) -> Result<PathBuf, String> {
    let path = Path::new(key);
    if path
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(format!("invalid OreUI asset path: {key}"));
    }
    Ok(dir.join(path))
}

fn collect_rasters(
    dir: &Path,
    at: &Path,
    files: &mut Vec<(String, PathBuf)>,
) -> Result<(), String> {
    let Ok(entries) = std::fs::read_dir(at) else {
        return Ok(());
    };
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            collect_rasters(dir, &entry.path(), files)?;
        } else if kind.is_file()
            && entry
                .path()
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| {
                    matches!(
                        ext.to_ascii_lowercase().as_str(),
                        "png" | "jpg" | "jpeg" | "gif" | "webp"
                    )
                })
        {
            if files.len() >= MAX_BUNDLE_FILES {
                return Err("OreUI bundle has too many raster assets".into());
            }
            let path = entry.path();
            let key = path
                .strip_prefix(dir)
                .map_err(|error| error.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            files.push((key, path));
        }
    }
    Ok(())
}

fn image_reader(path: &Path) -> Result<ImageReader<Cursor<Vec<u8>>>, String> {
    let bytes = read_bounded(path, MAX_IMAGE_BYTES)?;
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|error| error.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    Ok(reader)
}

fn is_mask(key: &str) -> bool {
    key.contains(".icon-") || key == SWITCH_ON_IMAGE || key == SWITCH_OFF_IMAGE
}

fn alpha_mask(pixels: &[u8]) -> Vec<u8> {
    pixels
        .chunks_exact(4)
        .flat_map(|pixel| [255, 255, 255, pixel[3]])
        .collect()
}

/// A decoded GIF frame: width, height, straight RGBA8 pixels and duration in milliseconds.
type AnimationFrame = (u32, u32, Vec<u8>, u32);

/// Decodes bounded GIF frames with their own timing and original pixels.
fn decode_animation(bytes: &[u8]) -> Result<Vec<AnimationFrame>, String> {
    let mut decoder = image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
        .map_err(|error| error.to_string())?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_SIDE);
    limits.max_image_height = Some(MAX_IMAGE_SIDE);
    limits.max_alloc = Some(MAX_ANIMATION_PIXELS_BYTES as u64);
    decoder
        .set_limits(limits)
        .map_err(|error| error.to_string())?;
    let mut frames = Vec::new();
    let mut decoded_bytes = 0;
    for frame in decoder.into_frames() {
        if frames.len() == MAX_ANIMATION_FRAMES {
            return Err("OreUI animation has too many frames".into());
        }
        let frame = frame.map_err(|error| error.to_string())?;
        let (numerator, denominator) = frame.delay().numer_denom_ms();
        let millis = numerator.div_ceil(denominator).max(1);
        let buffer = frame.into_buffer();
        let (width, height) = buffer.dimensions();
        let pixels = buffer.into_raw();
        decoded_bytes += pixels.len();
        if decoded_bytes > MAX_ANIMATION_PIXELS_BYTES {
            return Err("OreUI animation pixels are too large".into());
        }
        frames.push((width, height, pixels, millis));
    }
    Ok(frames)
}

fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, String> {
    let file = File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;
    if bytes.len() as u64 > max {
        return Err(format!("{} is too large", path.display()));
    }
    Ok(bytes)
}

/// The shader applies alpha, so uploaded pixels retain straight source RGB.
fn decode(path: &Path) -> Result<(u32, u32, Vec<u8>), String> {
    let reader = image_reader(path)?;
    let image = reader
        .decode()
        .map_err(|error| format!("{}: {error}", path.display()))?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Ok((width, height, image.into_raw()))
}

#[cfg(test)]
mod tests;
