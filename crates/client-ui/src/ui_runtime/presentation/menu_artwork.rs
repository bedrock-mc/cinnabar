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

use super::UiPresentationRuntime;
use ui::IconRef;

mod skin_previews;
use skin_previews::SkinArtwork;
pub(crate) use skin_previews::thumbnail_key;
mod cape_previews;
mod packing;
mod request_cache;
use cape_previews::CapeArtwork;
pub(crate) use cape_previews::{cape_texture_key, cape_thumbnail_key};
use packing::pack;

use launcher::accounts::MAX_ARTWORK_BYTES as MAX_SOURCE_BYTES;
/// Largest source side, as a desktop texture allows; `MAX_DECODE_ALLOC` bounds memory.
const MAX_SOURCE_SIDE: u32 = 16_384;
const MAX_DECODE_ALLOC: u64 = 64 * 1024 * 1024;
/// Peak conversion scratch, allowing an ordinary 2048-square source on the serial worker.
const MAX_WORKING_ALLOC: u64 = MAX_DECODE_ALLOC * 2;
/// Largest side artwork keeps; bigger sources scale down, smaller stay native.
const MAX_ARTWORK_SIDE: u32 = 512;
const SERVER_BANNER_SIDE: u32 = 960;
const SERVER_ACTIVITY_SIDE: u32 = 256;
const GUTTER: u32 = 1;
const MAX_ARTWORKS: usize = 64;
/// Longest side kept for a list thumbnail (server logos, gamerpics, badges), so
/// a whole featured list fits the art pages beside banners.
pub const THUMBNAIL_SIDE: u32 = 128;
/// Marketplace art sides: a card thumbnail, and the offer page's key art and screenshots. Both
/// keep 16:9 art several to a 1024 art page next to the title, so a screen's images all pack.
const STORE_CARD_SIDE: u32 = 192;
const STORE_FEATURE_SIDE: u32 = 480;
/// The start screen's title texture, which Cinnabar's own logo replaces.
pub(super) const TITLE_KEY: &str = "textures/ui/title";
/// Prefix of a server-pack texture's full-resolution copy on the art pages, so
/// a pack's `textures/ui/title` never collides with Cinnabar's logo.
pub(super) const SERVER_ART_PREFIX: &str = "server-pack:";
/// Cinnabar's logo; the pack's title draws only if this fails to decode.
pub const BUILT_IN_TITLE: &[u8] = include_bytes!("../../../../../assets/branding/title.png");

#[derive(Default)]
pub(super) struct MenuArtworkAtlas {
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
    pub(super) skins: Vec<SkinArtwork>,
    pub(super) capes: Vec<CapeArtwork>,
}

impl ArtworkSet {
    /// Equality without comparing texture bytes: engine textures by key and payload.
    pub(super) fn same(&self, other: &Self) -> bool {
        self.paths == other.paths
            && self.capes.len() == other.capes.len()
            && self.capes.iter().zip(&other.capes).all(|(a, b)| a.same(b))
            && self.skins.len() == other.skins.len()
            && self.skins.iter().zip(&other.skins).all(|(a, b)| a.same(b))
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
        not(any(test, feature = "test-support")),
        expect(dead_code, reason = "only tests wait for the final atlas")
    )]
    complete: bool,
    pub(super) pages: Vec<render_model::UiTexturePage>,
    pub(super) refs: HashMap<String, IconRef>,
}

/// Decodes and packs launcher art on a worker so a changed art set never
/// stalls a frame; until an atlas arrives the previous one keeps drawing.
pub(super) struct ArtworkLoader {
    requests: Sender<Request>,
    results: Receiver<Packed>,
    requested: u64,
    ready: Option<Packed>,
    gallery: Option<request_cache::GalleryRequest>,
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
            gallery: None,
        }
    }
}

impl ArtworkLoader {
    /// Asks the worker for `set`'s atlas, superseding any pending request.
    pub(super) fn request(&mut self, set: ArtworkSet) {
        self.gallery = None;
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

    /// Inspects pending pixels without retiring them before texture admission succeeds.
    pub(super) fn pending(&self) -> Option<&Packed> {
        self.ready.as_ref()
    }

    /// The admitted atlas, which becomes the installed one.
    pub(super) fn take(&mut self) -> Option<Packed> {
        let packed = self.ready.take()?;
        self.relative.clone_from(&packed.refs);
        Some(packed)
    }

    #[cfg(test)]
    pub(super) fn set_pending_fixture(
        &mut self,
        pages: Vec<render_model::UiTexturePage>,
        refs: HashMap<String, IconRef>,
    ) {
        self.ready = Some(Packed {
            id: self.requested,
            complete: true,
            pages,
            refs,
        });
    }

    /// Blocks until the latest request's complete atlas is ready.
    #[cfg(any(test, feature = "test-support"))]
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
    Skin(SkinArtwork),
    Cape(CapeArtwork),
}

impl Source {
    /// Includes replacement pack bytes so a reload cannot reuse an older image.
    fn key(&self) -> DecodeKey {
        match self {
            Self::Skin(skin) => skin.decode_key(),
            Self::Cape(cape) => cape.decode_key(),
            Self::File(path, side) => (path.clone(), *side, None),
            Self::Bytes(key, bytes) => {
                use sha2::{Digest, Sha256};
                (key.clone(), WHOLE_PAGE, Some(Sha256::digest(bytes).into()))
            }
        }
    }
}

const WHOLE_PAGE: u32 = render_model::UI_ART_PAGE_SIDE - GUTTER * 2;

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
                    Source::Skin(skin) => skin.decode(),
                    Source::Cape(cape) => cape.decode(),
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
        .map(|(path, side)| Source::File(path.clone(), (*side).min(SERVER_BANNER_SIDE)));
    let engine = set
        .oversized
        .iter()
        .map(|(key, bytes)| Source::Bytes(format!("{SERVER_ART_PREFIX}{key}"), Arc::clone(bytes)));
    let mut all: Vec<_> = set
        .capes
        .iter()
        .take(MAX_ARTWORKS)
        .cloned()
        .map(Source::Cape)
        .collect();
    let remaining = MAX_ARTWORKS - all.len();
    all.extend(set.skins.iter().take(remaining).cloned().map(Source::Skin));
    let remaining = MAX_ARTWORKS - all.len();
    all.extend(files.take(remaining));
    let remaining = MAX_ARTWORKS - all.len();
    all.extend(
        engine
            .filter(|source| unique.insert(source.key().0))
            .take(remaining),
    );
    all
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
    decode_bytes(&bytes, max_side.min(SERVER_BANNER_SIDE))
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
pub fn view_paths(view: &launcher::menu::MenuView) -> Vec<(String, u32)> {
    if view.screen == launcher::menu::MenuScreen::DressingRoom {
        return Vec::new();
    }
    if view.screen == launcher::menu::MenuScreen::Profile {
        return profile_art(view);
    }
    if view.screen == launcher::menu::MenuScreen::Store {
        return store_art(view);
    }
    // The invite screen draws only its friends' gamerpics, online list first.
    if let Some(invite) = view.invite.as_deref() {
        use launcher::menu::invite::Section;
        return [Section::Online, Section::Offline]
            .into_iter()
            .flat_map(|section| invite.section(section))
            .filter(|friend| !friend.picture_path.is_empty())
            .map(|friend| (friend.picture_path.clone(), THUMBNAIL_SIDE))
            .collect();
    }
    // The Servers tab shows the first experience until a server is picked.
    let shown = match view.feeds.selected_saved {
        Some(_) => None,
        None => Some(view.feeds.selected_featured.unwrap_or(0)),
    };
    let selected = shown
        .and_then(|index| view.featured.get(index))
        .and_then(|server| view.feeds.details.get(&server.address));
    let portraits = std::iter::once(view.feeds.profile.picture_path.clone())
        .chain(
            view.feeds
                .accounts
                .iter()
                .filter(|account| {
                    view.dialog == Some(launcher::menu::MenuDialog::Accounts)
                        || view.feeds.account_active_id.as_deref() == Some(account.id.as_str())
                })
                .filter_map(|account| account.picture_path.clone()),
        )
        .map(|path| (path, THUMBNAIL_SIDE));
    let thumbnails = view
        .featured
        .iter()
        .map(|server| (server.image_path.clone(), THUMBNAIL_SIDE));
    let full = home_art(&view.feeds.home)
        .into_iter()
        .chain(std::iter::once(view.feeds.profile.avatar_path.clone()))
        .chain(std::iter::once(
            view.feeds.profile.featured_screenshot_path.clone(),
        ))
        .map(|path| (path, MAX_ARTWORK_SIDE));
    if view.screen == launcher::menu::MenuScreen::Servers {
        let server_art = selected.into_iter().flat_map(|details| {
            let side = if details.games.len() > 8 {
                192
            } else {
                SERVER_ACTIVITY_SIDE
            };
            std::iter::once((details.banner.clone(), SERVER_BANNER_SIDE)).chain(
                details
                    .games
                    .iter()
                    .map(move |game| (game.image_path.clone(), side)),
            )
        });
        return portraits
            .chain(thumbnails)
            .chain(server_art)
            .filter(|(path, _)| !path.is_empty())
            .collect();
    }
    portraits
        .chain(
            view.global_resources
                .icons
                .values()
                .cloned()
                .map(|path| (path, THUMBNAIL_SIDE)),
        )
        .chain(thumbnails)
        .chain(full)
        .filter(|(path, _)| !path.is_empty())
        .collect()
}

/// Queues Profile card art first, followed by only the achievements Overview draws.
/// The Marketplace draws only its offer art, so the start screen's art does not take its pages.
fn store_art(view: &launcher::menu::MenuView) -> Vec<(String, u32)> {
    let Some(store) = view.store.as_deref() else {
        return Vec::new();
    };
    let mut paths = store.image_paths();
    // Feature art packs first: where a large image lands on the shelves decides how much else fits.
    paths.sort_by_key(|(_, art)| *art != launcher::store::StoreArt::Feature);
    paths
        .into_iter()
        .map(|(path, art)| {
            let side = match art {
                launcher::store::StoreArt::Card => STORE_CARD_SIDE,
                launcher::store::StoreArt::Feature => STORE_FEATURE_SIDE,
            };
            (path, side)
        })
        .collect()
}

fn profile_art(view: &launcher::menu::MenuView) -> Vec<(String, u32)> {
    let profile = &view.feeds.profile;
    let mut paths = vec![
        (profile.avatar_path.clone(), MAX_ARTWORK_SIDE),
        (profile.featured_screenshot_path.clone(), MAX_ARTWORK_SIDE),
        (profile.picture_path.clone(), THUMBNAIL_SIDE),
        (view.feeds.home.persona_head.clone(), THUMBNAIL_SIDE),
    ];
    if view.profile_tab == launcher::menu::ProfileTab::Overview
        && profile.achievements_loaded
        && !profile.achievements_error
        && let Some(summary) = &profile.achievements
    {
        let sections = launcher::menu::profile_achievements::visible_achievements(&summary.entries);
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
fn home_art(home: &launcher::menu::MenuHome) -> Vec<String> {
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
mod store_tests;

#[cfg(test)]
mod tests;
