//! Bounded decoding and atlas packing for service-provided launcher artwork.
//!
//! The authenticated Go catalog downloads remote images into the local cache.
//! This module treats those files as untrusted input: reads, decoded dimensions,
//! allocation and output pages are all capped before artwork enters the
//! retained UI texture array's full-resolution art pages.

use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    fs::File,
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

use crossbeam_channel::{Receiver, Sender};

use image::{ImageReader, Limits, imageops::FilterType};

use super::IconRef;

const MAX_SOURCE_BYTES: usize = 8 * 1024 * 1024;
/// Largest source side, as a desktop texture allows; `MAX_DECODE_ALLOC` bounds memory.
const MAX_SOURCE_SIDE: u32 = 16_384;
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;
/// Peak conversion scratch, allowing an ordinary 2048-square source on the serial worker.
const MAX_WORKING_ALLOC: u64 = MAX_DECODE_ALLOC * 2;
/// Largest side artwork keeps; bigger sources scale down, smaller stay native.
const MAX_ARTWORK_SIDE: u32 = 512;
const GUTTER: u32 = 1;
const MAX_ARTWORKS: usize = 64;
/// Longest side kept for a list thumbnail (server logos, gamerpics, badges), so
/// a whole featured list fits the art pages beside banners.
pub(crate) const THUMBNAIL_SIDE: u32 = 128;
/// The start screen's title texture, which Cinnabar's own logo replaces.
pub(super) const TITLE_KEY: &str = "textures/ui/title";
/// Prefix of a server-pack texture's full-resolution copy on the art pages, so
/// a pack's `textures/ui/title` never collides with Cinnabar's logo.
pub(super) const SERVER_ART_PREFIX: &str = "server-pack:";
/// Cinnabar's logo; the pack's title draws only if this fails to decode.
pub(crate) const BUILT_IN_TITLE: &[u8] = include_bytes!("../../../../assets/branding/title.png");

#[derive(Default)]
pub(super) struct MenuArtworkAtlas {
    pub(super) pages: Vec<render::UiTexturePage>,
    pub(super) refs: HashMap<String, IconRef>,
}

/// Decoded artwork: straight-alpha RGBA8, which the UI shader premultiplies, and its size.
struct Artwork {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

/// What an atlas is packed from: the service art at its sides plus the
/// engine's oversized textures by key.
#[derive(Clone, Default)]
pub(super) struct ArtworkSet {
    pub(super) paths: Vec<(String, u32)>,
    pub(super) oversized: Vec<(String, Arc<[u8]>)>,
}

impl ArtworkSet {
    /// Equality without comparing texture bytes: engine textures by key and payload.
    pub(super) fn same(&self, other: &Self) -> bool {
        self.paths == other.paths
            && self.oversized.len() == other.oversized.len()
            && self
                .oversized
                .iter()
                .zip(&other.oversized)
                .all(|(a, b)| a.0 == b.0 && Arc::ptr_eq(&a.1, &b.1))
    }
}

struct Request {
    id: u64,
    set: ArtworkSet,
}

/// A packed atlas whose refs name pages from 0, rebased when installed.
pub(super) struct Packed {
    id: u64,
    /// Every decodable source is in; a partial atlas precedes it.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "only tests wait for the final atlas")
    )]
    complete: bool,
    pub(super) pages: Vec<render::UiTexturePage>,
    pub(super) refs: HashMap<String, IconRef>,
}

/// Decodes and packs launcher art on a worker so a changed art set never
/// stalls a frame; until an atlas arrives the previous one keeps drawing.
pub(super) struct ArtworkLoader {
    requests: Sender<Request>,
    results: Receiver<Packed>,
    requested: u64,
    ready: Option<Packed>,
    /// The installed atlas's refs, page-relative, for rebasing.
    pub(super) relative: HashMap<String, IconRef>,
}

impl Default for ArtworkLoader {
    fn default() -> Self {
        let (requests, jobs) = crossbeam_channel::unbounded();
        let (done, results) = crossbeam_channel::unbounded();
        let spawned = std::thread::Builder::new()
            .name("menu-artwork".to_owned())
            .spawn(move || serve(&jobs, &done));
        if let Err(error) = spawned {
            bevy::log::warn!("menu artwork worker unavailable: {error}");
        }
        // The title is packed before the first frame so the pack's own never flashes.
        let title = pack(&ArtworkSet::default(), &DecodeCache::default(), 0, true);
        Self {
            requests,
            results,
            requested: 0,
            relative: HashMap::new(),
            ready: Some(title),
        }
    }
}

impl ArtworkLoader {
    /// Asks the worker for `set`'s atlas, superseding any pending request.
    pub(super) fn request(&mut self, set: ArtworkSet) {
        self.requested += 1;
        self.ready = None;
        let _ = self.requests.send(Request {
            id: self.requested,
            set,
        });
    }

    /// Whether an atlas for the latest request (or a partial one) is ready to install.
    pub(super) fn poll(&mut self) -> bool {
        while let Ok(packed) = self.results.try_recv() {
            if packed.id == self.requested {
                self.ready = Some(packed);
            }
        }
        self.ready.is_some()
    }

    /// The ready atlas, which becomes the installed one.
    pub(super) fn take(&mut self) -> Option<Packed> {
        let packed = self.ready.take()?;
        self.relative.clone_from(&packed.refs);
        Some(packed)
    }

    /// Blocks until the latest request's complete atlas is ready.
    #[cfg(test)]
    pub(super) fn wait(&mut self) {
        let done = |ready: &Option<Packed>, requested| {
            ready
                .as_ref()
                .is_some_and(|packed| packed.complete && packed.id == requested)
        };
        while !done(&self.ready, self.requested) {
            let Ok(packed) = self
                .results
                .recv_timeout(std::time::Duration::from_secs(30))
            else {
                return;
            };
            if packed.id == self.requested {
                self.ready = Some(packed);
            }
        }
    }
}

/// `refs` moved onto pages starting at `first_page`.
pub(super) fn rebase(refs: &HashMap<String, IconRef>, first_page: u16) -> HashMap<String, IconRef> {
    refs.iter()
        .map(|(path, icon)| {
            let mut icon = *icon;
            icon.page = icon.page.saturating_add(first_page);
            (path.clone(), icon)
        })
        .collect()
}

/// Source identity, requested size, and optional pack payload hash.
type DecodeKey = (String, u32, Option<[u8; 32]>);

/// Decoded art by source and side, plus sources that failed (both bounded).
#[derive(Default)]
struct DecodeCache {
    decoded: HashMap<DecodeKey, Arc<Artwork>>,
    failed: VecDeque<DecodeKey>,
}

const MAX_DECODED: usize = 160;
const MAX_FAILED: usize = 256;
/// Art decoded before the first partial atlas goes out, so visible rows fill early.
const FIRST_BATCH: usize = 8;

fn serve(jobs: &Receiver<Request>, done: &Sender<Packed>) {
    let mut cache = DecodeCache::default();
    while let Ok(mut request) = jobs.recv() {
        'request: loop {
            // Only the newest request matters; older ones are obsolete.
            while let Ok(newer) = jobs.try_recv() {
                request = newer;
            }
            let missing = cache.missing(&request.set);
            let mut decoded = 0;
            for batch in missing.chunks(FIRST_BATCH) {
                cache.decode(batch, &request.set);
                decoded += batch.len();
                if !jobs.is_empty() {
                    continue 'request;
                }
                if decoded == FIRST_BATCH && missing.len() > FIRST_BATCH {
                    let _ = done.send(pack(&request.set, &cache, request.id, false));
                }
            }
            let _ = done.send(pack(&request.set, &cache, request.id, true));
            cache.trim(&request.set);
            break;
        }
    }
}

/// A source to decode: a file path, or an engine texture's bytes, at a side.
#[derive(Clone)]
enum Source {
    File(String, u32),
    Bytes(String, Arc<[u8]>),
}

impl Source {
    /// Includes replacement pack bytes so a reload cannot reuse an older image.
    fn key(&self) -> DecodeKey {
        match self {
            Self::File(path, side) => (path.clone(), *side, None),
            Self::Bytes(key, bytes) => {
                use sha2::{Digest, Sha256};
                (key.clone(), WHOLE_PAGE, Some(Sha256::digest(bytes).into()))
            }
        }
    }
}

const WHOLE_PAGE: u32 = render::UI_ART_PAGE_SIDE - GUTTER * 2;

impl DecodeCache {
    fn missing(&self, set: &ArtworkSet) -> Vec<Source> {
        sources(set)
            .into_iter()
            .filter(|source| {
                let key = source.key();
                !self.decoded.contains_key(&key) && !self.failed.contains(&key)
            })
            .collect()
    }

    /// Keep large image scratch allocations on the existing artwork worker.
    fn decode(&mut self, batch: &[Source], set: &ArtworkSet) {
        let results: Vec<_> = batch
            .iter()
            .map(|source| {
                let art = match source {
                    Source::File(path, side) => decode(Path::new(path), *side),
                    Source::Bytes(_, bytes) => decode_bytes(bytes, WHOLE_PAGE),
                };
                (source.key(), art)
            })
            .collect();
        for (key, art) in results {
            match art {
                Some((pixels, width, height)) => {
                    let art = Artwork {
                        width,
                        height,
                        pixels,
                    };
                    // A replacement image supersedes the prior bytes at this path and size.
                    self.decoded
                        .retain(|old, _| old.0 != key.0 || old.1 != key.1);
                    self.decoded.insert(key, Arc::new(art));
                }
                None => {
                    if self.failed.len() >= MAX_FAILED {
                        self.failed.pop_front();
                    }
                    self.failed.push_back(key);
                }
            }
        }
        // Cancellation must not bypass eviction after a finished batch.
        self.trim(set);
    }

    /// Drops decoded art `set` no longer names once the cache outgrows its bound.
    fn trim(&mut self, set: &ArtworkSet) {
        if self.decoded.len() <= MAX_DECODED {
            return;
        }
        let wanted: BTreeSet<_> = sources(set).iter().map(Source::key).collect();
        self.decoded.retain(|key, _| wanted.contains(key));
    }
}

/// `set`'s distinct sources in draw priority, capped like the atlas.
fn sources(set: &ArtworkSet) -> Vec<Source> {
    let mut unique = BTreeSet::new();
    let files = set
        .paths
        .iter()
        .filter(|(path, _)| !path.is_empty() && unique.insert(path.clone()))
        .map(|(path, side)| Source::File(path.clone(), (*side).min(MAX_ARTWORK_SIDE)));
    let engine = set
        .oversized
        .iter()
        .map(|(key, bytes)| Source::Bytes(format!("{SERVER_ART_PREFIX}{key}"), Arc::clone(bytes)));
    let mut all: Vec<_> = files.take(MAX_ARTWORKS).collect();
    let remaining = MAX_ARTWORKS - all.len();
    all.extend(
        engine
            .filter(|source| unique.insert(source.key().0))
            .take(remaining),
    );
    all
}

/// The built-in title, decoded once per process.
fn title() -> Option<&'static Artwork> {
    static TITLE: std::sync::OnceLock<Option<Artwork>> = std::sync::OnceLock::new();
    TITLE
        .get_or_init(|| {
            decode_bytes(BUILT_IN_TITLE, WHOLE_PAGE).map(|(pixels, width, height)| Artwork {
                width,
                height,
                pixels,
            })
        })
        .as_ref()
}

/// Shelf-packs `set`'s decoded art into art pages numbered from 0; what is
/// not decoded yet or does not fit is left out.
fn pack(set: &ArtworkSet, cache: &DecodeCache, id: u64, complete: bool) -> Packed {
    let side = render::UI_ART_PAGE_SIDE;
    let mut rest: Vec<(String, &Artwork)> = sources(set)
        .iter()
        .filter_map(|source| {
            let key = source.key();
            let art = cache.decoded.get(&key)?;
            Some((key.0, art.as_ref()))
        })
        .collect();
    rest.sort_by(|a, b| b.1.height.cmp(&a.1.height).then(a.0.cmp(&b.0)));
    // The title packs first so later art can never crowd it out.
    let decoded: Vec<(String, &Artwork)> = title()
        .map(|art| (TITLE_KEY.to_owned(), art))
        .into_iter()
        .chain(rest)
        .collect();
    let page_bytes = side as usize * side as usize * 4;
    let mut buffers: Vec<Vec<u8>> = Vec::new();
    let mut refs = HashMap::with_capacity(decoded.len());
    let (mut page, mut x, mut y, mut shelf) = (0usize, GUTTER, GUTTER, 0u32);
    for (path, art) in decoded {
        if x + art.width + GUTTER > side {
            x = GUTTER;
            y += shelf + GUTTER;
            shelf = 0;
        }
        if y + art.height + GUTTER > side {
            page += 1;
            x = GUTTER;
            y = GUTTER;
            shelf = 0;
        }
        if page >= render::MAX_UI_ART_PAGES {
            break;
        }
        while buffers.len() <= page {
            buffers.push(vec![0; page_bytes]);
        }
        let row_bytes = art.width as usize * 4;
        for row in 0..art.height as usize {
            let target = ((y as usize + row) * side as usize + x as usize) * 4;
            buffers[page][target..target + row_bytes]
                .copy_from_slice(&art.pixels[row * row_bytes..(row + 1) * row_bytes]);
        }
        let (left, top) = (x as u16, y as u16);
        refs.insert(
            path,
            IconRef {
                page: page as u16,
                uv: [left, top, left + art.width as u16, top + art.height as u16],
                glint: false,
            },
        );
        x += art.width + GUTTER;
        shelf = shelf.max(art.height);
    }
    let pages = buffers
        .into_iter()
        .map(|pixels| {
            render::UiTexturePage::owned([side, side], Arc::from(pixels))
                .expect("art pages have exact checked dimensions")
        })
        .collect();
    Packed {
        id,
        complete,
        pages,
        refs,
    }
}

fn decode(path: &Path, max_side: u32) -> Option<(Vec<u8>, u32, u32)> {
    let file = File::open(path).ok()?;
    let mut bytes = Vec::new();
    file.take((MAX_SOURCE_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > MAX_SOURCE_BYTES {
        return None;
    }
    decode_bytes(&bytes, max_side.min(MAX_ARTWORK_SIDE))
}

/// Straight-alpha RGBA8 (what the UI shader samples) of an image no larger
/// than `max_side` on either axis; a downscale filters premultiplied so
/// transparent texels never bleed into edges.
fn decode_bytes(bytes: &[u8], max_side: u32) -> Option<(Vec<u8>, u32, u32)> {
    if bytes.is_empty() || bytes.len() > MAX_SOURCE_BYTES || max_side == 0 {
        return None;
    }
    let format = image::guess_format(bytes).ok()?;
    let dimensions = ImageReader::with_format(Cursor::new(bytes), format)
        .into_dimensions()
        .ok()?;
    if dimensions.0 == 0
        || dimensions.1 == 0
        || dimensions.0 > MAX_SOURCE_SIDE
        || dimensions.1 > MAX_SOURCE_SIDE
    {
        return None;
    }
    // Budget decoder output, RGBA32F conversion, the separable resize's
    // intermediate and final buffers, and the RGBA8 result before allocating.
    let pixels = u64::from(dimensions.0).checked_mul(u64::from(dimensions.1))?;
    let scale = f64::from(max_side) / f64::from(dimensions.0.max(dimensions.1));
    let fitted = |side: u32| {
        if scale >= 1.0 {
            side
        } else {
            ((f64::from(side) * scale).round() as u32).clamp(1, max_side)
        }
    };
    let [output_width, output_height] = [fitted(dimensions.0), fitted(dimensions.1)].map(u64::from);
    let conversion = pixels.checked_mul(32)?;
    let resize = pixels
        .checked_mul(16)?
        .checked_add(
            u64::from(dimensions.0)
                .checked_mul(output_height)?
                .checked_mul(16)?,
        )?
        .checked_add(output_width.checked_mul(output_height)?.checked_mul(20)?)?;
    if conversion.max(resize) > MAX_WORKING_ALLOC {
        return None;
    }
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_SIDE);
    limits.max_image_height = Some(MAX_SOURCE_SIDE);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let image = reader.decode().ok()?.into_rgba32f();
    if image.width() <= max_side && image.height() <= max_side {
        let image = image::DynamicImage::ImageRgba32F(image).into_rgba8();
        let (width, height) = image.dimensions();
        return Some((image.into_raw(), width, height));
    }
    let mut premultiplied = image;
    for pixel in premultiplied.pixels_mut() {
        let alpha = pixel[3];
        pixel[0] *= alpha;
        pixel[1] *= alpha;
        pixel[2] *= alpha;
    }
    let scale = f64::from(max_side) / f64::from(premultiplied.width().max(premultiplied.height()));
    let size = |side: u32| ((f64::from(side) * scale).round() as u32).clamp(1, max_side);
    let (width, height) = (size(premultiplied.width()), size(premultiplied.height()));
    let mut resized = image::imageops::resize(&premultiplied, width, height, FilterType::Lanczos3);
    for pixel in resized.pixels_mut() {
        let alpha = pixel[3].clamp(0.0, 1.0);
        for channel in 0..3 {
            pixel[channel] = if alpha > 0.0 {
                (pixel[channel] / alpha).clamp(0.0, 1.0)
            } else {
                0.0
            };
        }
        pixel[3] = alpha;
    }
    let image = image::DynamicImage::ImageRgba32F(resized).into_rgba8();
    Some((image.into_raw(), width, height))
}

/// Every downloaded artwork path the menu view can draw.
pub(super) fn view_paths(view: &crate::menu::MenuView) -> Vec<(String, u32)> {
    if view.screen == crate::menu::MenuScreen::Profile {
        return profile_art(view);
    }
    // The Servers tab shows the first experience until a server is picked.
    let shown = match view.feeds.selected_saved {
        Some(_) => None,
        None => Some(view.feeds.selected_featured.unwrap_or(0)),
    };
    let selected = shown
        .and_then(|index| {
            view.featured
                .iter()
                .chain(view.gatherings.iter())
                .nth(index)
        })
        .and_then(|server| view.feeds.details.get(&server.address));
    let thumbnails = view
        .featured
        .iter()
        .chain(view.gatherings.iter())
        .map(|server| server.image_path.clone())
        .chain(std::iter::once(view.feeds.profile.picture_path.clone()))
        .map(|path| (path, THUMBNAIL_SIDE));
    let full = home_art(&view.feeds.home)
        .into_iter()
        .chain(std::iter::once(view.feeds.profile.avatar_path.clone()))
        .chain(std::iter::once(
            view.feeds.profile.featured_screenshot_path.clone(),
        ))
        .chain(selected.into_iter().flat_map(|details| {
            details
                .screenshots
                .iter()
                .cloned()
                .chain(details.games.iter().map(|game| game.image_path.clone()))
        }))
        .chain(
            view.store
                .as_deref()
                .map(crate::store::StoreSnapshot::image_paths)
                .unwrap_or_default(),
        )
        .map(|path| (path, MAX_ARTWORK_SIDE));
    view.global_resources
        .icons
        .values()
        .cloned()
        .map(|path| (path, THUMBNAIL_SIDE))
        .chain(thumbnails)
        .chain(full)
        .filter(|(path, _)| !path.is_empty())
        .collect()
}

/// Queues Profile card art first, followed by only the achievements Overview draws.
fn profile_art(view: &crate::menu::MenuView) -> Vec<(String, u32)> {
    let profile = &view.feeds.profile;
    let mut paths = vec![
        (profile.avatar_path.clone(), MAX_ARTWORK_SIDE),
        (profile.featured_screenshot_path.clone(), MAX_ARTWORK_SIDE),
        (profile.picture_path.clone(), THUMBNAIL_SIDE),
        (view.feeds.home.persona_head.clone(), THUMBNAIL_SIDE),
    ];
    if view.profile_tab == ui::ProfileTab::Overview
        && profile.achievements_loaded
        && !profile.achievements_error
        && let Some(summary) = &profile.achievements
    {
        let sections = crate::menu::profile_achievements::visible_achievements(&summary.entries);
        paths.extend(
            sections
                .into_iter()
                .flatten()
                .map(|entry| (entry.image.path.clone(), THUMBNAIL_SIDE)),
        );
    }
    paths.retain(|(path, _)| !path.is_empty());
    paths
}

/// The start screen's service art: messaging tile layers, the event badge and the persona head.
fn home_art(home: &crate::menu::MenuHome) -> Vec<String> {
    let mut paths = vec![home.persona_head.clone()];
    for art in [&home.play_art, &home.store_art].into_iter().flatten() {
        paths.extend([
            art.banner_texture.clone(),
            art.default_background.clone(),
            art.hover_background.clone(),
            art.default_foreground.clone(),
            art.hover_foreground.clone(),
        ]);
    }
    if let Some(event) = &home.live_event {
        paths.push(event.badge_path.clone());
    }
    paths
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_replacement_atlas_preserves_the_portrait_fallback() {
        let path = std::env::temp_dir().join(format!(
            "cinnabar-profile-portrait-{}-{}.png",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, png(8, 8, [40, 80, 120, 255])).unwrap();
        let portrait = path.to_string_lossy().into_owned();
        let mut view = crate::menu::MenuRuntime::new(true, 2, "Fixture Player".into()).view();
        view.auth_state = crate::menu::auth::AuthState::Authenticated;
        view.feeds.profile.loaded = true;
        view.feeds.profile.avatar_loaded = true;
        view.feeds.profile.featured_screenshot_loaded = true;
        view.feeds.home.persona_head = portrait.clone();
        let home = ArtworkSet {
            paths: view_paths(&view),
            ..Default::default()
        };
        let mut cache = DecodeCache::default();
        cache.decode(&cache.missing(&home), &home);
        assert!(pack(&home, &cache, 0, true).refs.contains_key(&portrait));
        view.screen = crate::menu::MenuScreen::Profile;
        for tab in [ui::ProfileTab::Overview, ui::ProfileTab::Stats] {
            view.profile_tab = tab;
            // A missing gamerpic and a failed gamerpic decode both use the head.
            for gamerpic in [String::new(), format!("{portrait}.missing")] {
                view.feeds.profile.picture_path = gamerpic;
                let profile = ArtworkSet {
                    paths: view_paths(&view),
                    ..Default::default()
                };
                cache.decode(&cache.missing(&profile), &profile);
                let replacement = pack(&profile, &cache, 1, true);
                assert!(
                    replacement.refs.contains_key(&portrait),
                    "Profile {tab:?} replaced the atlas without its portrait fallback"
                );
            }
        }
        std::fs::remove_file(path).unwrap();
    }

    /// Encodes a solid test image without any external assets.
    fn png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::from_pixel(width, height, image::Rgba(pixel))
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    /// Every decode batch can be superseded before an atlas finishes packing.
    #[test]
    fn superseded_artwork_batches_keep_the_decode_cache_bounded() {
        let mut cache = DecodeCache::default();
        for revision in 0..2000_u32 {
            let color = [revision as u8, (revision >> 8) as u8, 20, 255];
            let set = ArtworkSet {
                oversized: vec![("changing".into(), png(8, 8, color).into())],
                ..Default::default()
            };
            cache.decode(&cache.missing(&set), &set);
            assert_eq!(cache.decoded.len(), 1);
            let source = sources(&set).pop().unwrap();
            assert_eq!(&cache.decoded[&source.key()].pixels[..4], &color);
        }
    }

    /// Churning distinct paths exercises eviction even when every batch is cancelled.
    #[test]
    fn cancelled_artwork_batches_evict_obsolete_paths() {
        let mut cache = DecodeCache::default();
        let bytes: Arc<[u8]> = png(8, 8, [10, 20, 30, 255]).into();
        for revision in 0..2000 {
            let set = ArtworkSet {
                oversized: vec![(format!("changing-{revision}"), Arc::clone(&bytes))],
                ..Default::default()
            };
            cache.decode(&cache.missing(&set), &set);
            assert!(cache.decoded.len() <= MAX_DECODED);
        }
    }

    #[test]
    fn replacing_pack_bytes_invalidates_decoded_artwork() {
        let mut cache = DecodeCache::default();
        for color in [[200, 30, 40, 255], [20, 220, 30, 255]] {
            let set = ArtworkSet {
                oversized: vec![(TITLE_KEY.to_owned(), png(900, 300, color).into())],
                ..Default::default()
            };
            cache.decode(&cache.missing(&set), &set);
            let atlas = pack(&set, &cache, 0, true);
            let art = atlas.refs[&format!("{SERVER_ART_PREFIX}{TITLE_KEY}")];
            let page = &atlas.pages[usize::from(art.page)];
            let at =
                (u32::from(art.uv[1]) * page.dimensions()[0] + u32::from(art.uv[0])) as usize * 4;
            assert_eq!(&page.pixels()[at..at + 4], &color);
        }
    }

    #[test]
    fn a_superseded_prepared_atlas_cannot_be_installed() {
        let mut loader = ArtworkLoader::default();
        assert!(loader.ready.is_some());
        loader.request(ArtworkSet::default());
        assert!(loader.ready.is_none());
        assert!(loader.take().is_none());
    }

    // A large server texture (Zeqa's 1992x669 title) keeps a whole art page of
    // detail under its own key, not the 256px server-page downscale.
    #[test]
    fn oversized_server_textures_keep_full_resolution() {
        let bytes: std::sync::Arc<[u8]> = png(1992, 669, [200, 30, 40, 255]).into();
        let set = ArtworkSet {
            paths: Vec::new(),
            oversized: vec![(TITLE_KEY.to_owned(), bytes)],
        };
        let mut cache = DecodeCache::default();
        cache.decode(&cache.missing(&set), &set);
        let atlas = pack(&set, &cache, 0, true);
        let art = atlas.refs[&format!("{SERVER_ART_PREFIX}{TITLE_KEY}")];
        let [u0, v0, u1, v1] = art.uv;
        assert_eq!([u1 - u0, v1 - v0], [1022, 343]);
        // Cinnabar's logo keeps the plain title key.
        assert_ne!(atlas.refs[TITLE_KEY].uv, art.uv);
    }

    // Artwork stays straight alpha, as the UI shader samples it.
    #[test]
    fn artwork_is_straight_alpha() {
        let (pixels, _, _) = decode_bytes(&png(4, 4, [200, 100, 50, 128]), 64).unwrap();
        assert_eq!(&pixels[..4], &[200, 100, 50, 128]);
        let (scaled, width, _) = decode_bytes(&png(128, 128, [200, 100, 50, 128]), 64).unwrap();
        assert_eq!(width, 64);
        assert!(
            scaled[..3]
                .iter()
                .zip([200, 100, 50])
                .all(|(a, b)| a.abs_diff(b) <= 1)
        );
    }
    #[test]
    fn grayscale_conversion_obeys_the_decode_memory_budget() {
        let mut bytes = Vec::new();
        image::GrayImage::new(4096, 4096)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        assert!(decode_bytes(&bytes, 128).is_none());
    }

    #[test]
    fn oversized_source_sets_cannot_defeat_the_decode_cache_bound() {
        let bytes: Arc<[u8]> = png(2, 2, [255; 4]).into();
        let set = ArtworkSet {
            oversized: (0..MAX_DECODED + 1)
                .map(|i| (format!("texture-{i}"), bytes.clone()))
                .collect(),
            ..Default::default()
        };
        let mut cache = DecodeCache::default();
        let missing = cache.missing(&set);
        assert!(missing.len() <= MAX_ARTWORKS);
        cache.decode(&missing, &set);
        assert!(cache.decoded.len() <= MAX_DECODED);
    }

    #[test]
    fn repeated_artwork_paths_do_not_exclude_later_unique_sources() {
        let set = ArtworkSet {
            paths: std::iter::repeat_n(("same".into(), 128), MAX_ARTWORKS)
                .chain([("later".into(), 128)])
                .collect(),
            ..Default::default()
        };
        assert_eq!(sources(&set).len(), 2);
    }
}
