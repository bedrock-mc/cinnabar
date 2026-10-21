//! A joined server's resource-pack UI textures: any pack-relative image in the
//! pack stack shadows the vanilla carrier's of the same path, and its `*.json`
//! sidecar shadows the carrier's independently. Each image is read, decoded and
//! shelf-packs into the reserved 256x256 dynamic pages only when a rendered
//! screen draws it; one larger than a page packs downscaled. A frame decodes
//! inline only within a small budget on the host's clock and hands the rest to
//! workers, or to a later frame when the host has none; decoded pixels are kept
//! (bounded) so an evicted texture repacks without decoding again. Undecodable
//! images are skipped and remembered.

use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    io::Cursor,
    sync::Arc,
    time::Duration,
};

use image::{ImageReader, Limits};
use json_ui::{TextureMeta, parse_texture_meta};
use render_model::UiTexturePage;

use crate::sources::{ImageWork, LocalPack, RemoteImage, RemoteSource, is_remote};

pub(crate) mod fitted;
mod frame_sidecars;
pub mod jobs;
mod local_sources;
mod outlines;
mod oversized;
pub use oversized::OversizedTextures;
mod client_rasters;
pub use client_rasters::LOCAL_PLAYER_HEAD;
pub mod native_sheets;
pub mod preread;
mod source_reads;
mod source_reset;

/// Image extensions a texture path may resolve to, in lookup order.
pub const IMAGE_EXTENSIONS: [&str; 4] = [".png", ".tga", ".jpg", ".jpeg"];
/// Where vanilla's in-package resource pack sits; its files read from the local
/// vanilla pack.
pub const VANILLA_IN_PACKAGE: &str = "resource_packs/vanilla/";

/// Client-shipped images by extensionless pack path; server packs shadow them.
pub type BuiltinImages = &'static [(&'static str, &'static [u8])];

/// Side of a dynamic UI page, which also bounds one server texture.
pub(crate) const PAGE_SIDE: u32 = 256;
const GUTTER: u32 = 1;

/// Largest texture file read from the pack stack.
use resource_pack::MAX_PACK_TEXTURE_BYTES;

/// The pack stack's images and sidecars by path stem, read lazily.
struct PackTextures {
    view: resource_pack::LayeredPackView,
    images: BTreeMap<String, String>,
    sidecars: BTreeMap<String, String>,
    loaded: RefCell<BTreeMap<String, Option<Source>>>,
    reads: source_reads::SourceReads,
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
            reads: source_reads::SourceReads::default(),
            loaded_sidecars: RefCell::default(),
        }
    }

    /// Resolves small sources inline and queues large archive entries using only indexed sizes.
    #[cfg(test)]
    fn image(&self, key: &str) -> Option<Source> {
        self.image_capped(key, true, || self.read_limit())
    }

    /// Shares the large-source allowance with installed image fallbacks; without `workers` a
    /// large entry reads on this thread.
    fn image_capped(
        &self,
        key: &str,
        workers: bool,
        limit: impl FnOnce() -> u64,
    ) -> Option<Source> {
        if let Some(found) = self.loaded.borrow().get(key) {
            return found.clone();
        }
        let path = self.images.get(key)?;
        let size = self
            .view
            .layers()
            .filter_map(|layer| layer.file_size(path))
            .max()?;
        if size > MAX_PACK_TEXTURE_BYTES {
            if workers {
                self.reads.request(key, path, &self.view, limit());
                return None;
            }
            let found = read_source(&self.view, key, path, limit());
            self.loaded
                .borrow_mut()
                .insert(key.to_owned(), found.clone());
            return found;
        }
        self.read_image(key)
    }

    /// Caps each readable layer by the remaining large-source budget without charging shadowed copies.
    fn read_limit(&self) -> u64 {
        let bytes: usize = self
            .loaded
            .borrow()
            .values()
            .flatten()
            .filter(|source| source.bytes.len() as u64 > MAX_PACK_TEXTURE_BYTES)
            .map(|source| source.bytes.len())
            .sum();
        render_model::MAX_UI_NATIVE_SHEET_BYTES
            .saturating_sub(bytes)
            .max(MAX_PACK_TEXTURE_BYTES as usize)
            .min(resource_pack::MAX_FILE_BYTES as usize) as u64
    }

    /// Reads on the reload worker or for a small first-use entry, retaining the result once.
    fn read_image(&self, key: &str) -> Option<Source> {
        if let Some(found) = self.loaded.borrow().get(key) {
            return found.clone();
        }
        let found = self
            .images
            .get(key)
            .and_then(|path| read_source(&self.view, key, path, self.read_limit()));
        self.loaded
            .borrow_mut()
            .insert(key.to_owned(), found.clone());
        found
    }

    /// A large declared override keeps its draw pending until its header read completes.
    fn read_pending(&self, key: &str) -> bool {
        !self.loaded.borrow().contains_key(key)
            && self.images.get(key).is_some_and(|path| {
                self.view.layers().any(|layer| {
                    layer
                        .file_size(path)
                        .is_some_and(|size| size > MAX_PACK_TEXTURE_BYTES)
                })
            })
    }

    /// Admits worker-read metadata once so retained frames can be invalidated before painting.
    #[cfg(test)]
    fn collect(&self) -> bool {
        self.collect_capped(|| self.read_limit())
    }

    /// Rechecks admission after other source owners may have consumed the allowance.
    fn collect_capped(&self, limit: impl FnOnce() -> u64) -> bool {
        let Some((key, source)) = self.reads.collect() else {
            return false;
        };
        let limit = limit();
        self.loaded.borrow_mut().insert(
            key,
            source.filter(|source| source.bytes.len() as u64 <= limit),
        );
        true
    }

    fn sidecar(&self, key: &str) -> Option<TextureMeta> {
        if let Some(found) = self.loaded_sidecars.borrow().get(key) {
            return *found;
        }
        let found = self.sidecars.get(key).and_then(|path| {
            let bytes = self.view.read_capped(path, MAX_PACK_TEXTURE_BYTES)?;
            let text = pack_json::normalize_jsonc(&bytes)?;
            parse_texture_meta(&serde_json::from_slice(&text).ok()?)
        });
        self.loaded_sidecars
            .borrow_mut()
            .insert(key.to_owned(), found);
        found
    }
}

/// Reads one admitted image and its header on the source worker or a bounded small-image path.
fn read_source(
    view: &resource_pack::LayeredPackView,
    key: &str,
    path: &str,
    limit: u64,
) -> Option<Source> {
    #[cfg(any(test, feature = "test-support"))]
    PACK_READS.with(|reads| reads.borrow_mut().push(key.to_owned()));
    #[cfg(not(any(test, feature = "test-support")))]
    let _ = key;
    source(Arc::from(view.read_capped(path, limit)?))
}

/// Where a resident server texture sits: its page (relative to the first
/// server page) and pixel rect.
#[derive(Clone, Copy, Debug)]
pub struct ServerTexture {
    pub page: u16,
    pub rect: [u16; 4],
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
pub struct ServerAtlas {
    /// Prepared client artwork, owned by the screen rather than the pack stack.
    client_rasters: BTreeMap<String, UiTexturePage>,
    /// Images read up front, by path stem.
    sources: BTreeMap<String, Source>,
    /// Sidecars read up front, by path stem, whether or not the pack has the image.
    sidecars: BTreeMap<String, TextureMeta>,
    frames: frame_sidecars::FrameSidecars,
    pack: Option<PackTextures>,
    /// Vanilla images and downloaded URLs, found on first use; `None` when absent.
    extra: RefCell<BTreeMap<String, Option<Source>>>,
    local_reads: source_reads::SourceReads,
    /// Local vanilla sidecars inherit independently from the texture image.
    extra_sidecars: RefCell<BTreeMap<String, Option<TextureMeta>>>,
    /// The local vanilla resource pack vanilla image paths read from.
    vanilla: Option<Arc<dyn LocalPack>>,
    /// Referenced encoded images retained beside the carrier's UI JSON.
    carrier: Option<Arc<assets::RuntimeUiAssets>>,
    builtin: BuiltinImages,
    remote: Option<Arc<dyn RemoteSource>>,
    resident: BTreeMap<String, ServerTexture>,
    pages: Vec<Page>,
    images: Vec<UiTexturePage>,
    max_pages: usize,
    clock: u64,
    dirty: bool,
    metadata_revision: u64,
    metadata_dirty: bool,
    /// Drawn textures too big for a page, for the full-resolution art pages.
    oversized: OversizedTextures,
    decodes: Decodes,
    native_enabled: bool,
    native: native_sheets::NativeSheets,
    outlines: outlines::Outlines,
    work: ImageWork,
}

/// Decode time a frame spends inline before spilling misses to workers or a later frame.
const INLINE_DECODE_BUDGET: Duration = Duration::from_millis(2);
/// Decoded pixels kept for repacking evicted textures.
const MAX_DECODED_BYTES: usize = 32 * 1024 * 1024;
/// Remembered undecodable keys and settled fallback lookups.
const MAX_FAILED: usize = 256;
const MAX_EXTRA: usize = 512;

/// Decoded pixels by key (at their packed size), failures, and worker decodes.
#[derive(Default)]
struct Decodes {
    pixels: BTreeMap<String, Arc<Vec<u8>>>,
    order: VecDeque<String>,
    bytes: usize,
    failed: VecDeque<String>,
    jobs: fitted::FittedDecodes,
}

/// Where a missing texture's decode runs this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecodeOn {
    Inline,
    Worker,
    /// Past the inline budget with no workers: a later frame decodes it.
    Later,
}

impl DecodeOn {
    /// Small encodings decode inline within the budget; workers take the rest, and without
    /// workers everything decodes inline once the budget allows.
    fn choose(workers: bool, within_budget: bool, encoded_bytes: usize) -> Self {
        match (workers, within_budget) {
            (true, true) if encoded_bytes as u64 <= MAX_PACK_TEXTURE_BYTES => Self::Inline,
            (true, _) => Self::Worker,
            (false, true) => Self::Inline,
            (false, false) => Self::Later,
        }
    }
}

impl Decodes {
    /// Files finished worker decodes.
    fn collect(&mut self) {
        while let Some((key, pixels)) = self.jobs.collect() {
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

    /// `key`'s pixels at `size`: cached, decoded inline, queued on a worker,
    /// or left for a later frame (`None` until then).
    fn get(&mut self, key: &str, source: &Source, decode_on: DecodeOn) -> Option<Arc<Vec<u8>>> {
        if let Some(pixels) = self.pixels.get(key) {
            return Some(Arc::clone(pixels));
        }
        if self.failed.iter().any(|failed| failed == key) || self.jobs.pending() == Some(key) {
            return None;
        }
        match decode_on {
            DecodeOn::Inline => {
                self.store(key.to_owned(), decode(&source.bytes, source.packed));
                return self.pixels.get(key).cloned();
            }
            DecodeOn::Later => return None,
            DecodeOn::Worker => {}
        }
        self.jobs.request(key, source);
        None
    }
}

impl ServerAtlas {
    /// Index up-front files' images (by path stem) and sidecars, and the pack
    /// stack's texture paths; nothing decodes.
    pub fn new(
        files: &[(String, Vec<u8>)],
        view: Option<resource_pack::LayeredPackView>,
        max_pages: usize,
    ) -> Self {
        let sidecars = files
            .iter()
            .filter_map(|(path, bytes)| {
                let stem = path.strip_suffix(".json")?;
                let value = pack_json::parse_jsonc(bytes).ok()?;
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
    pub fn with_fallbacks(
        mut self,
        vanilla: Option<Arc<dyn LocalPack>>,
        remote: Option<Arc<dyn RemoteSource>>,
    ) -> Self {
        if !source_reset::same_owner(&self.vanilla, &vanilla)
            || !source_reset::same_owner(&self.remote, &remote)
        {
            self.reset_fallback_artwork();
        }
        self.frames.set_vanilla(vanilla.clone());
        self.vanilla = vanilla;
        self.remote = remote;
        self
    }

    pub fn with_carrier(mut self, carrier: Option<Arc<assets::RuntimeUiAssets>>) -> Self {
        if !source_reset::same_owner(&self.carrier, &carrier) {
            self.reset_fallback_artwork();
        }
        self.carrier = carrier;
        self
    }

    /// Serves `images` below the server pack and above the carrier and local pack.
    pub fn with_builtin_images(mut self, images: BuiltinImages) -> Self {
        if !std::ptr::eq(self.builtin, images) {
            self.reset_fallback_artwork();
        }
        self.builtin = images;
        self
    }

    /// Runs image work on the host's clock and threads.
    pub fn with_image_work(mut self, work: ImageWork) -> Self {
        self.work = work;
        self
    }

    /// Enables the original-image slots owned by the main presentation catalog.
    pub fn with_native_sheets(mut self, enabled: bool) -> Self {
        self.native_enabled = enabled;
        self
    }

    /// Whether this source requires original tiles instead of a fitted artwork image.
    pub fn is_native_sheet(&self, key: &str) -> bool {
        self.native_enabled
            && !self.native.failed(key)
            && match self.native_source(key) {
                Some(source) => source
                    .size
                    .into_iter()
                    .any(|side| side > crate::pages::WHOLE_ART_PAGE),
                None => self
                    .pack
                    .as_ref()
                    .is_some_and(|pack| pack.read_pending(key)),
            }
    }

    /// Requests draw-sized source coverage independently of GPU sheets, outside painting.
    pub(crate) fn prepare_outline(&mut self, key: &str, size: [u32; 2]) {
        if !self.outlines.needs(key, size) {
            return;
        }
        let carrier = (!self.has_image(key))
            .then(|| {
                let carrier = self.carrier.as_ref()?;
                let slot = carrier.texture(key)?;
                Some(outlines::Input::Carrier {
                    page: carrier.atlas_pages().get(usize::from(slot.page))?.clone(),
                    bounds: [slot.x, slot.y, slot.width, slot.height].map(u32::from),
                })
            })
            .flatten();
        if let Some(input) =
            carrier.or_else(|| self.native_source(key).map(outlines::Input::Encoded))
        {
            self.outlines.request(key, size, input, self.work.workers);
        }
    }

    /// Reads a ready draw-sized mask without doing source work during paint.
    pub(crate) fn original_outline(
        &self,
        key: &str,
        size: [u32; 2],
    ) -> Option<Arc<render_api::sprite_mask::SpriteMask>> {
        self.outlines.get(key, size)
    }

    /// Whether an original virtual image has finished its one-time decode.
    pub fn native_sheet_ready(&self, key: &str) -> bool {
        self.native.get(key).is_some()
    }

    /// The original image when its complete extent is representable by one sprite.
    pub fn native_sprite(&self, key: &str) -> Option<(usize, [f32; 4])> {
        self.native.sprite(key)
    }

    /// Original-image fragments sampled by the authored UV rectangle.
    pub fn native_quads(
        &self,
        key: &str,
        uv: json_ui::UvRect,
        dest: [f32; 4],
    ) -> Option<native_sheets::Quads<'_>> {
        let (sheet, first) = self.native.get(key)?;
        Some(sheet.quads(uv, dest, first))
    }

    /// Collects image completions before a retained menu can reuse its saved frame.
    pub fn poll_native_images(&mut self) -> bool {
        let ready = self
            .pack
            .as_ref()
            .is_some_and(|pack| pack.collect_capped(|| self.source_read_limit()))
            | self.collect_local_source();
        if ready {
            self.metadata_revision = self.metadata_revision.wrapping_add(1);
            self.metadata_dirty = true;
        }
        ready | self.native.collect() | self.outlines.collect()
    }

    /// Changes to image dimensions must rebuild normalized UVs, aspect ratios and slice geometry.
    pub fn metadata_revision(&self) -> u64 {
        self.metadata_revision
    }

    /// Keeps readiness observed during painting visible to the next retained-frame poll.
    pub fn take_metadata_changed(&mut self) -> bool {
        std::mem::take(&mut self.metadata_dirty)
    }

    /// Original virtual-image pages, separate from the ordinary small atlas.
    pub fn native_pages(&self) -> &[UiTexturePage] {
        self.native.pages()
    }

    /// Whether the pack has an image at `key`, without reading it.
    pub fn has_image(&self, key: &str) -> bool {
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
            None => self
                .pack
                .as_ref()?
                .image_capped(key, self.work.workers, || self.source_read_limit()),
        }
    }

    /// A declared server source waits for its read instead of caching a lower layer under its key.
    fn native_source(&self, key: &str) -> Option<Source> {
        if self.has_image(key) {
            self.image(key)
        } else {
            self.fallback(key)
        }
    }

    /// The pack image's pixel size; `None` when the pack lacks or cannot decode it.
    pub fn image_size(&self, key: &str) -> Option<[f64; 2]> {
        self.image(key).map(|source| source.size.map(f64::from))
    }

    /// The pack's sidecar for `key`, which overrides a lower layer's whether or
    /// not the pack also replaces the image, as in vanilla.
    pub fn sidecar(&self, key: &str) -> Option<TextureMeta> {
        self.sidecars
            .get(key)
            .copied()
            .or_else(|| self.pack.as_ref()?.sidecar(key))
    }

    pub fn aseprite_frames(&self, key: &str) -> Option<Arc<[json_ui::AsepriteFrame]>> {
        self.frames.get(key)
    }

    /// Pixel size of a vanilla image or a downloaded URL, reading or requesting
    /// it on first use.
    pub fn fallback_size(&self, key: &str) -> Option<[f64; 2]> {
        if let Some(image) = self.client_rasters.get(key) {
            return Some(image.dimensions().map(f64::from));
        }
        self.fallback(key)
            .as_ref()
            .map(|source| source.size.map(f64::from))
    }

    /// A local vanilla sidecar when neither the server pack nor carrier has it.
    /// Cache misses as well as hits, matching the bounded image fallback cache.
    pub fn fallback_sidecar(&self, key: &str) -> Option<TextureMeta> {
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
                let path = format!("{relative}.json");
                (root.file_size(&path)? <= MAX_PACK_TEXTURE_BYTES).then_some(())?;
                let bytes = root.read(&path, MAX_PACK_TEXTURE_BYTES)?;
                let text = pack_json::normalize_jsonc(&bytes)?;
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
                RemoteImage::Ready(bytes) => (source(bytes), true),
                RemoteImage::Failed => (None, true),
                RemoteImage::Loading => (None, false),
            }
        } else {
            self.local_source(key)
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

    pub fn placement(&self, key: &str) -> Option<ServerTexture> {
        self.resident.get(key).copied()
    }

    /// Drawn textures a page had to shrink, by key, with their source bytes.
    pub fn oversized(&self) -> OversizedTextures {
        self.oversized.clone()
    }

    /// Page images in order, for the reserved dynamic pages.
    pub fn images(&self) -> &[UiTexturePage] {
        &self.images
    }

    /// `true` once since the page images last changed.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty) | self.native.take_dirty()
    }

    /// Make every texture in `keys` resident for this frame: pack textures, then
    /// vanilla images and downloaded URLs. A texture
    /// that fits nowhere without evicting one drawn this frame is left out.
    pub fn require<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) {
        self.clock += 1;
        self.require_current(keys);
    }

    /// Adds prepared artwork without making earlier draws in this paint eligible for eviction.
    pub(crate) fn require_current<'a>(&mut self, keys: impl IntoIterator<Item = &'a str>) {
        self.decodes.collect();
        self.poll_native_images();
        // Mark what is already resident first, so a miss never evicts a page
        // this frame still draws.
        let mut missing = Vec::new();
        for key in keys {
            if self.is_native_sheet(key) {
                let Some(source) = self.native_source(key) else {
                    continue;
                };
                self.native.request(key, &source, self.work.workers);
                // One-sprite consumers share original pages when possible and retain their
                // bounded fitted artwork fallback for images spanning multiple tiles.
                if native_sheets::fits_single_tile(source.size) || !preview_allowed(source.size) {
                    continue;
                }
            }
            match self.resident.get(key) {
                Some(texture) => self.pages[usize::from(texture.page)].used = self.clock,
                None => missing.push(key),
            }
        }
        // Small backdrops and animation strips must precede expensive artwork.
        missing.sort_by_cached_key(|key| {
            if let Some(image) = self.client_rasters.get(*key) {
                let [w, h] = image.dimensions();
                return (u64::from(w) * u64::from(h), *key);
            }
            let area = self
                .image(key)
                .or_else(|| self.fallback(key))
                .map_or(u64::MAX, |source| {
                    u64::from(source.size[0]) * u64::from(source.size[1])
                });
            (area, *key)
        });
        let deadline = self.work.deadline(INLINE_DECODE_BUDGET);
        let mut changed = Vec::new();
        for key in missing {
            if !self.resident.contains_key(key)
                && let Some(page) = self.place(key, self.work.before(deadline))
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
    /// while its decode is still on a worker or waits for a later frame.
    fn place(&mut self, key: &str, within_budget: bool) -> Option<usize> {
        let (size, rgba) = if let Some(image) = self.client_rasters.get(key) {
            (image.dimensions(), Arc::new(image.pixels().to_vec()))
        } else {
            let source = match self.image(key) {
                Some(source) => source,
                None => self.fallback(key)?,
            };
            if source.packed != source.size {
                self.oversized.insert(key, &source.bytes);
            }
            let size = source.packed;
            let decode_on = DecodeOn::choose(self.work.workers, within_budget, source.bytes.len());
            let rgba = self.decodes.get(key, &source, decode_on)?;
            (size, rgba)
        };
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
            if self
                .resident
                .get(&key)
                .is_some_and(|slot| usize::from(slot.page) == index)
            {
                self.resident.remove(&key);
            }
        }
        page.pixels.fill(0);
        page.cursor = [0; 3];
        Some((index, page.allocate(size)?))
    }
}

/// A decodable image as a source.
fn source(bytes: std::sync::Arc<[u8]>) -> Option<Source> {
    (bytes.len() as u64 <= resource_pack::MAX_FILE_BYTES).then_some(())?;
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
const MAX_SOURCE_SIDE: u32 = render_model::MAX_UI_NATIVE_SHEET_SIDE;
/// Original-image bytes remain within the independent virtual-sheet budget.
const MAX_SOURCE_PIXELS: u64 = render_model::MAX_UI_NATIVE_SHEET_BYTES as u64 / 4;

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

/// Keeps fitted compatibility images within their existing decoded-area envelope.
fn preview_allowed(size: [u32; 2]) -> bool {
    u64::from(size[0]) * u64::from(size[1]) <= u64::from(render_model::MAX_UI_TEXTURE_SIDE).pow(2)
}

/// RGBA8 pixels of a bounded image at `size`, or `None` when undecodable.
fn decode(bytes: &[u8], size: [u32; 2]) -> Option<Vec<u8>> {
    let original = dimensions(bytes)?;
    if !preview_allowed(original) {
        return None;
    }
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

#[cfg(any(test, feature = "test-support"))]
thread_local! {
    /// Image keys this thread looked up in the pack archives, so tests can tell a frame's reads
    /// from a worker's.
    pub static PACK_READS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
thread_local! {
    /// Installed image files read by this thread.
    static LOCAL_READS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

#[cfg(test)]
mod tests;
