//! A joined server's resource-pack UI textures: any pack-relative image in the
//! pack stack shadows the vanilla carrier's of the same path, and its `*.json`
//! sidecar shadows the carrier's independently. Each image is read, decoded and
//! shelf-packs into the reserved 256x256 dynamic pages only when a rendered
//! screen draws it; one larger than a page packs downscaled. A frame decodes
//! inline only within a small budget and hands the rest to workers; decoded
//! pixels are kept (bounded) so an evicted texture repacks without decoding
//! again. Undecodable images are skipped and remembered.

use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    io::Cursor,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

use image::{ImageReader, Limits};
use json_ui::{TextureMeta, parse_texture_meta};
use render_model::UiTexturePage;

use crate::remote_images::{RemoteImages, RemoteState, is_remote};

mod frame_sidecars;
mod prepared_settings;
mod preread;
pub use prepared_settings::PreparedScreenSettings;
pub use preread::PrereadTextures;

/// Image extensions a texture path may resolve to, in lookup order.
const IMAGE_EXTENSIONS: [&str; 4] = [".png", ".tga", ".jpg", ".jpeg"];
/// Where vanilla's in-package resource pack sits; its files read from the local
/// vanilla pack.
pub(super) const VANILLA_IN_PACKAGE: &str = "resource_packs/vanilla/";

/// Side of a dynamic UI page, which also bounds one server texture.
const PAGE_SIDE: u32 = 256;
const GUTTER: u32 = 1;

/// A session's server resource-pack UI: each pack's indexed definitions, lowest
/// precedence first (each layer merges over the ones below), and where its
/// image files read from.
#[derive(Clone, Debug, Default)]
pub struct ServerUiPack {
    pub ui_layers: Vec<Vec<(String, Vec<u8>)>>,
    /// Winning texture files read up front (fixture packs).
    pub textures: Vec<(String, Vec<u8>)>,
    /// The session's pack stack, which texture files read from on first draw.
    pub view: Option<resource_pack::LayeredPackView>,
    pub catalog: Option<Arc<json_ui::Catalog>>,
    /// Immutable screen policies prepared alongside the matching catalog.
    pub screen_settings: Option<Arc<PreparedScreenSettings>>,
}

impl ServerUiPack {
    /// Image extensions used by both pack reload tracking and texture loading.
    pub fn image_extensions() -> &'static [&'static str] {
        &IMAGE_EXTENSIONS
    }

    /// Image files JSON-UI can reference anywhere in an admitted pack.
    pub fn is_image_path(path: &str) -> bool {
        IMAGE_EXTENSIONS
            .iter()
            .any(|extension| path.ends_with(extension))
    }

    /// Resolves pack definitions on the reload worker against the immutable carrier catalog.
    pub fn prepare_catalog(&self, base: &json_ui::Catalog) -> Arc<Self> {
        let mut prepared = self.clone();
        let catalog = Arc::new(super::engine::layer_pack_catalog(base, &self.ui_layers));
        let textures = super::hud::prepared::prepare(&catalog);
        if let Some(view) = &self.view {
            super::hud::prepared::keep_textures(&catalog, PrereadTextures::read(view, &textures));
        }
        prepared.screen_settings = Some(Arc::new(PreparedScreenSettings::new(
            catalog.clone(),
            super::menu_screens::retail_context(),
        )));
        prepared.catalog = Some(catalog);
        Arc::new(prepared)
    }

    pub fn is_empty(&self) -> bool {
        self.ui_layers.iter().all(Vec::is_empty) && self.textures.is_empty()
    }
}

/// Largest texture file read from the pack stack.
pub use resource_pack::MAX_PACK_TEXTURE_BYTES;

/// The pack stack's images and sidecars by path stem, read lazily.
struct PackTextures {
    view: resource_pack::LayeredPackView,
    images: BTreeMap<String, String>,
    sidecars: BTreeMap<String, String>,
    loaded: RefCell<BTreeMap<String, Option<Source>>>,
    loaded_sidecars: RefCell<BTreeMap<String, Option<TextureMeta>>>,
}

impl PackTextures {
    fn index(view: resource_pack::LayeredPackView) -> Self {
        let mut images = BTreeMap::new();
        let mut sidecars = BTreeMap::new();
        let paths = view.list("");
        for &path in &paths {
            if let Some(stem) = path.strip_suffix(".json") {
                sidecars.insert(stem.to_owned(), path.to_owned());
            }
        }
        // `.png` wins over `.tga` and `.jpg`, so it inserts last.
        for extension in IMAGE_EXTENSIONS.iter().rev() {
            for &path in &paths {
                if let Some(stem) = path.strip_suffix(extension) {
                    images.insert(stem.to_owned(), path.to_owned());
                }
            }
        }
        Self {
            view,
            images,
            sidecars,
            loaded: RefCell::default(),
            loaded_sidecars: RefCell::default(),
        }
    }

    fn image(&self, key: &str) -> Option<Source> {
        if let Some(found) = self.loaded.borrow().get(key) {
            return found.clone();
        }
        #[cfg(test)]
        PACK_READS.with(|reads| reads.borrow_mut().push(key.to_owned()));
        let found = self.images.get(key).and_then(|path| {
            let bytes = self.view.read_capped(path, MAX_PACK_TEXTURE_BYTES)?;
            source(std::sync::Arc::from(bytes))
        });
        self.loaded
            .borrow_mut()
            .insert(key.to_owned(), found.clone());
        found
    }

    fn sidecar(&self, key: &str) -> Option<TextureMeta> {
        if let Some(found) = self.loaded_sidecars.borrow().get(key) {
            return *found;
        }
        let found = self.sidecars.get(key).and_then(|path| {
            let bytes = self.view.read_capped(path, MAX_PACK_TEXTURE_BYTES)?;
            let text = resource_pack::normalize_jsonc(&bytes)?;
            parse_texture_meta(&serde_json::from_slice(&text).ok()?)
        });
        self.loaded_sidecars
            .borrow_mut()
            .insert(key.to_owned(), found);
        found
    }
}

/// Where a resident server texture sits: its page (relative to the first
/// server page) and pixel rect.
#[derive(Clone, Copy, Debug)]
pub(super) struct ServerTexture {
    pub(super) page: u16,
    pub(super) rect: [u16; 4],
}

/// A pack texture known by its header, decoded only when a screen draws it.
/// One larger than a page packs downscaled to fit; UVs stay normalized.
#[derive(Clone)]
struct Source {
    bytes: std::sync::Arc<[u8]>,
    size: [u32; 2],
    packed: [u32; 2],
}

/// One reserved page: its pixels, shelf cursor, residents, and last use.
struct Page {
    pixels: Vec<u8>,
    cursor: [u32; 3],
    keys: Vec<String>,
    used: u64,
    image: UiTexturePage,
}

impl Page {
    fn blank() -> Option<Self> {
        let pixels = vec![0; (PAGE_SIDE * PAGE_SIDE * 4) as usize];
        let image = UiTexturePage::owned([PAGE_SIDE; 2], pixels.clone().into()).ok()?;
        Some(Self {
            pixels,
            cursor: [0; 3],
            keys: Vec::new(),
            used: 0,
            image,
        })
    }

    /// A shelf slot for `size`, advancing the cursor, or `None` when full.
    fn allocate(&mut self, size: [u32; 2]) -> Option<[u32; 2]> {
        let [mut x, mut y, mut shelf] = self.cursor;
        if x + size[0] > PAGE_SIDE {
            (x, y, shelf) = (0, y + shelf + GUTTER, 0);
        }
        if y + size[1] > PAGE_SIDE {
            return None;
        }
        self.cursor = [x + size[0] + GUTTER, y, shelf.max(size[1])];
        Some([x, y])
    }
}

/// The server pack's UI textures, packed on demand into a fixed number of
/// reserved pages: a texture becomes resident when a rendered screen draws it,
/// and a full atlas evicts its least recently drawn page.
#[derive(Default)]
pub(super) struct ServerAtlas {
    /// Images read up front, by path stem.
    sources: BTreeMap<String, Source>,
    /// Sidecars read up front, by path stem, whether or not the pack has the image.
    sidecars: BTreeMap<String, TextureMeta>,
    frames: frame_sidecars::FrameSidecars,
    pack: Option<PackTextures>,
    /// Vanilla images and downloaded URLs, found on first use; `None` when absent.
    extra: RefCell<BTreeMap<String, Option<Source>>>,
    /// Local vanilla sidecars inherit independently from the texture image.
    extra_sidecars: RefCell<BTreeMap<String, Option<TextureMeta>>>,
    /// The local vanilla resource pack vanilla image paths read from.
    vanilla: Option<PathBuf>,
    /// Referenced encoded images retained beside the carrier's UI JSON.
    carrier: Option<Arc<assets::RuntimeUiAssets>>,
    remote: Option<RemoteImages>,
    resident: BTreeMap<String, ServerTexture>,
    pages: Vec<Page>,
    images: Vec<UiTexturePage>,
    max_pages: usize,
    clock: u64,
    dirty: bool,
    /// Drawn textures too big for a page, for the full-resolution art pages.
    oversized: BTreeMap<String, std::sync::Arc<[u8]>>,
    decodes: Decodes,
}

/// Decode time a frame spends inline before spilling misses to workers.
const INLINE_DECODE_BUDGET: Duration = Duration::from_millis(2);
/// Decoded pixels kept for repacking evicted textures.
const MAX_DECODED_BYTES: usize = 32 * 1024 * 1024;
/// Remembered undecodable keys and settled fallback lookups.
const MAX_FAILED: usize = 256;
const MAX_EXTRA: usize = 512;

/// Decoded pixels by key (at their packed size), failures, and worker decodes.
struct Decodes {
    pixels: BTreeMap<String, Arc<Vec<u8>>>,
    order: VecDeque<String>,
    bytes: usize,
    failed: VecDeque<String>,
    pending: BTreeSet<String>,
    sender: crossbeam_channel::Sender<(String, Option<Vec<u8>>)>,
    receiver: crossbeam_channel::Receiver<(String, Option<Vec<u8>>)>,
}

impl Default for Decodes {
    fn default() -> Self {
        let (sender, receiver) = crossbeam_channel::unbounded();
        Self {
            pixels: BTreeMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            failed: VecDeque::new(),
            pending: BTreeSet::new(),
            sender,
            receiver,
        }
    }
}

impl Decodes {
    /// Files finished worker decodes.
    fn collect(&mut self) {
        while let Ok((key, pixels)) = self.receiver.try_recv() {
            self.pending.remove(&key);
            self.store(key, pixels);
        }
    }

    fn store(&mut self, key: String, pixels: Option<Vec<u8>>) {
        let Some(pixels) = pixels else {
            if self.failed.len() >= MAX_FAILED {
                self.failed.pop_front();
            }
            self.failed.push_back(key);
            return;
        };
        self.bytes += pixels.len();
        self.pixels.insert(key.clone(), Arc::new(pixels));
        self.order.push_back(key);
        while self.bytes > MAX_DECODED_BYTES
            && let Some(oldest) = self.order.pop_front()
        {
            if let Some(evicted) = self.pixels.remove(&oldest) {
                self.bytes -= evicted.len();
            }
        }
    }

    /// `key`'s pixels at `size`: cached, decoded inline while `inline`, else
    /// queued on a worker (`None` until a later frame).
    fn get(&mut self, key: &str, source: &Source, inline: bool) -> Option<Arc<Vec<u8>>> {
        if let Some(pixels) = self.pixels.get(key) {
            return Some(Arc::clone(pixels));
        }
        if self.failed.iter().any(|failed| failed == key) || self.pending.contains(key) {
            return None;
        }
        if inline {
            self.store(key.to_owned(), decode(&source.bytes, source.packed));
            return self.pixels.get(key).cloned();
        }
        self.pending.insert(key.to_owned());
        let (key, bytes, size, done) = (
            key.to_owned(),
            Arc::clone(&source.bytes),
            source.packed,
            self.sender.clone(),
        );
        rayon::spawn(move || {
            let _ = done.send((key, decode(&bytes, size)));
        });
        None
    }
}

impl ServerAtlas {
    /// Index up-front files' images (by path stem) and sidecars, and the pack
    /// stack's texture paths; nothing decodes.
    pub(super) fn new(
        files: &[(String, Vec<u8>)],
        view: Option<resource_pack::LayeredPackView>,
        max_pages: usize,
    ) -> Self {
        let sidecars = files
            .iter()
            .filter_map(|(path, bytes)| {
                let stem = path.strip_suffix(".json")?;
                let value = serde_json::from_slice(bytes).ok()?;
                Some((stem.to_owned(), parse_texture_meta(&value)?))
            })
            .collect();
        // A path names its image without an extension; `.png` wins over `.tga`
        // and `.jpg`, so it inserts last.
        let ranked = IMAGE_EXTENSIONS.iter().rev().flat_map(|extension| {
            files
                .iter()
                .filter_map(move |(path, bytes)| Some((path.strip_suffix(extension)?, bytes)))
        });
        let sources = ranked
            .filter_map(|(stem, bytes)| Some((stem.to_owned(), source(bytes.as_slice().into())?)))
            .collect::<BTreeMap<_, _>>();
        let frames = frame_sidecars::FrameSidecars::new(files, view.clone());
        let pack = view.map(PackTextures::index);
        Self {
            sources,
            sidecars,
            frames,
            pack,
            max_pages,
            dirty: true,
            ..Self::default()
        }
    }

    /// Also read vanilla images from `vanilla` and download remote ones.
    pub(super) fn with_fallbacks(
        mut self,
        vanilla: Option<PathBuf>,
        remote: Option<RemoteImages>,
    ) -> Self {
        self.frames.set_vanilla(vanilla.clone());
        self.vanilla = vanilla;
        self.remote = remote;
        self
    }

    pub(super) fn with_carrier(mut self, carrier: Option<Arc<assets::RuntimeUiAssets>>) -> Self {
        self.carrier = carrier;
        self
    }

    /// Whether the pack has an image at `key`, without reading it.
    pub(super) fn has_image(&self, key: &str) -> bool {
        self.sources.contains_key(key)
            || self
                .pack
                .as_ref()
                .is_some_and(|pack| pack.images.contains_key(key))
    }

    /// The pack's image at `key`, read on first use.
    fn image(&self, key: &str) -> Option<Source> {
        match self.sources.get(key) {
            Some(source) => Some(source.clone()),
            None => self.pack.as_ref()?.image(key),
        }
    }

    /// The pack image's pixel size; `None` when the pack lacks or cannot decode it.
    pub(super) fn image_size(&self, key: &str) -> Option<[f64; 2]> {
        self.image(key).map(|source| source.size.map(f64::from))
    }

    /// The pack's sidecar for `key`, which overrides a lower layer's whether or
    /// not the pack also replaces the image, as in vanilla.
    pub(super) fn sidecar(&self, key: &str) -> Option<TextureMeta> {
        self.sidecars
            .get(key)
            .copied()
            .or_else(|| self.pack.as_ref()?.sidecar(key))
    }

    pub(super) fn aseprite_frames(&self, key: &str) -> Option<Arc<[json_ui::AsepriteFrame]>> {
        self.frames.get(key)
    }

    /// Pixel size of a vanilla image or a downloaded URL, reading or requesting
    /// it on first use.
    pub(super) fn fallback_size(&self, key: &str) -> Option<[f64; 2]> {
        self.fallback(key)
            .as_ref()
            .map(|source| source.size.map(f64::from))
    }

    /// A local vanilla sidecar when neither the server pack nor carrier has it.
    /// Cache misses as well as hits, matching the bounded image fallback cache.
    pub(super) fn fallback_sidecar(&self, key: &str) -> Option<TextureMeta> {
        if let Some(found) = self.extra_sidecars.borrow().get(key) {
            return *found;
        }
        if is_remote(key) {
            return None;
        }
        let root = self.vanilla.as_ref()?;
        let relative = key.strip_prefix(VANILLA_IN_PACKAGE).unwrap_or(key);
        let found = (key.starts_with("textures/") || relative != key)
            .then(|| {
                let path = vanilla_path(root, &format!("{relative}.json"))?;
                exact_case(&path).then_some(())?;
                (std::fs::metadata(&path).ok()?.len() <= MAX_PACK_TEXTURE_BYTES).then_some(())?;
                let bytes = std::fs::read(path).ok()?;
                let text = resource_pack::normalize_jsonc(&bytes)?;
                let value = serde_json::from_slice(&text).ok()?;
                parse_texture_meta(&value)
            })
            .flatten();
        let mut extra = self.extra_sidecars.borrow_mut();
        if extra.len() >= MAX_EXTRA {
            extra.pop_first();
        }
        extra.insert(key.to_owned(), found);
        found
    }

    /// The vanilla image or downloaded URL behind `key`. A vanilla miss is
    /// remembered; a URL still loading is asked again next time.
    fn fallback(&self, key: &str) -> Option<Source> {
        if let Some(found) = self.extra.borrow().get(key) {
            return found.clone();
        }
        let (found, settled) = if is_remote(key) {
            match self.remote.as_ref()?.state(key) {
                RemoteState::Ready(bytes) => (source(bytes), true),
                RemoteState::Failed => (None, true),
                RemoteState::Loading => (None, false),
            }
        } else {
            let relative = key.strip_prefix(VANILLA_IN_PACKAGE).unwrap_or(key);
            let found = (key.starts_with("textures/") || relative != key).then(|| {
                self.carrier
                    .as_ref()
                    .and_then(|carrier| {
                        IMAGE_EXTENSIONS.iter().find_map(|extension| {
                            source(carrier.ui_file(&format!("{relative}{extension}"))?.into())
                        })
                    })
                    .or_else(|| {
                        let root = self.vanilla.as_ref()?;
                        IMAGE_EXTENSIONS.iter().find_map(|extension| {
                            let path = vanilla_path(root, &format!("{relative}{extension}"))?;
                            (std::fs::metadata(&path).ok()?.len() <= MAX_PACK_TEXTURE_BYTES)
                                .then_some(())?;
                            exact_case(&path).then_some(())?;
                            source(std::fs::read(path).ok()?.into())
                        })
                    })
            });
            (found.flatten(), true)
        };
        if settled {
            let mut extra = self.extra.borrow_mut();
            if extra.len() >= MAX_EXTRA {
                extra.pop_first();
            }
            extra.insert(key.to_owned(), found.clone());
        }
        found
    }

    pub(super) fn placement(&self, key: &str) -> Option<ServerTexture> {
        self.resident.get(key).copied()
    }

    /// Drawn textures a page had to shrink, by key, with their source bytes.
    pub(super) fn oversized(&self) -> Vec<(String, std::sync::Arc<[u8]>)> {
        self.oversized
            .iter()
            .map(|(key, bytes)| (key.clone(), std::sync::Arc::clone(bytes)))
            .collect()
    }

    /// Page images in order, for the reserved dynamic pages.
    pub(super) fn images(&self) -> &[UiTexturePage] {
        &self.images
    }

    /// `true` once since the page images last changed.
    pub(super) fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Make every texture in `keys` resident for this frame: pack textures, then
    /// vanilla images and downloaded URLs. A texture
    /// that fits nowhere without evicting one drawn this frame is left out.
    pub(super) fn require<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) {
        self.clock += 1;
        self.decodes.collect();
        // Mark what is already resident first, so a miss never evicts a page
        // this frame still draws.
        let mut missing = Vec::new();
        for key in keys {
            match self.resident.get(key) {
                Some(texture) => self.pages[usize::from(texture.page)].used = self.clock,
                None => missing.push(key),
            }
        }
        // Small backdrops and animation strips must precede expensive artwork.
        missing.sort_by_cached_key(|key| {
            let area = self
                .image(key)
                .or_else(|| self.fallback(key))
                .map_or(u64::MAX, |source| {
                    u64::from(source.size[0]) * u64::from(source.size[1])
                });
            (area, *key)
        });
        let inline_until = Instant::now() + INLINE_DECODE_BUDGET;
        let mut changed = Vec::new();
        for key in missing {
            if !self.resident.contains_key(key)
                && let Some(page) = self.place(key, Instant::now() < inline_until)
            {
                changed.push(page);
            }
        }
        changed.sort_unstable();
        changed.dedup();
        for page in changed {
            let page = &mut self.pages[page];
            if let Ok(image) = UiTexturePage::owned([PAGE_SIDE; 2], page.pixels.clone().into()) {
                page.image = image;
            }
        }
        if self.images.len() != self.pages.len()
            || self
                .images
                .iter()
                .zip(&self.pages)
                .any(|(image, page)| image.identity() != page.image.identity())
        {
            self.images = self.pages.iter().map(|page| page.image.clone()).collect();
            self.dirty = true;
        }
    }

    /// Pack `key`'s decoded pixels, returning the page it landed on; `None`
    /// while its decode is still on a worker.
    fn place(&mut self, key: &str, inline: bool) -> Option<usize> {
        let source = match self.image(key) {
            Some(source) => source,
            None => self.fallback(key)?,
        };
        if source.packed != source.size && self.oversized.len() < MAX_OVERSIZED {
            self.oversized
                .insert(key.to_owned(), std::sync::Arc::clone(&source.bytes));
        }
        let size = source.packed;
        let rgba = self.decodes.get(key, &source, inline)?;
        let (index, origin) = self.slot(size)?;
        let page = &mut self.pages[index];
        let row_bytes = PAGE_SIDE as usize * 4;
        let width = size[0] as usize * 4;
        for row in 0..size[1] as usize {
            let start = (origin[1] as usize + row) * row_bytes + origin[0] as usize * 4;
            page.pixels[start..start + width]
                .copy_from_slice(&rgba[row * width..(row + 1) * width]);
        }
        page.keys.push(key.to_owned());
        page.used = self.clock;
        let rect = [origin[0], origin[1], size[0], size[1]].map(|value| value as u16);
        self.resident.insert(
            key.to_owned(),
            ServerTexture {
                page: index as u16,
                rect,
            },
        );
        Some(index)
    }

    /// Room for `size`: an existing page, a new page, or the least recently
    /// drawn page not drawn this frame, cleared.
    fn slot(&mut self, size: [u32; 2]) -> Option<(usize, [u32; 2])> {
        for (index, page) in self.pages.iter_mut().enumerate() {
            if let Some(origin) = page.allocate(size) {
                return Some((index, origin));
            }
        }
        if self.pages.len() < self.max_pages {
            self.pages.push(Page::blank()?);
            let index = self.pages.len() - 1;
            return Some((index, self.pages[index].allocate(size)?));
        }
        let clock = self.clock;
        let (index, _) = self
            .pages
            .iter()
            .enumerate()
            .filter(|(_, page)| page.used < clock)
            .min_by_key(|(_, page)| page.used)?;
        let page = &mut self.pages[index];
        for key in page.keys.drain(..) {
            self.resident.remove(&key);
        }
        page.pixels.fill(0);
        page.cursor = [0; 3];
        Some((index, page.allocate(size)?))
    }
}

/// Resolves a pack-relative file while refusing parent components and linked escapes.
fn vanilla_path(root: &std::path::Path, relative: &str) -> Option<std::path::PathBuf> {
    use std::path::{Component, Path};
    let relative = Path::new(relative);
    if relative
        .components()
        .any(|part| !matches!(part, Component::Normal(_)))
        || relative.as_os_str().is_empty()
    {
        return None;
    }
    let root = root.canonicalize().ok()?;
    let path = root.join(relative);
    let canonical = path.canonicalize().ok()?;
    canonical.starts_with(&root).then_some(path)
}

/// Whether `path`'s file name exists spelled exactly so, as the client's asset
/// index matches even on a case-insensitive file system.
fn exact_case(path: &std::path::Path) -> bool {
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return false;
    };
    std::fs::read_dir(parent).is_ok_and(|mut entries| {
        entries.any(|entry| entry.is_ok_and(|entry| entry.file_name() == name))
    })
}

/// A decodable image as a source.
fn source(bytes: std::sync::Arc<[u8]>) -> Option<Source> {
    let size = dimensions(&bytes)?;
    Some(Source {
        bytes,
        size,
        packed: fitted(size),
    })
}

/// Oversized textures remembered for the art pages.
const MAX_OVERSIZED: usize = 16;

/// Largest source side decoded, as a desktop texture allows; bigger images are skipped.
const MAX_SOURCE_SIDE: u32 = 16_384;
/// Largest source area decoded (a 4096 square), bounding decode memory.
const MAX_SOURCE_PIXELS: u64 = 4096 * 4096;

/// A png's size from its header, when within the decode bound.
fn dimensions(bytes: &[u8]) -> Option<[u32; 2]> {
    let (width, height) = reader(bytes)?.into_dimensions().ok()?;
    (width > 0
        && height > 0
        && width <= MAX_SOURCE_SIDE
        && height <= MAX_SOURCE_SIDE
        && u64::from(width) * u64::from(height) <= MAX_SOURCE_PIXELS)
        .then_some([width, height])
}

/// A reader for a pack image by its content, as the vanilla loader detects it:
/// a `.png` path may hold TGA data, which carries no magic number.
fn reader(bytes: &[u8]) -> Option<ImageReader<Cursor<&[u8]>>> {
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    if reader.format().is_none() {
        reader.set_format(image::ImageFormat::Tga);
    }
    Some(reader)
}

/// `size` scaled down, keeping its aspect, to fit one page.
fn fitted(size: [u32; 2]) -> [u32; 2] {
    let largest = size[0].max(size[1]);
    if largest <= PAGE_SIDE {
        return size;
    }
    size.map(|side| (u64::from(side) * u64::from(PAGE_SIDE) / u64::from(largest)).max(1) as u32)
}

/// RGBA8 pixels of a bounded image at `size`, or `None` when undecodable.
fn decode(bytes: &[u8], size: [u32; 2]) -> Option<Vec<u8>> {
    let mut reader = reader(bytes)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba8();
    let image = if [image.width(), image.height()] == size {
        image
    } else {
        image::imageops::resize(
            &image,
            size[0],
            size[1],
            image::imageops::FilterType::Triangle,
        )
    };
    Some(image.into_raw())
}

#[cfg(test)]
#[path = "server_pack/external_paths_tests.rs"]
mod external_paths_tests;

#[cfg(test)]
thread_local! {
    /// Image keys this thread looked up in the pack archives, so tests can tell a frame's reads
    /// from a worker's.
    static PACK_READS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests;
