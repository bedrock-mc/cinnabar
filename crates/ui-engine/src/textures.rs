//! Where a JSON-UI texture path draws from, in the vanilla lookup's spirit: the
//! server pack first, then the UI carrier, then the item icon atlas already on
//! the UI texture array, and last vanilla images from the local pack or remote
//! URLs, packed on demand into the reserved server pages. Paths match exactly,
//! as the client's preloaded asset index does, so vanilla's `textures/ui/White`
//! (only `white.png` ships) is unresolved and draws the default white texture.

use std::{
    borrow::Cow,
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};

use assets::RuntimeUiAssets;
use json_ui::{NineSlice, TextureMeta, TextureSource};

use crate::atlas::{BuiltinImages, ServerAtlas, native_sheets::Quads};
use crate::revision::{AtlasRevision, Revision};
use crate::sources::{ImageWork, LocalPack, RemoteSource, is_remote};
use ui::IconRef;

/// The start screen's title texture, which Cinnabar's own logo replaces.
pub const TITLE_KEY: &str = "textures/ui/title";
/// Prepared local branding has its own original-art key; base/server title ownership stays distinct.
pub const MOD_MENU_TITLE_KEY: &str = "local-mod:menu-title";

/// Texture sources a form engine owns across frames.
pub struct TextureSet {
    pub(crate) masks: Mutex<crate::engine::masks::MaskCache>,
    /// A lock only because renders borrow the engine shared.
    atlas: Mutex<ServerAtlas>,
    /// Preserves metadata revision changes across atlas replacement.
    metadata_base: u64,
    /// Item icon atlas sprites by lowercase item texture path.
    icons: HashMap<String, IconRef>,
    /// Texture page of carrier atlas page 0.
    pub first_page: u16,
    /// Texture page of the first reserved server page.
    pub server_page: u16,
    vanilla: Option<Arc<dyn LocalPack>>,
    /// Encoded carrier images too large for its sprite atlas.
    carrier: Option<Arc<RuntimeUiAssets>>,
    builtin: BuiltinImages,
    pub remote: Arc<dyn RemoteSource>,
    /// The host's clock and threads every installed atlas runs its image work on.
    work: ImageWork,
    /// Full-resolution art-page copies of server textures too big for a server page.
    full_res: HashMap<String, IconRef>,
    /// Source ownership for the installed full-resolution mapping across atlas moves.
    full_res_sources: crate::atlas::OversizedTextures,
    /// A client part's screen: only portable `textures/` paths, never a URL.
    confined: bool,
    /// Changes whenever the icons, full-resolution art or installed atlas change.
    revision: Revision,
}

/// Identifies everything a paint resolves textures from; any change yields a new value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TextureRevision {
    set: Revision,
    atlas: AtlasRevision,
    /// The carrier's and the server atlas's first texture pages.
    pages: [u16; 2],
}

impl TextureSet {
    /// An empty set over carrier pages from `first_page`, downloading URLs through `remote` and
    /// running image work as `work` allows.
    pub fn new(first_page: u16, remote: Arc<dyn RemoteSource>, work: ImageWork) -> Self {
        let pages = crate::pages::SERVER_UI_PAGES;
        Self {
            masks: Default::default(),
            atlas: Mutex::new(
                ServerAtlas::new(&[], None, pages)
                    .with_native_sheets(true)
                    .with_image_work(work.clone()),
            ),
            metadata_base: 0,
            icons: HashMap::new(),
            first_page,
            server_page: 0,
            vanilla: None,
            carrier: None,
            builtin: &[],
            remote,
            work,
            full_res: HashMap::new(),
            full_res_sources: Default::default(),
            confined: false,
            revision: Default::default(),
        }
    }

    pub fn with_carrier(mut self, carrier: Arc<RuntimeUiAssets>) -> Self {
        self.carrier = Some(carrier);
        let atlas = std::mem::take(self.atlas_mut());
        self.set_atlas(atlas, self.server_page);
        self
    }

    /// Serves the screen families' shipped `images` from every installed atlas.
    pub fn with_builtin_images(mut self, images: BuiltinImages) -> Self {
        self.builtin = images;
        let atlas = std::mem::take(self.atlas_mut());
        self.set_atlas(atlas, self.server_page);
        self
    }

    pub fn lock(&self) -> MutexGuard<'_, ServerAtlas> {
        self.atlas
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub fn atlas_mut(&mut self) -> &mut ServerAtlas {
        self.atlas
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Where oversized server textures' full-resolution copies sit, by texture key.
    pub fn set_full_res(&mut self, full_res: HashMap<String, IconRef>) {
        self.full_res = full_res;
        self.full_res_sources = self.oversized();
        self.revision.bump();
    }

    /// A package screen's sources: its `files` over the carrier, the icon atlas and the local
    /// vanilla pack, packed into the `pages` dynamic pages from `page`. The server pack, remote
    /// URLs and paths that leave `textures/` are out of reach.
    pub fn confined(&self, files: &[(String, Vec<u8>)], page: u16, pages: usize) -> Self {
        let atlas = ServerAtlas::new(files, None, pages)
            .with_fallbacks(self.vanilla.clone(), None)
            .with_carrier(self.carrier.clone())
            .with_builtin_images(self.builtin)
            .with_image_work(self.work.clone());
        Self {
            masks: Default::default(),
            atlas: Mutex::new(atlas),
            metadata_base: self.metadata_revision().wrapping_add(1),
            icons: self.icons.clone(),
            first_page: self.first_page,
            server_page: page,
            vanilla: self.vanilla.clone(),
            carrier: self.carrier.clone(),
            builtin: self.builtin,
            remote: Arc::clone(&self.remote),
            work: self.work.clone(),
            full_res: HashMap::new(),
            full_res_sources: Default::default(),
            confined: true,
            revision: Default::default(),
        }
    }
    /// Drawn textures too big for a server page, with their source bytes.
    pub fn oversized(&self) -> crate::atlas::OversizedTextures {
        self.lock().oversized()
    }

    /// Install a server atlas, wired to the vanilla and remote fallbacks.
    pub fn set_atlas(&mut self, atlas: ServerAtlas, server_page: u16) {
        *self
            .masks
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner()) = Default::default();
        self.metadata_base = self.metadata_revision().wrapping_add(1);
        let atlas = atlas
            .with_native_sheets(!self.confined)
            .with_fallbacks(self.vanilla.clone(), Some(self.remote.clone()))
            .with_carrier(self.carrier.clone())
            .with_builtin_images(self.builtin)
            .with_image_work(self.work.clone());
        if !self.full_res_sources.same(&atlas.oversized()) {
            self.full_res.clear();
            self.full_res_sources = Default::default();
        }
        self.atlas = Mutex::new(atlas);
        self.server_page = server_page;
        self.revision.bump();
    }

    /// The current source metadata epoch, including atlas replacements.
    pub fn metadata_revision(&self) -> u64 {
        self.metadata_base
            .wrapping_add(self.lock().metadata_revision())
    }

    /// What a paint holding `atlas`, this set's locked atlas, resolves textures from.
    pub(crate) fn revision(&self, atlas: &ServerAtlas) -> TextureRevision {
        TextureRevision {
            set: self.revision,
            atlas: atlas.revision(),
            pages: [self.first_page, self.server_page],
        }
    }

    /// Item icons by texture path and the local vanilla pack; the atlas
    /// restarts so both apply.
    pub fn set_fallbacks(&mut self, icons: HashMap<String, IconRef>, vanilla: Arc<dyn LocalPack>) {
        self.icons = icons
            .into_iter()
            .map(|(path, icon)| (path.to_ascii_lowercase(), icon))
            .collect();
        self.vanilla = Some(vanilla);
        let atlas = std::mem::take(self.atlas_mut());
        let server_page = self.server_page;
        self.set_atlas(atlas, server_page);
    }
}

/// One phase's view of the sources: the carrier plus the locked atlas.
pub struct Textures<'a> {
    pub assets: &'a RuntimeUiAssets,
    pub set: &'a TextureSet,
    pub atlas: &'a ServerAtlas,
    /// Downloaded menu artwork by local path, drawn ahead of every other source.
    pub images: Option<&'a HashMap<String, IconRef>>,
}

impl<'a> Textures<'a> {
    /// `path` without an image extension, the key every source uses.
    pub fn canonical<'p>(&self, path: &'p str) -> Cow<'p, str> {
        Cow::Borrowed(texture_key(path))
    }

    /// A confined set reads only portable `textures/` paths: no URL, no `..`, no other root.
    pub(crate) fn admits(&self, path: &str) -> bool {
        !self.set.confined
            || path.strip_prefix("textures/").is_some_and(|rest| {
                rest.split('/').all(|part| {
                    !part.is_empty()
                        && part != "."
                        && part != ".."
                        && part
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
                })
            })
    }

    /// The item icon at `key`, matched case-insensitively against the lowercased icon paths;
    /// only a key with uppercase letters is copied to lowercase it.
    fn icon(&self, key: &str) -> Option<IconRef> {
        let icons = &self.set.icons;
        if key.bytes().any(|byte| byte.is_ascii_uppercase()) {
            icons.get(&key.to_ascii_lowercase()).copied()
        } else {
            icons.get(key).copied()
        }
    }

    fn image(&self, path: &str) -> Option<IconRef> {
        // Cinnabar's shipped logo is a base-pack replacement. Actual server
        // titles still win, including while their pixels decode asynchronously.
        if texture_key(path) == TITLE_KEY && self.atlas.has_image(texture_key(path)) {
            return None;
        }
        let images = self.images?;
        images
            .get(path)
            .or_else(|| images.get(texture_key(path)))
            .copied()
    }

    /// The drawn paths the server atlas must hold: pack textures, and what
    /// neither the carrier nor the icon atlas already has. Keys borrow from `paths`.
    pub fn atlas_keys<'p>(&self, paths: impl Iterator<Item = &'p str>) -> Vec<Cow<'p, str>> {
        paths
            .filter(|path| self.admits(path) && self.image(path).is_none())
            .map(|path| self.canonical(path))
            .filter(|key| {
                self.atlas.has_image(key)
                    || (self.assets.texture(key).is_none() && self.icon(key).is_none())
            })
            .collect()
    }

    /// Whether no source has `path`; a URL still loading, or a local file the artwork atlas has not
    /// packed, is not missing, so it draws nothing instead of white.
    pub fn missing(&self, path: &str) -> bool {
        let key = texture_key(path);
        !self.admits(path)
            || !is_remote(key)
                && !std::path::Path::new(path).is_absolute()
                && self.images.is_none_or(|images| !images.contains_key(path))
                && !self.atlas.has_image(key)
                && self.assets.texture(key).is_none()
                && self.icon(key).is_none()
                && self.atlas.fallback_size(key).is_none()
    }

    /// The texture page and pixel rect `path` draws from.
    pub fn sprite(&self, path: &str) -> Option<(u16, [f32; 4])> {
        if !self.admits(path) {
            return None;
        }
        if let Some(image) = self.image(path) {
            let [u0, v0, u1, v1] = image.uv.map(f32::from);
            return Some((image.page, [u0, v0, u1 - u0, v1 - v0]));
        }
        let key = self.canonical(path);
        if self.native_sheet(path)
            && let Some((offset, rect)) = self.atlas.native_sprite(&key)
        {
            let page = self.native_first_page()?.checked_add(offset)?;
            return Some((u16::try_from(page).ok()?, rect));
        }
        if let Some(art) = self.set.full_res.get(key.as_ref()) {
            let [u0, v0, u1, v1] = art.uv.map(f32::from);
            return Some((art.page, [u0, v0, u1 - u0, v1 - v0]));
        }
        if let Some(server) = self.atlas.placement(&key) {
            return Some((
                self.set.server_page.saturating_add(server.page),
                server.rect.map(f32::from),
            ));
        }
        if let Some(placement) = self.assets.texture(&key) {
            return Some((
                self.set.first_page.saturating_add(placement.page),
                [placement.x, placement.y, placement.width, placement.height].map(f32::from),
            ));
        }
        if let Some(icon) = self.icon(&key) {
            let [u0, v0, u1, v1] = icon.uv.map(f32::from);
            return Some((icon.page, [u0, v0, u1 - u0, v1 - v0]));
        }
        None
    }

    /// Frame strips wait for artwork rather than a preview that merges adjacent frames.
    pub fn animation_sprite(&self, path: &str) -> Option<(u16, [f32; 4])> {
        let sprite = self.sprite(path)?;
        if self.set.full_res.contains_key(texture_key(path)) {
            return Some(sprite);
        }
        let pixels = self.texture(path)?.pixels;
        ([f64::from(sprite.1[2]), f64::from(sprite.1[3])] == pixels).then_some(sprite)
    }

    /// Whether animation resource readiness comes from the native sheet or ordinary artwork.
    pub fn animation_ready(&self, path: &str) -> bool {
        if self.native_sheet(path) {
            self.atlas.native_sheet_ready(&self.canonical(path))
        } else {
            self.animation_sprite(path).is_some()
        }
    }

    /// Original images bypass fitted previews while their worker prepares exact texels.
    pub fn native_sheet(&self, path: &str) -> bool {
        let key = self.canonical(path);
        !self.set.confined
            && self.image(path).is_none()
            && self.admits(path)
            && (self.atlas.has_image(&key)
                || (self.assets.texture(&key).is_none() && self.icon(&key).is_none()))
            && self.atlas.is_native_sheet(&key)
    }

    /// The original-image slots rebased into this presentation's logical catalog.
    fn native_first_page(&self) -> Option<usize> {
        Some(
            usize::from(self.set.server_page).checked_sub(crate::pages::SERVER_UI_PAGE)?
                + render_model::UI_NATIVE_SHEET_PAGE_OFFSET,
        )
    }

    /// Maps a virtual image to its reserved logical pages without changing source UV units.
    pub fn native_quads(
        &self,
        path: &str,
        uv: json_ui::UvRect,
        dest: [f32; 4],
    ) -> Option<Quads<'a>> {
        let quads = self.atlas.native_quads(&self.canonical(path), uv, dest)?;
        let first = self.native_first_page()?;
        Some(quads.rebased(first))
    }
}

impl TextureSource for Textures<'_> {
    /// Lets every retained screen observe asynchronous dimensions and source replacement.
    fn metadata_revision(&self) -> u64 {
        self.set
            .metadata_base
            .wrapping_add(self.atlas.metadata_revision())
    }

    fn aseprite_frames(&self, path: &str) -> Option<Arc<[json_ui::AsepriteFrame]>> {
        self.admits(path).then_some(())?;
        self.atlas.aseprite_frames(&self.canonical(path))
    }

    fn texture(&self, path: &str) -> Option<TextureMeta> {
        if !self.admits(path) {
            return None;
        }
        let key = self.canonical(path);
        let key = key.as_ref();
        // The image and its sidecar each come from the highest layer that has
        // them: the server pack, then the carrier. An image promoted to the art
        // pages keeps its source size and sidecar.
        let size = self
            .image(path)
            .map(|image| {
                let [u0, v0, u1, v1] = image.uv.map(f64::from);
                [u1 - u0, v1 - v0]
            })
            .or_else(|| self.atlas.image_size(key))
            .or_else(|| {
                let placement = self.assets.texture(key)?;
                Some([f64::from(placement.width), f64::from(placement.height)])
            })
            .or_else(|| {
                let [u0, v0, u1, v1] = self.icon(key)?.uv.map(f64::from);
                Some([u1 - u0, v1 - v0])
            })
            .or_else(|| self.atlas.fallback_size(key))?;
        let sidecar = self
            .atlas
            .sidecar(key)
            .or_else(|| {
                let sidecar = self.assets.sidecar(key)?;
                Some(TextureMeta {
                    base_size: sidecar.base_size.map(f64::from),
                    pixels: size,
                    nineslice: sidecar.nineslice.map(|inset| NineSlice {
                        left: f64::from(inset.left),
                        top: f64::from(inset.top),
                        right: f64::from(inset.right),
                        bottom: f64::from(inset.bottom),
                    }),
                })
            })
            .or_else(|| self.atlas.fallback_sidecar(key));
        // A sidecar without `base_size` measures in the image's pixels.
        Some(match sidecar {
            Some(meta) => TextureMeta {
                base_size: if meta.base_size == [0.0, 0.0] {
                    size
                } else {
                    meta.base_size
                },
                pixels: size,
                ..meta
            },
            None => TextureMeta::plain(size),
        })
    }
}

/// Ui json sometimes spells a texture with its file extension; sources key it
/// without one. A URL keeps its whole spelling.
pub fn texture_key(path: &str) -> &str {
    if is_remote(path) {
        return path;
    }
    for extension in [".png", ".jpg", ".jpeg", ".tga"] {
        if let Some(stem) = path.strip_suffix(extension) {
            return stem;
        }
    }
    path
}

/// Downloads that never resolve.
#[cfg(test)]
struct NoDownloads;

#[cfg(test)]
impl RemoteSource for NoDownloads {
    fn state(&self, _: &str) -> crate::sources::RemoteImage {
        crate::sources::RemoteImage::Failed
    }
}

/// A UI carrier with one blank atlas page and no textures.
#[cfg(test)]
fn test_carrier() -> Arc<RuntimeUiAssets> {
    let files = [
        ("ui/_global_variables.json", "{}"),
        ("ui/_ui_defs.json", r#"{ "ui_defs": [] }"#),
    ]
    .map(|(path, text)| assets::UiFile {
        path: path.into(),
        bytes: text.as_bytes().into(),
    });
    let page = assets::UiAtlasPage {
        width: 4,
        height: 4,
        rgba8: vec![255; 64].into(),
    };
    let bytes = assets::encode_ui_catalog([1; 32], &[page], &[], &[], &files).unwrap();
    Arc::new(RuntimeUiAssets::decode(&bytes).unwrap())
}

#[cfg(test)]
#[path = "textures/animation_tests.rs"]
mod animation_tests;

#[cfg(test)]
mod review_tests {
    use {super::*, ui::IconRef};
    #[test]
    fn outline_cache_reuses_source_alpha_when_unrelated_atlas_art_changes() {
        let path = "textures/ui/mask_source";
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(9, 9, image::Rgba([30, 50, 70, 128]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let assets = test_carrier();
        let set = TextureSet::new(0, Arc::new(NoDownloads), ImageWork::inline());
        let mut atlas = ServerAtlas::new(&[(format!("{path}.png"), png)], None, 1)
            .with_image_work(ImageWork::inline());
        atlas.require([path]);
        atlas.prepare_outline(path, [9, 9]);
        let first = Textures {
            assets: &assets,
            set: &set,
            atlas: &atlas,
            images: None,
        }
        .outline_mask(path, [9, 9])
        .unwrap();
        let other = "client-raster:other/test";
        atlas.set_client_rasters(
            "client-raster:other/",
            &std::collections::BTreeMap::from([(
                other.into(),
                render_model::UiTexturePage::owned([8; 2], vec![255; 8 * 8 * 4].into()).unwrap(),
            )]),
        );
        atlas.require([path, other]);
        let second = Textures {
            assets: &assets,
            set: &set,
            atlas: &atlas,
            images: None,
        }
        .outline_mask(path, [9, 9])
        .unwrap();
        assert!(
            Arc::ptr_eq(&first, &second),
            "unchanged source alpha must not be derived again"
        );
    }

    #[test]
    fn outline_uses_original_pixels_when_the_drawn_sprite_is_a_fitted_preview() {
        use render_api::sprite_mask::{MaskLayout, MaskPatch};
        let path = "textures/ui/large_mask";
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(512, 512, image::Rgba([255; 4]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let assets = test_carrier();
        let set = TextureSet::new(0, Arc::new(NoDownloads), ImageWork::inline());
        let mut atlas = ServerAtlas::new(&[(format!("{path}.png"), png)], None, 1)
            .with_image_work(ImageWork::inline());
        atlas.require([path]);
        assert_eq!(atlas.placement(path).unwrap().rect[2..], [256, 256]);
        atlas.prepare_outline(path, [512, 512]);
        let view = Textures {
            assets: &assets,
            set: &set,
            atlas: &atlas,
            images: None,
        };
        let source = view.outline_mask(path, [512, 512]).unwrap();
        assert!(Arc::ptr_eq(
            &source,
            &view.outline_mask(path, [512, 512]).unwrap()
        ));
        let pixels = MaskLayout {
            source,
            extent: [512.; 2],
            size: [512; 2],
            patches: vec![MaskPatch {
                bounds: [0., 0., 512., 512.],
                uv: [0., 0., 1., 1.],
                opacity: 255,
            }],
        }
        .rasterize()
        .unwrap();
        assert_eq!(pixels[(10 * 512) * 4 + 3], 255);
        assert_eq!(
            pixels[(10 * 512 + 1) * 4 + 3],
            0,
            "a fitted preview would incorrectly double the border width"
        );
    }

    #[test]
    fn outline_paint_waits_for_worker_completion_without_generating_source_pixels() {
        use render_api::sprite_mask::{MaskLayout, MaskPatch};
        let path = "textures/ui/worker_mask";
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(4096, 9, image::Rgba([255; 4]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let assets = test_carrier();
        let set = TextureSet::new(0, Arc::new(NoDownloads), ImageWork::default());
        let mut atlas = ServerAtlas::new(&[(format!("{path}.png"), png.into_inner())], None, 1)
            .with_native_sheets(true)
            .with_image_work(ImageWork::inline());
        atlas.require([path]);
        assert!(atlas.native_sheet_ready(path));
        atlas = atlas.with_image_work(ImageWork::default());
        atlas.prepare_outline(path, [4096, 9]);
        let paint = |atlas: &ServerAtlas| {
            Textures {
                assets: &assets,
                set: &set,
                atlas,
                images: None,
            }
            .outline_mask(path, [4096, 9])
        };
        for _ in 0..3 {
            assert!(
                paint(&atlas).is_none(),
                "paint must skip a source whose worker result has not been collected"
            );
        }
        test_time::eventually("source outline worker", || {
            atlas.poll_native_images();
            paint(&atlas).is_some()
        });
        let pixels = MaskLayout {
            source: paint(&atlas).unwrap(),
            extent: [4096., 9.],
            size: [4096, 9],
            patches: vec![MaskPatch {
                bounds: [0., 0., 4096., 9.],
                uv: [0., 0., 1., 1.],
                opacity: 255,
            }],
        }
        .rasterize()
        .unwrap();
        assert_eq!(pixels[(4 * 4096) * 4 + 3], 255);
        assert_eq!(pixels[(4 * 4096 + 1) * 4 + 3], 0);
        let boundary = render_model::MAX_UI_TEXTURE_SIDE - 2;
        for x in [boundary - 1, boundary] {
            assert_eq!(pixels[((4 * 4096 + x) * 4 + 3) as usize], 0);
        }
    }

    #[test]
    fn outline_preparation_reads_carrier_bounds_and_retires_replaced_sources() {
        use render_api::sprite_mask::{MaskLayout, MaskPatch};
        let path = "textures/ui/carrier_mask";
        let make_carrier = |alpha| {
            let page = assets::UiAtlasPage {
                width: 8,
                height: 8,
                rgba8: [10, 20, 30, alpha].repeat(64).into(),
            };
            let slot = assets::UiTexturePlacement {
                path: path.into(),
                page: 0,
                x: 2,
                y: 3,
                width: 3,
                height: 3,
            };
            let bytes = assets::encode_ui_catalog([alpha; 32], &[page], &[slot], &[], &[]).unwrap();
            Arc::new(RuntimeUiAssets::decode(&bytes).unwrap())
        };
        let assets = make_carrier(128);
        let mut atlas = ServerAtlas::new(&[], None, 1)
            .with_carrier(Some(assets.clone()))
            .with_image_work(ImageWork::inline());
        assert!(atlas.original_outline(path, [3, 3]).is_none());
        atlas.prepare_outline(path, [3, 3]);
        let source = atlas.original_outline(path, [3, 3]).unwrap();
        let pixels = MaskLayout {
            source: source.clone(),
            extent: [3.; 2],
            size: [3; 2],
            patches: vec![MaskPatch {
                bounds: [0., 0., 3., 3.],
                uv: [0., 0., 1., 1.],
                opacity: 255,
            }],
        }
        .rasterize()
        .unwrap();
        assert_eq!(pixels[3], 128, "the crop edge is a source edge");
        assert_eq!(pixels[4 * 4 + 3], 0);
        atlas = atlas.with_carrier(Some(assets));
        assert!(Arc::ptr_eq(
            &source,
            &atlas.original_outline(path, [3, 3]).unwrap()
        ));
        atlas = atlas.with_carrier(Some(make_carrier(255)));
        assert!(atlas.original_outline(path, [3, 3]).is_none());
        atlas.prepare_outline(path, [3, 3]);
        assert!(!Arc::ptr_eq(
            &source,
            &atlas.original_outline(path, [3, 3]).unwrap()
        ));
    }

    #[test]
    fn outline_draw_size_changes_wait_for_their_own_prepared_coverage() {
        let path = "textures/ui/resized_mask";
        let mut png = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(72, 72, image::Rgba([255; 4]))
            .write_to(&mut png, image::ImageFormat::Png)
            .unwrap();
        let mut atlas = ServerAtlas::new(&[(format!("{path}.png"), png.into_inner())], None, 1)
            .with_image_work(ImageWork::inline());
        atlas.prepare_outline(path, [18; 2]);
        let first = atlas.original_outline(path, [18; 2]).unwrap();
        assert!(atlas.original_outline(path, [36; 2]).is_none());
        assert!(
            atlas.poll_native_images(),
            "inline completion must also invalidate retained frames"
        );
        assert!(!atlas.poll_native_images());
        atlas.prepare_outline(path, [36; 2]);
        assert!(!Arc::ptr_eq(
            &first,
            &atlas.original_outline(path, [36; 2]).unwrap()
        ));
        assert!(Arc::ptr_eq(
            &first,
            &atlas.original_outline(path, [18; 2]).unwrap()
        ));
    }

    #[test]
    fn review_texture_metadata_uses_the_drawn_artwork_dimensions() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(16, 16, image::Rgba([255; 4]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let assets = test_carrier();
        let set = TextureSet::new(0, Arc::new(NoDownloads), ImageWork::default());
        let atlas = ServerAtlas::new(&[("textures/ui/test.png".into(), png)], None, 1);
        let images = HashMap::from([(
            "textures/ui/test".into(),
            IconRef {
                page: 7,
                uv: [0, 0, 256, 128],
                glint: false,
            },
        )]);
        let textures = Textures {
            assets: &assets,
            set: &set,
            atlas: &atlas,
            images: Some(&images),
        };
        assert_eq!(
            textures.sprite("textures/ui/test").unwrap().1[2..],
            [256.0, 128.0]
        );
        assert_eq!(
            textures.texture("textures/ui/test").unwrap().pixels,
            [256.0, 128.0]
        );
    }

    // Offer art past the atlas drew vanilla's white instead of nothing.
    #[test]
    fn an_unpacked_local_file_is_not_missing() {
        let assets = test_carrier();
        let set = TextureSet::new(0, Arc::new(NoDownloads), ImageWork::default());
        let atlas = ServerAtlas::new(&[], None, 1);
        let textures = Textures {
            assets: &assets,
            set: &set,
            atlas: &atlas,
            images: None,
        };
        // A rooted path without a drive is not absolute on Windows.
        let local = std::env::temp_dir().join("store-images").join("a.jpg");
        assert!(!textures.missing(local.to_str().unwrap()));
        assert!(textures.missing("textures/ui/White"));
    }
}
