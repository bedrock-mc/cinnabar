//! Where a JSON-UI texture path draws from, in the vanilla lookup's spirit: the
//! server pack first, then the UI carrier, then the item icon atlas already on
//! the UI texture array, and last vanilla images from the local pack or remote
//! URLs, packed on demand into the reserved server pages. Paths match exactly,
//! as the client's preloaded asset index does, so vanilla's `textures/ui/White`
//! (only `white.png` ships) is unresolved and draws the default white texture.

use std::{
    borrow::Cow,
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use assets::RuntimeUiAssets;
use json_ui::{NineSlice, TextureMeta, TextureSource};

use super::remote_images::{RemoteImages, is_remote};
use super::server_pack::ServerAtlas;
use ui::IconRef;

/// Texture sources a form engine owns across frames.
#[derive(Default)]
pub(super) struct TextureSet {
    /// A lock only because renders borrow the engine shared.
    atlas: Mutex<ServerAtlas>,
    /// Item icon atlas sprites by lowercase item texture path.
    icons: HashMap<String, IconRef>,
    /// Texture page of carrier atlas page 0.
    pub(super) first_page: u16,
    /// Texture page of the first reserved server page.
    pub(super) server_page: u16,
    vanilla: Option<PathBuf>,
    /// Encoded carrier images too large for its sprite atlas.
    carrier: Option<Arc<RuntimeUiAssets>>,
    pub(super) remote: RemoteImages,
    /// Full-resolution art-page copies of server textures too big for a server page.
    full_res: HashMap<String, IconRef>,
    /// A client part's screen: only portable `textures/` paths, never a URL.
    confined: bool,
}

impl TextureSet {
    pub(super) fn new(first_page: u16) -> Self {
        let pages = super::super::dynamic_textures::SERVER_UI_PAGES;
        Self {
            atlas: Mutex::new(ServerAtlas::new(&[], None, pages)),
            first_page,
            ..Self::default()
        }
    }

    pub(super) fn with_carrier(mut self, carrier: Arc<RuntimeUiAssets>) -> Self {
        self.carrier = Some(carrier);
        let atlas = std::mem::take(self.atlas_mut());
        self.set_atlas(atlas, self.server_page);
        self
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, ServerAtlas> {
        self.atlas
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    pub(super) fn atlas_mut(&mut self) -> &mut ServerAtlas {
        self.atlas
            .get_mut()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Where oversized server textures' full-resolution copies sit, by texture key.
    pub(super) fn set_full_res(&mut self, full_res: HashMap<String, IconRef>) {
        self.full_res = full_res;
    }

    /// A package screen's sources: its `files` over the carrier, the icon atlas and the local
    /// vanilla pack, packed into the `pages` dynamic pages from `page`. The server pack, remote
    /// URLs and paths that leave `textures/` are out of reach.
    pub(super) fn confined(&self, files: &[(String, Vec<u8>)], page: u16, pages: usize) -> Self {
        let atlas = ServerAtlas::new(files, None, pages)
            .with_fallbacks(self.vanilla.clone(), None)
            .with_carrier(self.carrier.clone());
        Self {
            atlas: Mutex::new(atlas),
            icons: self.icons.clone(),
            first_page: self.first_page,
            server_page: page,
            vanilla: self.vanilla.clone(),
            carrier: self.carrier.clone(),
            remote: RemoteImages::default(),
            full_res: HashMap::new(),
            confined: true,
        }
    }
    /// Drawn textures too big for a server page, with their source bytes.
    pub(super) fn oversized(&self) -> Vec<(String, std::sync::Arc<[u8]>)> {
        self.lock().oversized()
    }

    /// Install a server atlas, wired to the vanilla and remote fallbacks.
    pub(super) fn set_atlas(&mut self, atlas: ServerAtlas, server_page: u16) {
        let atlas = atlas
            .with_fallbacks(self.vanilla.clone(), Some(self.remote.clone()))
            .with_carrier(self.carrier.clone());
        self.atlas = Mutex::new(atlas);
        self.server_page = server_page;
        self.full_res.clear();
    }

    /// Item icons by texture path and the local vanilla pack; the atlas
    /// restarts so both apply.
    pub(super) fn set_fallbacks(&mut self, icons: HashMap<String, IconRef>, vanilla: PathBuf) {
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
pub(super) struct Textures<'a> {
    pub(super) assets: &'a RuntimeUiAssets,
    pub(super) set: &'a TextureSet,
    pub(super) atlas: &'a ServerAtlas,
    /// Downloaded menu artwork by local path, drawn ahead of every other source.
    pub(super) images: Option<&'a HashMap<String, IconRef>>,
}

impl Textures<'_> {
    /// `path` without an image extension, the key every source uses.
    pub(super) fn canonical<'p>(&self, path: &'p str) -> Cow<'p, str> {
        Cow::Borrowed(texture_key(path))
    }

    /// A confined set reads only portable `textures/` paths: no URL, no `..`, no other root.
    fn admits(&self, path: &str) -> bool {
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

    fn icon(&self, key: &str) -> Option<IconRef> {
        self.set.icons.get(&key.to_ascii_lowercase()).copied()
    }

    fn image(&self, path: &str) -> Option<IconRef> {
        // Cinnabar's shipped logo is a base-pack replacement. Actual server
        // titles still win, including while their pixels decode asynchronously.
        if texture_key(path) == super::super::menu_artwork::TITLE_KEY
            && self.atlas.has_image(texture_key(path))
        {
            return None;
        }
        let images = self.images?;
        images
            .get(path)
            .or_else(|| images.get(texture_key(path)))
            .copied()
    }

    /// The drawn paths the server atlas must hold: pack textures, and what
    /// neither the carrier nor the icon atlas already has.
    pub(super) fn atlas_keys<'p>(&self, paths: impl Iterator<Item = &'p str>) -> Vec<String> {
        paths
            .filter(|path| self.admits(path) && self.image(path).is_none())
            .map(|path| self.canonical(path))
            .filter(|key| {
                self.atlas.has_image(key)
                    || (self.assets.texture(key).is_none() && self.icon(key).is_none())
            })
            .map(Cow::into_owned)
            .collect()
    }

    /// Whether no source has `path`; a URL still loading, or a local file the artwork atlas has not
    /// packed, is not missing, so it draws nothing instead of white.
    pub(super) fn missing(&self, path: &str) -> bool {
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
    pub(super) fn sprite(&self, path: &str) -> Option<(u16, [f32; 4])> {
        if !self.admits(path) {
            return None;
        }
        if let Some(image) = self.image(path) {
            let [u0, v0, u1, v1] = image.uv.map(f32::from);
            return Some((image.page, [u0, v0, u1 - u0, v1 - v0]));
        }
        let key = self.canonical(path);
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
    pub(super) fn animation_sprite(&self, path: &str) -> Option<(u16, [f32; 4])> {
        let sprite = self.sprite(path)?;
        if self.set.full_res.contains_key(texture_key(path)) {
            return Some(sprite);
        }
        let pixels = self.texture(path)?.pixels;
        ([f64::from(sprite.1[2]), f64::from(sprite.1[3])] == pixels).then_some(sprite)
    }
}

impl TextureSource for Textures<'_> {
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
pub(super) fn texture_key(path: &str) -> &str {
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

#[cfg(test)]
#[path = "textures/animation_tests.rs"]
mod animation_tests;

#[cfg(test)]
mod review_tests {
    use {super::*, ui::IconRef};
    #[test]
    fn review_texture_metadata_uses_the_drawn_artwork_dimensions() {
        let mut png = Vec::new();
        image::RgbaImage::from_pixel(16, 16, image::Rgba([255; 4]))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let assets = super::super::tests::mini_carrier();
        let set = TextureSet::new(0);
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
        let assets = super::super::tests::mini_carrier();
        let set = TextureSet::new(0);
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
